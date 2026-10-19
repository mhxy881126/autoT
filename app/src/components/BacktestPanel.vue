<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import type { BacktestResult, Scorecard } from "../lib/types";
import { money } from "../lib/format";
import { isTauri } from "../lib/ipc";
import { useDeskStore } from "../stores/desk";

const store = useDeskStore();
const symbol = computed(() => store.config?.symbol ?? "");
const day = ref("");
const rangeText = ref("");
const single = ref<BacktestResult | null>(null);
const card = ref<Scorecard | null>(null);
const importing = ref(false);
const busyDay = ref(false);
const busyRange = ref(false);
const importMsg = ref("");
const err = ref("");

function fmtDay(d: string): string {
  return d.length === 8 ? `${d.slice(0, 4)}-${d.slice(4, 6)}-${d.slice(6)}` : d;
}

async function refreshDays(): Promise<void> {
  if (!isTauri() || !symbol.value) return;
  try {
    const days = await store.loadMinuteDays(symbol.value);
    if (days.length) {
      if (!day.value) day.value = fmtDay(days[days.length - 1]);
      if (!rangeText.value) rangeText.value = days.map(fmtDay).join("\n");
    }
  } catch (e) {
    err.value = e instanceof Error ? e.message : String(e);
  }
}

onMounted(() => void refreshDays());

watch(symbol, () => {
  single.value = null;
  card.value = null;
  day.value = "";
  rangeText.value = "";
  importMsg.value = "";
  void refreshDays();
});

async function onFile(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file || !isTauri()) return;
  if (file.size > 20 * 1024 * 1024) {
    importMsg.value = "文件超过 20MB，已拒绝";
    input.value = "";
    return;
  }
  importing.value = true;
  importMsg.value = "";
  err.value = "";
  try {
    const buf = await file.arrayBuffer();
    const bytes = Array.from(new Uint8Array(buf));
    const res = await store.importMinutesCsv(symbol.value, bytes);
    importMsg.value = `导入 ${res.days} 天 / ${res.rows} 行，跳过 ${res.skipped} 行`;
    await refreshDays();
  } catch (e) {
    err.value = e instanceof Error ? e.message : String(e);
  } finally {
    importing.value = false;
    input.value = "";
  }
}

async function runDay(): Promise<void> {
  if (!symbol.value || !day.value || busyDay.value) return;
  busyDay.value = true;
  err.value = "";
  try {
    single.value = await store.backtestDay(symbol.value, day.value);
  } catch (e) {
    err.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyDay.value = false;
  }
}

async function runRange(): Promise<void> {
  if (!symbol.value || busyRange.value) return;
  const days = rangeText.value
    .split(/[\s,，;；]+/)
    .map((s) => s.trim())
    .filter(Boolean);
  if (!days.length) {
    err.value = "先填日期";
    return;
  }
  busyRange.value = true;
  err.value = "";
  try {
    card.value = await store.backtestRange(symbol.value, days);
  } catch (e) {
    err.value = e instanceof Error ? e.message : String(e);
  } finally {
    busyRange.value = false;
  }
}
</script>

<template>
  <div class="bt">
    <div class="head">回测 · <b>{{ symbol || "—" }}</b></div>

    <div class="block">
      <div class="lbl">历史分钟 CSV</div>
      <input
        class="file"
        type="file"
        accept=".csv,.txt"
        :disabled="!isTauri() || importing || !symbol"
        @change="onFile"
      />
      <div class="note">列：day,time,price,cum_vol,cum_amt（累计量/额）</div>
      <div v-if="importMsg" class="msg">{{ importMsg }}</div>
      <div v-if="store.minuteDays.length" class="chips">
        <span class="muted">已缓存 {{ store.minuteDays.length }} 天</span>
        <button
          v-for="d in store.minuteDays.slice(-6)"
          :key="d"
          class="chip"
          @click="day = fmtDay(d)"
        >
          {{ fmtDay(d) }}
        </button>
      </div>
    </div>

    <div class="block">
      <div class="lbl">单日</div>
      <div class="row">
        <input v-model="day" placeholder="2026-10-19" />
        <button class="btn" :disabled="busyDay" @click="runDay">
          {{ busyDay ? "跑…" : "跑单日" }}
        </button>
      </div>

      <template v-if="single">
        <div class="grid">
          <div><span>净收益</span><b :class="single.summary.net >= 0 ? 'pos' : 'neg'">{{ money(single.summary.net) }}</b></div>
          <div><span>成交</span><b>{{ single.summary.trades }} 笔</b></div>
          <div><span>胜率</span><b>{{ (single.summary.winRate * 100).toFixed(0) }}%</b></div>
          <div><span>费用</span><b>{{ money(single.summary.fees) }}</b></div>
          <div><span>滑点</span><b>{{ money(single.summary.slippage) }}</b></div>
          <div><span>最大回撤</span><b>{{ money(single.summary.maxDrawdown) }}</b></div>
        </div>
        <table v-if="single.trades.length" class="tbl">
          <thead>
            <tr><th>时间</th><th>方向</th><th>成交</th><th>净</th></tr>
          </thead>
          <tbody>
            <tr v-for="t in single.trades" :key="t.signalId + t.entryT">
              <td>{{ t.entryT }}→{{ t.exitT }}</td>
              <td>{{ t.side }}</td>
              <td>{{ t.entryPrice.toFixed(2) }}→{{ t.exitPrice.toFixed(2) }}</td>
              <td :class="t.net >= 0 ? 'pos' : 'neg'">{{ money(t.net) }}</td>
            </tr>
          </tbody>
        </table>
        <div v-else class="empty">无成交；跳过 {{ single.skipped.length }} 条</div>
        <div v-if="single.skipped.length" class="skip">{{ single.skipped[0] }}</div>
      </template>
    </div>

    <div class="block">
      <div class="lbl">区间</div>
      <textarea v-model="rangeText" rows="4" placeholder="2026-10-19&#10;2026-10-20" />
      <button class="btn full" :disabled="busyRange" @click="runRange">
        {{ busyRange ? "跑…" : "跑区间" }}
      </button>

      <template v-if="card">
        <div class="grid">
          <div><span>天数</span><b>{{ card.days }}</b></div>
          <div><span>成交</span><b>{{ card.trades }} 笔</b></div>
          <div><span>净收益</span><b :class="card.net >= 0 ? 'pos' : 'neg'">{{ money(card.net) }}</b></div>
          <div><span>胜率</span><b>{{ (card.winRate * 100).toFixed(0) }}%</b></div>
          <div><span>最大回撤</span><b>{{ money(card.maxDrawdown) }}</b></div>
          <div><span>无交易日</span><b>{{ card.noTradeDays }}</b></div>
        </div>
        <div class="note">
          与“不做”基线差 {{ money(card.edgeVsNoTrade) }}；日均 {{ money(card.avgNetPerDay) }}。
        </div>
        <table class="tbl">
          <thead><tr><th>日期</th><th>笔数</th><th>净</th></tr></thead>
          <tbody>
            <tr v-for="d in card.daily" :key="d.day">
              <td>{{ fmtDay(d.day) }}</td>
              <td>{{ d.trades }}</td>
              <td :class="d.net >= 0 ? 'pos' : 'neg'">{{ d.net === 0 ? "—" : money(d.net) }}</td>
            </tr>
          </tbody>
        </table>
      </template>
    </div>

    <div v-if="err" class="err">{{ err }}</div>
  </div>
</template>

<style scoped>
.bt {
  font-size: 11px;
  color: #c3cddd;
}
.head {
  font-size: 12px;
  color: #e8eef8;
  margin-bottom: 6px;
}
.block {
  border-top: 1px solid #1d2431;
  padding: 8px 0;
}
.lbl {
  font-size: 10px;
  color: #76839a;
  margin-bottom: 5px;
}
.file {
  width: 100%;
  font-size: 10px;
  color: #9aa7bb;
}
.file::file-selector-button {
  background: #1a2230;
  border: 1px solid #28313f;
  color: #9aa7bb;
  border-radius: 3px;
  padding: 3px 8px;
  margin-right: 6px;
  cursor: pointer;
}
.note {
  font-size: 9px;
  color: #4d5a70;
  margin-top: 4px;
  line-height: 1.5;
}
.msg {
  font-size: 10px;
  color: #c8b483;
  margin-top: 4px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 6px;
  align-items: center;
}
.muted {
  color: #4d5a70;
  font-size: 9px;
}
.chip {
  background: #151c27;
  border: 1px solid #28313f;
  color: #9aa7bb;
  border-radius: 3px;
  padding: 1px 5px;
  font-size: 9px;
  cursor: pointer;
}
.row {
  display: flex;
  gap: 6px;
}
input,
textarea {
  background: #0b1017;
  border: 1px solid #28313f;
  border-radius: 3px;
  color: #dbe2ee;
  padding: 4px 6px;
  font: 11px Consolas, monospace;
}
input {
  flex: 1;
  min-width: 0;
}
textarea {
  width: 100%;
  resize: vertical;
  margin-bottom: 6px;
}
.btn {
  background: #1a2230;
  border: 1px solid #3a4354;
  color: #e8b339;
  border-radius: 3px;
  padding: 4px 10px;
  font-size: 10px;
  cursor: pointer;
}
.btn.full {
  width: 100%;
}
.btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.grid {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 5px;
  margin: 8px 0;
}
.grid div {
  background: #0e131c;
  border: 1px solid #1d2431;
  border-radius: 3px;
  padding: 4px 6px;
}
.grid span {
  display: block;
  font-size: 9px;
  color: #4d5a70;
}
.grid b {
  font: 11px Consolas, monospace;
  color: #dbe2ee;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font: 9px Consolas, monospace;
}
.tbl th {
  text-align: left;
  color: #4d5a70;
  font-weight: normal;
  border-bottom: 1px solid #1d2431;
  padding: 3px 2px;
}
.tbl td {
  padding: 3px 2px;
  border-bottom: 1px solid #141a24;
  color: #9aa7bb;
}
.pos {
  color: #ef5350;
}
.neg {
  color: #26a69a;
}
.empty {
  color: #4d5a70;
  padding: 8px 0;
}
.skip {
  color: #6b7688;
  font-size: 9px;
  margin-top: 4px;
}
.err {
  color: #ff8a87;
  font-size: 10px;
  margin-top: 6px;
}
</style>
