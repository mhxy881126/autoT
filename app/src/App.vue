<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useDeskStore } from "./stores/desk";
import { fmt, money, pct } from "./lib/format";
import IntradayChart from "./components/IntradayChart.vue";
import KLineChart from "./components/KLineChart.vue";
import DecisionCard from "./components/DecisionCard.vue";
import Panels from "./components/Panels.vue";
import WatchList from "./components/WatchList.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import GuidePanel from "./components/GuidePanel.vue";

const store = useDeskStore();
const showSettings = ref(false);
const showGuide = ref(false);
const payload = computed(() => store.payload);
const meta = computed(() => payload.value.meta);
const account = computed(() => payload.value.account);
const close = computed(() => store.last.price);
const chg = computed(() => close.value / meta.value.prevClose - 1);
const posVal = computed(() => close.value * account.value.baseQty);
const floatPnl = computed(() => (close.value - account.value.baseCost) * account.value.baseQty);
const hotScans = computed(() => store.scan.filter((s) => s.accepted > 0 && !s.paused));

async function focusSymbol(symbol: string): Promise<void> {
  try {
    await store.switchSymbol(symbol);
  } catch {
    // 切换失败时保持当前票，错误由 store.bootError 呈现
  }
}

const REGIME_DOT: Record<string, string> = {
  上行: "#ef5350",
  下行: "#26a69a",
  震荡: "#8c99ad",
  极端: "#e8b339"
};

onMounted(() => {
  void store.load();
});
</script>

<template>
  <div class="app">
    <header class="topbar">
      <div class="brand"><span class="logo">T</span> 做T决策台 <span class="ver">TAURI</span></div>
      <div class="crumb">
        <b>{{ meta.code }} {{ meta.symbol }}</b> · {{ meta.date }} ·
        {{ store.mode === "live" ? "实时" : "回放" }}
      </div>
      <div class="spacer"></div>
      <button class="ghost" @click="showGuide = true">怎么用</button>
      <button class="ghost" @click="showSettings = true">设置</button>
      <div class="quote">
        <span class="px">{{ close.toFixed(2) }}</span>
        <span class="chg" :class="chg >= 0 ? 'up' : 'down'">{{ pct(chg) }}</span>
      </div>
      <span v-if="store.config?.paused" class="pill paused">已暂停</span>
      <span class="pill" :class="store.mode === 'live' ? 'live' : 'reject'">
        {{ store.mode === "live" ? "LIVE" : "DEMO" }}
      </span>
      <button class="halt" title="原型未接入交易通道">全局急停</button>
    </header>

    <div v-if="hotScans.length" class="signal-bar">
      <span class="sig-label">信号</span>
      <button
        v-for="s in hotScans"
        :key="s.symbol"
        class="sig-item"
        @click="focusSymbol(s.symbol)"
      >
        <b>{{ s.name || s.symbol }}</b> · {{ s.accepted }} 个 · {{ s.signalSide }}
        {{ s.signalT }}
      </button>
    </div>

    <section class="strip">
      <div class="cell">
        <label>总资产</label><b>¥{{ fmt(posVal + account.cash) }}</b><small>含持仓市值</small>
      </div>
      <div class="cell">
        <label>持仓市值</label><b>¥{{ fmt(posVal) }}</b><small>{{ account.baseQty }} 股 · 可卖 {{ account.available }}</small>
      </div>
      <div class="cell">
        <label>可用资金</label><b>¥{{ fmt(account.cash) }}</b><small>手工回填</small>
      </div>
      <div class="cell">
        <label>底仓成本</label><b>{{ account.baseCost.toFixed(2) }}</b><small>近 20 日均价</small>
      </div>
      <div class="cell">
        <label>浮动盈亏</label>
        <b :class="floatPnl >= 0 ? 'pos' : 'neg'">{{ money(floatPnl) }}</b>
        <small>{{ pct(floatPnl / (account.baseCost * account.baseQty)) }}</small>
      </div>
      <div class="cell">
        <label>今日做T</label>
        <b :class="payload.stats.totalNet >= 0 ? 'pos' : 'neg'">{{ money(payload.stats.totalNet) }}</b>
        <small>{{ payload.stats.filled }} / {{ store.config?.maxSignals ?? 4 }} 次</small>
      </div>
      <div class="cell">
        <label>风控 / 状态</label>
        <b class="ok">{{ store.mode === "live" ? "实时链路" : "回放模式" }}</b>
        <small>
          <span class="regime">
            <span class="dot" :style="{ background: REGIME_DOT[store.last.regime] ?? '#8c99ad' }"></span>
            {{ store.last.regime }}
          </span>
        </small>
      </div>
    </section>

    <main class="main">
      <WatchList />
      <div class="left">
        <section class="panel">
          <div class="panel-head">
            <span class="t">分时 · 1 分钟</span>
            <span class="sub">背离 / 回归 / 否决标记 · {{ store.mode === "live" ? "Rust 实时" : "回放" }}</span>
            <div class="legend">
              <span><i style="background:#dbe2ee"></i>价格</span>
              <span><i style="background:#e8b339"></i>均价</span>
              <span><i style="background:#4fc3f7"></i>VWAP20</span>
              <span><i style="background:#6b7688"></i>昨收</span>
            </div>
          </div>
          <IntradayChart
            :minutes="store.minutes"
            :signals="payload.signals"
            :prev-close="meta.prevClose"
          />
        </section>

        <section class="panel">
          <div class="panel-head">
            <span class="t">日 K · 前复权</span>
            <span class="sub">环境判断，不用于触发</span>
            <div class="legend">
              <span><i style="background:#e8b339"></i>MA5</span>
              <span><i style="background:#4fc3f7"></i>MA10</span>
              <span><i style="background:#9b8cff"></i>MA20</span>
            </div>
          </div>
          <KLineChart :kline="payload.kline" />
        </section>
      </div>

      <aside class="side">
        <DecisionCard :payload="payload" :last="store.last" />
        <Panels :payload="payload" />
      </aside>
    </main>

    <footer class="statusbar">
      <span>模式：<span class="ok">{{ store.mode === "live" ? "半自动 · Rust 实时" : "半自动 · 浏览器回放" }}</span></span>
      <span>今日 T：<span>{{ payload.stats.filled }}</span> / {{ store.config?.maxSignals ?? 4 }}</span>
      <span>连亏：0</span>
      <span>报撤速率：0 笔/秒（监管线 300）</span>
      <span>数据：腾讯 L1 · 3 秒快照</span>
      <span class="spacer"></span>
      <span v-if="store.bootError" class="neg">IPC: {{ store.bootError }}</span>
      <span>生成于 {{ meta.generatedAt }}</span>
    </footer>

    <SettingsPanel v-if="showSettings" @close="showSettings = false" />
    <GuidePanel v-if="showGuide" @close="showGuide = false" />
  </div>
</template>

<style scoped>
.pill.live {
  background: rgba(103, 214, 163, 0.12);
  color: #67d6a3;
  border: 1px solid rgba(103, 214, 163, 0.3);
}
.pill.paused {
  background: rgba(232, 179, 57, 0.12);
  color: #e8b339;
  border: 1px solid rgba(232, 179, 57, 0.3);
}
.ghost {
  background: #141a24;
  border: 1px solid #28313f;
  color: #9aa7bb;
  border-radius: 5px;
  padding: 5px 12px;
  font-size: 11px;
  cursor: pointer;
  margin-right: 6px;
}
.ghost:hover {
  color: #dbe2ee;
  border-color: #3a4354;
}
.main {
  grid-template-columns: 190px minmax(0, 1fr) 420px;
}
.signal-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: rgba(239, 83, 80, 0.08);
  border-bottom: 1px solid rgba(239, 83, 80, 0.3);
}
.sig-label {
  font-size: 10px;
  letter-spacing: 0.12em;
  color: #ef5350;
}
.sig-item {
  background: #1a1214;
  border: 1px solid rgba(239, 83, 80, 0.4);
  color: #ffb3b1;
  border-radius: 4px;
  padding: 4px 9px;
  font-size: 11px;
  cursor: pointer;
}
.sig-item b {
  color: #ffffff;
}
</style>
