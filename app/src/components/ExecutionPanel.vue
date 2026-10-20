<script setup lang="ts">
import { computed, onMounted, watch } from "vue";
import { money } from "../lib/format";
import { useDeskStore } from "../stores/desk";

const store = useDeskStore();
const data = computed(() => store.executionQuality);

async function load(): Promise<void> {
  await store.loadExecution(store.payload.meta.date, store.payload.meta.symbolId);
}

onMounted(load);
watch(
  () => [store.payload.meta.date, store.payload.meta.symbolId],
  () => void load()
);
</script>

<template>
  <div class="exec">
    <div class="head">
      执行质量 · <b>{{ store.payload.meta.symbolId }}</b>
      <span class="dim">{{ store.payload.meta.date }}</span>
    </div>

    <div v-if="!data" class="empty">加载中…</div>
    <template v-else>
      <div class="cards">
        <div class="card">
          <div class="lbl">信号滑点</div>
          <div class="val" :class="data.slippage.avgBps > 0 ? 'neg' : 'pos'">
            {{ data.slippage.avgBps > 0 ? "+" : "" }}{{ data.slippage.avgBps.toFixed(1) }} bp
          </div>
          <div class="sub">{{ data.slippage.count }} 笔 · 最差 {{ data.slippage.worstBps.toFixed(1) }} bp</div>
        </div>
        <div class="card">
          <div class="lbl">平均 MFE</div>
          <div class="val pos">{{ data.avgMfePct.toFixed(2) }}%</div>
          <div class="sub">持仓期间最大有利</div>
        </div>
        <div class="card">
          <div class="lbl">平均 MAE</div>
          <div class="val neg">{{ data.avgMaePct.toFixed(2) }}%</div>
          <div class="sub">持仓期间最大不利</div>
        </div>
      </div>

      <div class="lbl">逐笔 MAE / MFE</div>
      <div v-if="!data.pairs.length" class="empty">今日没有已平仓 T</div>
      <table v-else class="tbl">
        <thead>
          <tr>
            <th>方向</th>
            <th>数量</th>
            <th>开→平</th>
            <th>净盈亏</th>
            <th>MFE</th>
            <th>MAE</th>
            <th>捕获</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in data.pairs" :key="p.pairId">
            <td>{{ p.side }}</td>
            <td>{{ p.qty }}</td>
            <td>{{ p.openPrice.toFixed(2) }}→{{ p.closePrice.toFixed(2) }}</td>
            <td :class="p.net >= 0 ? 'pos' : 'neg'">{{ money(p.net) }}</td>
            <td class="pos">{{ p.mfePct.toFixed(2) }}%</td>
            <td class="neg">{{ p.maePct.toFixed(2) }}%</td>
            <td>{{ p.capturePct.toFixed(0) }}%</td>
          </tr>
        </tbody>
      </table>

      <div class="lbl">成交滑点（信号价 → 成交价）</div>
      <div v-if="!data.slippage.rows.length" class="empty">没有带信号 ID 的成交</div>
      <table v-else class="tbl">
        <thead>
          <tr>
            <th>信号</th>
            <th>方向</th>
            <th>信号价</th>
            <th>成交价</th>
            <th>滑点</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in data.slippage.rows" :key="r.fillId">
            <td>{{ r.signalId }}</td>
            <td>{{ r.side === "BUY" ? "买" : "卖" }}</td>
            <td>{{ r.signalPrice.toFixed(2) }}</td>
            <td>{{ r.fillPrice.toFixed(2) }}</td>
            <td :class="r.bps > 0 ? 'neg' : 'pos'">{{ r.bps.toFixed(1) }} bp</td>
          </tr>
        </tbody>
      </table>

      <div class="note">
        滑点按“买入成交价高于信号价 / 卖出成交价低于信号价”为正滑点（不利）。MAE/MFE 用 1 分钟收盘价近似。
      </div>
    </template>
  </div>
</template>

<style scoped>
.exec {
  font-size: 11px;
  color: #c3cddd;
}
.head {
  font-size: 12px;
  margin-bottom: 6px;
}
.dim {
  color: #5f6d84;
  font-size: 9px;
  margin-left: 6px;
}
.cards {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 6px;
  margin-bottom: 8px;
}
.card {
  background: #0e131c;
  border: 1px solid #1d2431;
  border-radius: 4px;
  padding: 6px;
}
.lbl {
  font-size: 9px;
  color: #76839a;
  margin: 6px 0 3px;
}
.card .lbl {
  margin: 0;
}
.val {
  font: 14px Consolas, monospace;
  margin-top: 2px;
}
.sub {
  font-size: 8px;
  color: #5f6d84;
  margin-top: 2px;
}
.tbl {
  width: 100%;
  border-collapse: collapse;
  font: 9px Consolas, monospace;
}
.tbl th {
  text-align: left;
  color: #5f6d84;
  font-weight: 400;
  padding: 3px 2px;
  border-bottom: 1px solid #1d2431;
}
.tbl td {
  padding: 3px 2px;
  border-bottom: 1px solid #151c27;
}
.empty {
  color: #4d5a70;
  padding: 8px 2px;
  font-size: 9px;
}
.note {
  margin-top: 8px;
  color: #5f6d84;
  font-size: 9px;
  line-height: 1.6;
}
.pos {
  color: #ef5350;
}
.neg {
  color: #26a69a;
}
</style>
