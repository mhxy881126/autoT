<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watchEffect } from "vue";
import * as echarts from "echarts";
import type { DeskPayload } from "../lib/types";
import { fmt, money } from "../lib/format";
import { isTauri } from "../lib/ipc";
import { useDeskStore } from "../stores/desk";
import BacktestPanel from "./BacktestPanel.vue";
import JournalPanel from "./JournalPanel.vue";

const props = defineProps<{ payload: DeskPayload }>();

const store = useDeskStore();
const canWrite = isTauri();
const submitting = ref(false);
const msg = ref("");
const form = reactive({ side: "BUY" as "BUY" | "SELL", price: 0, qty: 100, t: "" });

function newId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

watchEffect(() => {
  if (!form.price) form.price = Number(props.payload.last.price.toFixed(2));
  if (!form.t) form.t = props.payload.last.t;
});

async function record(): Promise<void> {
  if (!canWrite || submitting.value) return;
  submitting.value = true;
  msg.value = "";
  try {
    const fill = await store.submitFill({
      clientId: newId(),
      symbol: store.config?.symbol ?? props.payload.meta.symbolId,
      day: props.payload.meta.date,
      t: form.t,
      side: form.side,
      price: Number(form.price),
      qty: Number(form.qty),
      fee: 0
    });
    msg.value = `已记录 #${fill.id} ${fill.side === "BUY" ? "买" : "卖"} ${fill.qty} @ ${fill.price.toFixed(2)}`;
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    submitting.value = false;
  }
}

async function undoFill(id: number): Promise<void> {
  if (!canWrite) return;
  msg.value = "";
  try {
    await store.voidFill(id, props.payload.meta.date);
    msg.value = `已冲正 #${id}`;
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  }
}

const importing = ref(false);
const importMsg = ref("");
const refreshing = ref(false);
const refreshMsg = ref("");

async function refreshRisk(): Promise<void> {
  if (!canWrite || refreshing.value) return;
  refreshing.value = true;
  refreshMsg.value = "";
  try {
    await store.refreshSignals();
    refreshMsg.value = "信号与风控已刷新";
  } catch (e) {
    refreshMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    refreshing.value = false;
  }
}

async function onStatementFile(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement;
  const file = input.files?.[0];
  if (!file || !canWrite) return;
  if (file.size > 2 * 1024 * 1024) {
    importMsg.value = "文件超过 2MB，已拒绝";
    input.value = "";
    return;
  }
  importing.value = true;
  importMsg.value = "";
  try {
    const buf = await file.arrayBuffer();
    const bytes = Array.from(new Uint8Array(buf));
    const res = await store.importStatement(props.payload.meta.date, file.name, bytes);
    importMsg.value =
      res.message + (res.errors.length ? `；首条：${res.errors[0]}` : "");
  } catch (e) {
    importMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    importing.value = false;
    input.value = "";
  }
}

type Tab = "signals" | "test" | "book" | "bt" | "journal";
const hash = typeof location !== "undefined" ? location.hash.replace("#", "") : "";
const tab = ref<Tab>(
  hash === "test" || hash === "book" || hash === "bt" || hash === "journal"
    ? hash
    : "signals"
);
const sensEl = ref<HTMLDivElement | null>(null);
let sensChart: ReturnType<typeof echarts.init> | null = null;

const strict = computed(() => props.payload.scenarios[0]);
const loose = computed(() => props.payload.scenarios[1]);
const trades = computed(() => strict.value?.trades ?? props.payload.trades);

function buildSens(): any {
  const rows = props.payload.sensitivity;
  return {
    animation: false,
    backgroundColor: "transparent",
    grid: { left: 44, right: 34, top: 18, bottom: 24 },
    xAxis: {
      type: "category",
      data: rows.map((r) => r.threshold.toFixed(2) + "%"),
      axisLine: { lineStyle: { color: "#2a3342" } },
      axisTick: { show: false },
      axisLabel: { color: "#6b7688", fontSize: 9 }
    },
    yAxis: [
      { type: "value", splitLine: { lineStyle: { color: "#161d29" } }, axisLabel: { color: "#6b7688", fontSize: 9 } },
      { type: "value", max: 4, splitLine: { show: false }, axisLabel: { color: "#4d5a70", fontSize: 9 } }
    ],
    tooltip: {
      trigger: "axis",
      backgroundColor: "rgba(12,16,24,.95)",
      borderColor: "#2a3342",
      textStyle: { color: "#dbe2ee", fontSize: 11 }
    },
    series: [
      {
        name: "净盈亏",
        type: "bar",
        barWidth: "46%",
        data: rows.map((r) => ({
          value: r.net,
          itemStyle: {
            color:
              r.net < 0
                ? "rgba(38,166,154,.55)"
                : r.net > 0
                  ? "rgba(239,83,80,.55)"
                  : "rgba(120,132,152,.25)",
            borderRadius: [2, 2, 0, 0]
          }
        }))
      },
      {
        name: "成交笔数",
        type: "line",
        yAxisIndex: 1,
        data: rows.map((r) => r.filled),
        symbolSize: 5,
        lineStyle: { color: "#e8b339", width: 1.4 },
        itemStyle: { color: "#e8b339" }
      }
    ]
  };
}

function initSens() {
  if (!sensEl.value) return;
  if (!sensChart) {
    sensChart = echarts.init(sensEl.value, undefined, { renderer: "canvas" });
  }
  sensChart.setOption(buildSens(), { notMerge: true });
  sensChart.resize();
}

async function show(t: Tab) {
  tab.value = t;
  if (t === "test") {
    await nextTick();
    initSens();
  }
}

onBeforeUnmount(() => {
  sensChart?.dispose();
  sensChart = null;
});

onMounted(async () => {
  if (canWrite) await store.loadFills(props.payload.meta.date);
  if (tab.value === "test") {
    await nextTick();
    initSens();
  }
});
</script>

<template>
  <div class="panel tabs">
    <div class="tabbar">
      <button class="tab" :class="{ active: tab === 'signals' }" @click="show('signals')">信号流水</button>
      <button class="tab" :class="{ active: tab === 'test' }" @click="show('test')">门槛压测</button>
      <button class="tab" :class="{ active: tab === 'book' }" @click="show('book')">账务 / 回填</button>
      <button class="tab" :class="{ active: tab === 'bt' }" @click="show('bt')">回测</button>
      <button class="tab" :class="{ active: tab === 'journal' }" @click="show('journal')">日志</button>
    </div>

    <div class="tabbody">
      <template v-if="tab === 'signals'">
        <template v-if="payload.signals.length">
          <div v-for="s in payload.signals" :key="s.id" class="srow">
            <span class="tm">{{ s.t }}</span>
            <span class="tag" :class="s.side === '正T' ? 'buy' : 'sell'">{{ s.side }}</span>
            <span class="px">{{ s.price.toFixed(2) }}</span>
            <span class="tag" :class="s.accepted ? 'pass' : 'veto'">{{ s.accepted ? "通过" : "否决" }}</span>
            <span class="rs" :title="(s.veto.length ? s.veto : s.why).join('；')">
              {{ s.veto[0] ?? s.why[0] }}
            </span>
          </div>
          <div class="empty" style="padding: 14px 4px; text-align: left; color: #4d5a70">
            共 {{ payload.signals.length }} 个候选，{{ payload.stats.accepted }} 个通过。
            <template v-if="payload.stats.accepted === 0">
              全部被硬门槛拦下（价差 / regime / 成本）。这不是故障，是今日的结论。
            </template>
          </div>
        </template>
        <div v-else class="empty">
          <b>暂无候选</b><br />行情链路已就绪，信号引擎将在 P2 接入。
        </div>
      </template>

      <template v-else-if="tab === 'test'">
        <template v-if="strict && loose">
          <div class="scen">
            <div class="box active">
              <div class="lbl">{{ strict.label }}</div>
              <div class="num" :class="strict.stats.totalNet >= 0 ? 'pos' : 'neg'">
                {{ money(strict.stats.totalNet) }}
              </div>
              <div class="sub">{{ strict.stats.filled }} 笔 · 胜率 {{ (strict.stats.winRate * 100).toFixed(0) }}%</div>
            </div>
            <div class="box">
              <div class="lbl">{{ loose.label }}</div>
              <div class="num" :class="loose.stats.totalNet >= 0 ? 'pos' : 'neg'">
                {{ money(loose.stats.totalNet) }}
              </div>
              <div class="sub">{{ loose.stats.filled }} 笔 · 胜率 {{ (loose.stats.winRate * 100).toFixed(0) }}%</div>
            </div>
          </div>

          <div ref="sensEl" style="height: 160px; margin: 2px -4px 8px"></div>

          <div class="empty" style="padding: 8px 4px; text-align: left; color: #6b7688; line-height: 1.7">
            门槛从 0.30% 降到 0.25%，交易从 0 笔变成 {{ loose.stats.filled }} 笔，止损离场。<br />
            亏损 ¥{{ fmt(Math.abs(loose.stats.totalNet)) }} 中，
            <b style="color: #c8b483">¥{{ fmt(loose.stats.totalCost) }} 是手续费与滑点</b>——比价格亏损还多。
          </div>

          <table class="cost">
            <tbody>
              <tr v-for="r in payload.sensitivity" :key="r.threshold">
                <td>门槛 {{ r.threshold.toFixed(2) }}%</td>
                <td>{{ r.filled }} 笔</td>
                <td :class="r.net < 0 ? 'neg' : ''">{{ r.net === 0 ? "—" : money(r.net) }}</td>
              </tr>
            </tbody>
          </table>
        </template>
        <div v-else class="empty">
          <b>没有压测样本</b><br />实时模式下至少需要一个交易日的候选记录。
        </div>
      </template>

      <template v-else-if="tab === 'bt'">
        <BacktestPanel />
      </template>

      <template v-else-if="tab === 'journal'">
        <JournalPanel />
      </template>

      <template v-else>
        <div class="lbl" style="font-size: 11px; color: #76839a; margin-bottom: 6px">
          今日成交{{ canWrite ? "（回填）" : "（模拟）" }}
        </div>
        <table v-if="canWrite && store.fills.length" class="cost">
          <tbody>
            <tr v-for="f in store.fills" :key="f.id" :class="{ voided: f.voided }">
              <td>{{ f.t }} {{ f.side === "BUY" ? "买" : "卖" }}</td>
              <td>{{ f.qty }} @ {{ f.price.toFixed(2) }}</td>
              <td>费 ¥{{ f.fee.toFixed(2) }}</td>
              <td>
                <button class="mini-btn" :disabled="f.voided" @click="undoFill(f.id)">
                  {{ f.voided ? "已冲正" : "冲正" }}
                </button>
              </td>
            </tr>
          </tbody>
        </table>
        <table v-else-if="trades.length" class="cost">
          <tbody>
            <tr v-for="t in trades" :key="t.id">
              <td>{{ t.entryT }} {{ t.side }}</td>
              <td>{{ t.entry.toFixed(2) }}→{{ t.exit.toFixed(2) }}</td>
              <td :class="t.netPnl >= 0 ? 'pos' : 'neg'">{{ money(t.netPnl) }}</td>
            </tr>
          </tbody>
        </table>
        <div v-else class="empty" style="padding: 8px 4px">0 笔成交 · 严格门槛下无可执行信号</div>

        <div v-if="canWrite" class="refresh-row">
          <button class="mini-btn" :disabled="refreshing" @click="refreshRisk">
            {{ refreshing ? "刷新中…" : "刷新信号 / 风控" }}
          </button>
          <span v-if="refreshMsg" class="refresh-msg">{{ refreshMsg }}</span>
        </div>

        <div v-if="canWrite && store.ledger" style="margin-top: 8px">
          <div class="kv">
            <div>持仓 <span>{{ store.ledger?.position.baseQty ?? 0 }} 股</span></div>
            <div>可卖 <span>{{ store.ledger?.position.available ?? 0 }} 股</span></div>
            <div>
              已实现
              <span :class="(store.ledger?.realizedNet ?? 0) >= 0 ? 'pos' : 'neg'">
                {{ money(store.ledger?.realizedNet ?? 0) }}
              </span>
            </div>
            <div>未闭环 <span>{{ store.ledger?.unresolved ?? 0 }} 笔</span></div>
          </div>
          <table v-if="store.ledger?.pairs.length" class="cost">
            <tbody>
              <tr v-for="p in store.ledger?.pairs ?? []" :key="p.id">
                <td>{{ p.side }} {{ p.qty }} 股</td>
                <td :class="(p.net ?? 0) >= 0 ? 'pos' : 'neg'">
                  {{ p.status === "CLOSED" ? money(p.net ?? 0) : "未闭环" }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <div class="divider"></div>
        <div class="lbl" style="font-size: 11px; color: #76839a; margin-bottom: 2px">往返成本模型（10 万元）</div>
        <table class="cost">
          <tbody>
            <tr><td>佣金（万 2.5，双边）</td><td>0.050%</td></tr>
            <tr><td>印花税（卖出）</td><td>0.050%</td></tr>
            <tr><td>过户费（双边）</td><td>0.002%</td></tr>
            <tr><td>滑点（双边 2bp）</td><td>0.040%</td></tr>
            <tr class="total"><td>合计</td><td>0.142%</td></tr>
          </tbody>
        </table>

        <div class="divider"></div>
        <div class="lbl" style="font-size: 11px; color: #76839a">交割单导入（CSV / TXT）</div>
        <input
          class="file"
          type="file"
          accept=".csv,.txt"
          :disabled="!canWrite || importing"
          @change="onStatementFile"
        />
        <div v-if="importMsg" class="note" style="color: #c8b483">{{ importMsg }}</div>

        <div class="divider"></div>
        <div class="lbl" style="font-size: 11px; color: #76839a">手工回填（无券商通道）</div>
        <div class="form">
          <div>
            <label>方向</label>
            <select v-model="form.side" :disabled="!canWrite">
              <option value="BUY">正T · 先买后卖</option>
              <option value="SELL">倒T · 先卖后买</option>
            </select>
          </div>
          <div>
            <label>成交价</label>
            <input v-model.number="form.price" type="number" step="0.01" :disabled="!canWrite" />
          </div>
          <div>
            <label>数量（股）</label>
            <input v-model.number="form.qty" type="number" step="100" :disabled="!canWrite" />
          </div>
          <div>
            <label>成交时间</label>
            <input v-model="form.t" :disabled="!canWrite" />
          </div>
          <button class="btn full" :disabled="!canWrite || submitting" @click="record">
            {{ canWrite ? (submitting ? "记录中…" : "记录成交") : "记录成交（仅桌面端）" }}
          </button>
        </div>
        <div v-if="msg" class="note" style="color: #c8b483">{{ msg }}</div>
        <div class="note">
          没有券商 API，系统无法自动获知成交。回填是账务准确性的唯一来源；回填缺失会让"信号绩效"冒充"实盘绩效"。
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.voided {
  opacity: 0.38;
  text-decoration: line-through;
}
.mini-btn {
  background: #1a2230;
  border: 1px solid #28313f;
  color: #9aa7bb;
  border-radius: 3px;
  padding: 1px 5px;
  font-size: 10px;
  cursor: pointer;
}
.mini-btn:disabled {
  cursor: default;
  color: #4d5a70;
}
.refresh-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
}
.refresh-msg {
  font-size: 9px;
  color: #c8b483;
}
.file {
  width: 100%;
  font-size: 11px;
  color: #9aa7bb;
  margin-top: 4px;
}
.file::file-selector-button {
  background: #1a2230;
  border: 1px solid #28313f;
  color: #9aa7bb;
  border-radius: 3px;
  padding: 3px 8px;
  margin-right: 8px;
  cursor: pointer;
}
</style>
