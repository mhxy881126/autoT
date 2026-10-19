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
  reason: "",
  emotion: "冷静",
  mistakes: [] as string[],
  note: ""
});

const EMOTIONS = ["冷静", "犹豫", "追涨", "恐慌", "报复", "其他"];
const MISTAKES = ["追高", "抄底过早", "未按信号", "超仓", "未止损", "情绪化", "计划外"];

function samePair(a: JournalEntry, p: TPair): boolean {
  return a.openFillId === p.openFillId && (a.closeFillId ?? 0) === (p.closeFillId ?? 0);
}

function entryFor(p: TPair): JournalEntry | undefined {
  return store.journal.find((e) => samePair(e, p));
}

function selectPair(p: TPair): void {
  selectedId.value = p.id;
  const e = entryFor(p);
  existing.value = e ?? null;
  form.planned = e?.planned ?? true;
  form.reason = e?.reason ?? "";
  form.emotion = e?.emotion ?? "冷静";
  form.mistakes = [...(e?.mistakes ?? [])];
  form.note = e?.note ?? "";
  msg.value = "";
}

const selectedPair = computed(() => pairs.value.find((p) => p.id === selectedId.value) ?? null);

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
      <div v-if="store.journalStats.mistakes.length" class="tags">
        <span v-for="m in store.journalStats.mistakes" :key="m.name" class="stat">
          {{ m.name }} ×{{ m.count }}
        </span>
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
