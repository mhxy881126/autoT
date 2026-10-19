<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useDeskStore } from "../stores/desk";
import { pct } from "../lib/format";

const store = useDeskStore();
const input = ref("");
const msg = ref("");
const busy = ref(false);
let listTimer: ReturnType<typeof setInterval> | null = null;
let scanTimer: ReturnType<typeof setInterval> | null = null;

async function add(): Promise<void> {
  const symbol = input.value.trim().toLowerCase();
  if (!symbol) return;
  busy.value = true;
  msg.value = "";
  try {
    await store.addSymbol(symbol);
    input.value = "";
    await refreshScan(false);
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

async function select(symbol: string): Promise<void> {
  if (store.config?.symbol === symbol || busy.value) return;
  busy.value = true;
  msg.value = "";
  try {
    await store.switchSymbol(symbol);
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

async function remove(symbol: string): Promise<void> {
  busy.value = true;
  msg.value = "";
  try {
    await store.removeSymbol(symbol);
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}

function scanOf(symbol: string) {
  return store.scan.find((s) => s.symbol === symbol) ?? null;
}

let prevAccepted = -1;

function totalAccepted(): number {
  return store.scan.reduce((sum, s) => sum + (s.paused ? 0 : s.accepted), 0);
}

function beep(): void {
  try {
    const Ctx =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctx) return;
    const ctx = new Ctx();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.value = 880;
    gain.gain.value = 0.06;
    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.start();
    osc.stop(ctx.currentTime + 0.16);
    setTimeout(() => void ctx.close(), 400);
  } catch {
    // 音频不可用时静默，不影响扫描
  }
}

async function refreshScan(sound: boolean): Promise<void> {
  await store.scanWatchlist();
  const total = totalAccepted();
  if (sound && prevAccepted >= 0 && total > prevAccepted) beep();
  prevAccepted = total;
}

onMounted(() => {
  void store.loadWatchlist().then(() => refreshScan(false));
  listTimer = setInterval(() => {
    if (!busy.value) void store.loadWatchlist();
  }, 30000);
  scanTimer = setInterval(() => {
    if (!busy.value) void refreshScan(true);
  }, 60000);
});

onBeforeUnmount(() => {
  if (listTimer) clearInterval(listTimer);
  if (scanTimer) clearInterval(scanTimer);
});
</script>

<template>
  <aside class="panel watch">
    <div class="head">
      <span>自选</span>
      <span class="count">{{ store.watchlist.length }}</span>
    </div>

    <div class="add">
      <input
        v-model="input"
        placeholder="sh600519"
        :disabled="busy"
        @keyup.enter="add"
      />
      <button :disabled="busy" @click="add">+</button>
    </div>
    <div v-if="msg" class="msg">{{ msg }}</div>

    <div class="list">
      <div
        v-for="item in store.watchlist"
        :key="item.symbol"
        class="item"
        :class="{ active: item.symbol === store.config?.symbol }"
        @click="select(item.symbol)"
      >
        <div class="row">
          <span class="code">{{ item.symbol.slice(2) }}</span>
          <span class="name">{{ item.name || item.symbol }}</span>
        </div>
        <div class="row">
          <span class="px">{{ item.price > 0 ? item.price.toFixed(2) : "--" }}</span>
          <span class="chg" :class="item.changePct >= 0 ? 'up' : 'down'">
            {{ item.price > 0 ? pct(item.changePct) : "" }}
          </span>
        </div>
        <div class="meta">
          {{ item.baseQty }} 股
          <span v-if="item.paused" class="paused">暂停</span>
        </div>
        <div v-if="scanOf(item.symbol)" class="scan">
          <span class="regime">{{ scanOf(item.symbol)?.regime }}</span>
          <span v-if="scanOf(item.symbol)?.paused" class="paused">不扫描</span>
          <span v-else-if="(scanOf(item.symbol)?.accepted ?? 0) > 0" class="hot">
            {{ scanOf(item.symbol)?.accepted }} 个信号
          </span>
          <span v-else class="idle">无信号 · {{ scanOf(item.symbol)?.candidates }} 候选</span>
          <span v-if="scanOf(item.symbol)?.stale" class="stale">重试中</span>
          <span v-else-if="scanOf(item.symbol)?.cached" class="cached">缓存</span>
          <span
            v-if="(scanOf(item.symbol)?.accepted ?? 0) > 0"
            class="detail"
            :title="`${scanOf(item.symbol)?.signalSide} ${scanOf(item.symbol)?.signalT}`"
          >
            {{ scanOf(item.symbol)?.signalSide }} {{ scanOf(item.symbol)?.signalT }}
          </span>
        </div>
        <button class="del" title="移出自选" @click.stop="remove(item.symbol)">×</button>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.watch {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 8px;
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  font-weight: 600;
  color: #b9c3d4;
  padding: 2px 4px 8px;
  border-bottom: 1px solid #1d2431;
}
.count {
  font: 11px Consolas, monospace;
  color: #4d5a70;
}
.add {
  display: flex;
  gap: 4px;
  margin: 8px 2px;
}
.add input {
  flex: 1;
  min-width: 0;
  background: #0b1017;
  border: 1px solid #28313f;
  border-radius: 3px;
  color: #dbe2ee;
  padding: 4px 6px;
  font: 11px Consolas, monospace;
}
.add button {
  width: 26px;
  background: #1a2230;
  border: 1px solid #28313f;
  color: #e8b339;
  border-radius: 3px;
  cursor: pointer;
  font-size: 15px;
  line-height: 1;
}
.msg {
  font-size: 10px;
  color: #ff8a87;
  padding: 0 4px 6px;
  line-height: 1.4;
}
.list {
  flex: 1;
  overflow: auto;
  min-height: 0;
}
.item {
  position: relative;
  padding: 7px 8px 6px;
  border: 1px solid transparent;
  border-radius: 5px;
  margin-bottom: 3px;
  cursor: pointer;
}
.item:hover {
  background: #151c27;
}
.item.active {
  background: rgba(232, 179, 57, 0.07);
  border-color: rgba(232, 179, 57, 0.4);
}
.row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 6px;
}
.code {
  font: 12px Consolas, monospace;
  color: #dbe2ee;
}
.name {
  font-size: 10px;
  color: #76839a;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 78px;
}
.px {
  font: 13px Consolas, monospace;
  color: #dbe2ee;
}
.chg {
  font: 10px Consolas, monospace;
}
.up {
  color: #ef5350;
}
.down {
  color: #26a69a;
}
.meta {
  font-size: 9px;
  color: #4d5a70;
  margin-top: 2px;
}
.paused {
  color: #e8b339;
  margin-left: 4px;
}
.scan {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 3px;
  font-size: 9px;
}
.regime {
  color: #76839a;
}
.hot {
  color: #ef5350;
  font-weight: 600;
}
.idle {
  color: #4d5a70;
}
.detail {
  color: #e8b339;
  font-family: Consolas, monospace;
}
.cached {
  color: #4d5a70;
  border: 1px solid #28313f;
  border-radius: 2px;
  padding: 0 3px;
  font-size: 8px;
}
.stale {
  color: #e8b339;
  border: 1px solid rgba(232, 179, 57, 0.4);
  border-radius: 2px;
  padding: 0 3px;
  font-size: 8px;
}
.del {
  position: absolute;
  right: 2px;
  top: 2px;
  background: none;
  border: 0;
  color: #4d5a70;
  cursor: pointer;
  font-size: 12px;
  display: none;
}
.item:hover .del {
  display: block;
}
</style>
