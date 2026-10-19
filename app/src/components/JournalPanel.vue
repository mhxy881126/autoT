<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import type { JournalEntry, TPair } from "../lib/types";
import { money } from "../lib/format";
import { isTauri } from "../lib/ipc";
import { useDeskStore } from "../stores/desk";

const store = useDeskStore();
const day = computed(() => store.payload.meta.date);
const symbol = computed(() => store.config?.symbol ?? store.payload.meta.symbolId);
const pairs = computed<TPair[]>(() => store.ledger?.pairs ?? []);
const selectedId = ref<number | null>(null);
const existing = ref<JournalEntry | null>(null);
const saving = ref(false);
const msg = ref("");

const form = reactive({
  planned: true,
  planTarget: 0,
  planStop: 0,
  reason: "",
  emotion: "冷静",
  mistakes: [] as string[],
  note: ""
});

const EMOTIONS = ["冷静", "犹豫", "追涨", "恐慌", "报复", "其他"];
const MISTAKES = ["追高", "抄底过早", "未按信号", "超仓", "未止损", "情绪化", "计划外"];

function round2(v: number): number {
  return Math.round(v * 100) / 100;
}

function openPriceFor(p: TPair): number {
  return store.ledger?.fills.find((f) => f.id === p.openFillId)?.price ?? 0;
}

function defaultTarget(p: TPair, open: number): number {
  const pct = store.config?.targetLo ?? 0.0045;
  return open > 0 ? round2(open * (p.side === "正T" ? 1 + pct : 1 - pct)) : 0;
}

function defaultStop(p: TPair, open: number): number {
  const pct = store.config?.stopPct ?? 0.0035;
  return open > 0 ? round2(open * (p.side === "正T" ? 1 - pct : 1 + pct)) : 0;
}

function samePair(a: JournalEntry, p: TPair): boolean {
  return a.openFillId === p.openFillId && (a.closeFillId ?? 0) === (p.closeFillId ?? 0);
}

function entryFor(p: TPair): JournalEntry | undefined {
  return store.journal.find((e) => samePair(e, p));
}

function selectPair(p: TPair): void {
  selectedId.value = p.id;
  const e = entryFor(p);
  const open = openPriceFor(p);
  existing.value = e ?? null;
  form.planned = e?.planned ?? true;
  form.planTarget = e?.planTarget ?? defaultTarget(p, open);
  form.planStop = e?.planStop ?? defaultStop(p, open);
  form.reason = e?.reason ?? "";
  form.emotion = e?.emotion ?? "冷静";
  form.mistakes = [...(e?.mistakes ?? [])];
  form.note = e?.note ?? "";
  msg.value = "";
}

const selectedPair = computed(() => pairs.value.find((p) => p.id === selectedId.value) ?? null);

const preview = computed(() => {
  const p = selectedPair.value;
  if (!p) return null;
  const open = openPriceFor(p);
  if (open <= 0) return null;
  const dir = p.side === "正T" ? 1 : -1;
  const targetPnl =
    form.planTarget > 0 ? (form.planTarget - open) * dir * p.qty : null;
  const stopPnl = form.planStop > 0 ? (form.planStop - open) * dir * p.qty : null;
  const actual = p.net;
  const delta =
    actual != null && targetPnl != null ? actual - targetPnl : null;
  return { open, targetPnl, stopPnl, actual, delta };
});

async function load(): Promise<void> {
  if (!isTauri() || !symbol.value) return;
  await store.loadJournal(day.value, symbol.value);
}

onMounted(async () => {
  await load();
  if (pairs.value.length && selectedId.value === null) selectPair(pairs.value[0]);
});

watch([day, symbol], async () => {
  selectedId.value = null;
  existing.value = null;
  await load();
});

async function save(): Promise<void> {
  const p = selectedPair.value;
  if (!p || !isTauri()) return;
  saving.value = true;
  msg.value = "";
  try {
    await store.saveJournal({
      symbol: symbol.value,
      day: day.value,
      openFillId: p.openFillId,
      closeFillId: p.closeFillId,
      planTarget: form.planTarget > 0 ? Number(form.planTarget) : null,
      planStop: form.planStop > 0 ? Number(form.planStop) : null,
      planned: form.planned,
      reason: form.reason,
      emotion: form.emotion,
      mistakes: form.mistakes,
      note: form.note
    });
    const e = entryFor(p);
    existing.value = e ?? null;
    msg.value = "已保存";
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}

async function remove(): Promise<void> {
  if (!existing.value) return;
  try {
    await store.deleteJournal(existing.value.id, day.value, symbol.value);
    existing.value = null;
    form.reason = "";
    form.mistakes = [];
    form.note = "";
    form.planned = true;
    msg.value = "已删除";
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  }
}
</script>

<template>
  <div class="journal">
    <div class="head">交易日志 · <b>{{ symbol }}</b></div>

    <div v-if="!pairs.length" class="empty">
      今日还没有配对成交。先在「账务 / 回填」记录成交，再回来写日志。
    </div>

    <template v-else>
      <div class="list">
        <button
          v-for="p in pairs"
          :key="p.id"
          class="pair"
          :class="{ active: selectedId === p.id }"
          @click="selectPair(p)"
        >
          <span class="side">{{ p.side }}</span>
          <span class="qty">{{ p.qty }} 股</span>
          <span :class="(p.net ?? 0) >= 0 ? 'pos' : 'neg'">
            {{ p.status === "CLOSED" ? money(p.net ?? 0) : "未闭环" }}
          </span>
          <span v-if="entryFor(p)" class="dot" :class="{ ok: entryFor(p)?.planned, warn: !entryFor(p)?.planned }"></span>
        </button>
      </div>

      <div v-if="selectedPair" class="form">
        <div class="kv">
          <span>{{ selectedPair.side }} · {{ selectedPair.qty }} 股</span>
          <span>{{ selectedPair.status }}</span>
        </div>

        <label class="check">
          <input v-model="form.planned" type="checkbox" />
          计划内交易
        </label>

        <div class="row2">
          <label>目标价
            <input v-model.number="form.planTarget" type="number" step="0.01" />
          </label>
          <label>止损价
            <input v-model.number="form.planStop" type="number" step="0.01" />
          </label>
        </div>

        <div v-if="preview" class="preview">
          <span>开仓 {{ preview.open.toFixed(2) }}</span>
          <span>目标 <b :class="(preview.targetPnl ?? 0) >= 0 ? 'pos' : 'neg'">{{ preview.targetPnl == null ? "—" : money(preview.targetPnl) }}</b></span>
          <span>止损 <b :class="(preview.stopPnl ?? 0) >= 0 ? 'pos' : 'neg'">{{ preview.stopPnl == null ? "—" : money(preview.stopPnl) }}</b></span>
          <span>实际 <b :class="(preview.actual ?? 0) >= 0 ? 'pos' : 'neg'">{{ preview.actual == null ? "未平" : money(preview.actual) }}</b></span>
          <span>实际−目标 <b :class="(preview.delta ?? 0) >= 0 ? 'pos' : 'neg'">{{ preview.delta == null ? "—" : money(preview.delta) }}</b></span>
        </div>

        <label>理由
          <input v-model="form.reason" placeholder="为什么做这笔 T" />
        </label>

        <label>情绪
          <select v-model="form.emotion">
            <option v-for="e in EMOTIONS" :key="e" :value="e">{{ e }}</option>
          </select>
        </label>

        <div class="lbl">错误标签</div>
        <div class="tags">
          <label v-for="m in MISTAKES" :key="m" class="tag">
            <input v-model="form.mistakes" type="checkbox" :value="m" />
            {{ m }}
          </label>
        </div>

        <label>备注
          <textarea v-model="form.note" rows="3" placeholder="当时怎么想、事后怎么看" />
        </label>

        <div class="actions">
          <button class="btn" :disabled="saving" @click="save">
            {{ saving ? "保存中…" : existing ? "更新日志" : "保存日志" }}
          </button>
          <button v-if="existing" class="btn ghost" @click="remove">删除</button>
        </div>
        <div v-if="msg" class="msg">{{ msg }}</div>
      </div>
    </template>

    <div v-if="store.journalStats" class="stats">
      <div class="lbl">今日统计</div>
      <div class="kv">
        <span>日志 {{ store.journalStats.entries }} 笔</span>
        <span>计划内 {{ store.journalStats.planned }}</span>
        <span>执行率 {{ (store.journalStats.adherence * 100).toFixed(0) }}%</span>
      </div>

      <div class="attr">
        <div class="attr-row head">
          <span>盈亏归因</span>
          <span>笔数</span>
          <span>净盈亏</span>
          <span>均值</span>
          <span>胜率</span>
        </div>
        <div class="attr-row">
          <span>计划内</span>
          <span>{{ store.journalStats.plannedPnl.trades }}</span>
          <span :class="store.journalStats.plannedPnl.net >= 0 ? 'pos' : 'neg'">
            {{ money(store.journalStats.plannedPnl.net) }}
          </span>
          <span :class="store.journalStats.plannedPnl.avg >= 0 ? 'pos' : 'neg'">
            {{ money(store.journalStats.plannedPnl.avg) }}
          </span>
          <span>{{ (store.journalStats.plannedPnl.winRate * 100).toFixed(0) }}%</span>
        </div>
        <div class="attr-row">
          <span>计划外</span>
          <span>{{ store.journalStats.unplannedPnl.trades }}</span>
          <span :class="store.journalStats.unplannedPnl.net >= 0 ? 'pos' : 'neg'">
            {{ money(store.journalStats.unplannedPnl.net) }}
          </span>
          <span :class="store.journalStats.unplannedPnl.avg >= 0 ? 'pos' : 'neg'">
            {{ money(store.journalStats.unplannedPnl.avg) }}
          </span>
          <span>{{ (store.journalStats.unplannedPnl.winRate * 100).toFixed(0) }}%</span>
        </div>
      </div>

      <div class="delta">
        计划内均值 − 计划外均值 =
        <b :class="store.journalStats.pnlDeltaAvg >= 0 ? 'pos' : 'neg'">
          {{ money(store.journalStats.pnlDeltaAvg) }}
        </b>
      </div>

      <div v-if="store.journalStats.planTrades" class="plan-box">
        <span>计划目标 {{ money(store.journalStats.planTargetNet) }}</span>
        <span>计划止损 {{ money(store.journalStats.planStopNet) }}</span>
        <span>
          实际−目标
          <b :class="store.journalStats.actualVsTarget >= 0 ? 'pos' : 'neg'">
            {{ money(store.journalStats.actualVsTarget) }}
          </b>
        </span>
        <span class="dim">{{ store.journalStats.planTrades }} 笔有目标</span>
      </div>

      <div v-if="store.journalStats.mistakePnl.length" class="mis">
        <div class="lbl">错误标签盈亏</div>
        <div v-for="m in store.journalStats.mistakePnl" :key="m.name" class="mis-row">
          <span>{{ m.name }} ×{{ m.count }}</span>
          <span :class="m.net >= 0 ? 'pos' : 'neg'">{{ money(m.net) }}</span>
          <span class="avg">{{ money(m.avg) }}/笔</span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.journal {
  font-size: 11px;
  color: #c3cddd;
}
.head {
  font-size: 12px;
  color: #e8eef8;
  margin-bottom: 6px;
}
.empty {
  color: #4d5a70;
  padding: 14px 4px;
  line-height: 1.8;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 8px;
}
.pair {
  display: flex;
  align-items: center;
  gap: 8px;
  background: #0e131c;
  border: 1px solid #1d2431;
  border-radius: 4px;
  padding: 5px 8px;
  color: #9aa7bb;
  font: 10px Consolas, monospace;
  cursor: pointer;
  text-align: left;
}
.pair.active {
  border-color: rgba(232, 179, 57, 0.5);
  background: rgba(232, 179, 57, 0.06);
}
.side {
  color: #dbe2ee;
}
.qty {
  color: #6b7688;
}
.dot {
  margin-left: auto;
  width: 6px;
  height: 6px;
  border-radius: 50%;
}
.dot.ok {
  background: #67d6a3;
}
.dot.warn {
  background: #e8b339;
}
.form {
  border-top: 1px solid #1d2431;
  padding-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.kv {
  display: flex;
  gap: 10px;
  color: #76839a;
  font-size: 10px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 10px;
  color: #76839a;
}
label.check {
  flex-direction: row;
  align-items: center;
  gap: 6px;
  color: #c3cddd;
}
input,
select,
textarea {
  background: #0b1017;
  border: 1px solid #28313f;
  border-radius: 3px;
  color: #dbe2ee;
  padding: 4px 6px;
  font: 11px Consolas, monospace;
}
textarea {
  resize: vertical;
}
.lbl {
  font-size: 10px;
  color: #76839a;
  margin-top: 2px;
}
.tags {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}
.tag {
  flex-direction: row;
  align-items: center;
  gap: 4px;
  background: #151c27;
  border: 1px solid #28313f;
  border-radius: 3px;
  padding: 2px 6px;
  color: #9aa7bb;
  font-size: 9px;
}
.tag input {
  width: 11px;
  height: 11px;
}
.actions {
  display: flex;
  gap: 6px;
  margin-top: 4px;
}
.btn {
  background: #1a2230;
  border: 1px solid #3a4354;
  color: #e8b339;
  border-radius: 3px;
  padding: 4px 12px;
  font-size: 10px;
  cursor: pointer;
}
.btn.ghost {
  color: #9aa7bb;
}
.msg {
  color: #c8b483;
  font-size: 10px;
}
.stats {
  border-top: 1px solid #1d2431;
  margin-top: 10px;
  padding-top: 8px;
}
.attr {
  margin-top: 6px;
  border: 1px solid #1d2431;
  border-radius: 3px;
  overflow: hidden;
}
.attr-row {
  display: grid;
  grid-template-columns: 1.3fr 0.7fr 1fr 1fr 0.7fr;
  gap: 4px;
  padding: 3px 6px;
  font: 9px Consolas, monospace;
  color: #9aa7bb;
}
.attr-row.head {
  background: #0b1017;
  color: #5f6d84;
}
.attr-row + .attr-row {
  border-top: 1px solid #151c27;
}
.delta {
  margin-top: 5px;
  font-size: 9px;
  color: #76839a;
}
.plan-box {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 5px;
  font: 9px Consolas, monospace;
  color: #9aa7bb;
}
.plan-box .dim {
  color: #5f6d84;
}
.row2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}
.preview {
  display: flex;
  flex-wrap: wrap;
  gap: 9px;
  padding: 5px 6px;
  background: #0b1017;
  border: 1px solid #1d2431;
  border-radius: 3px;
  font: 9px Consolas, monospace;
  color: #76839a;
}
.preview b {
  font-weight: 500;
}
.mis {
  margin-top: 6px;
}
.mis-row {
  display: flex;
  gap: 8px;
  padding: 2px 0;
  font: 9px Consolas, monospace;
  color: #9aa7bb;
}
.mis-row .avg {
  margin-left: auto;
  color: #5f6d84;
}
.stat {
  color: #e8b339;
  font-size: 9px;
}
.pos {
  color: #ef5350;
}
.neg {
  color: #26a69a;
}
</style>
