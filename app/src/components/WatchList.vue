<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useDeskStore } from "../stores/desk";
import { pct } from "../lib/format";

const store = useDeskStore();
const input = ref("");
const msg = ref("");
const busy = ref(false);
let timer: ReturnType<typeof setInterval> | null = null;

async function add(): Promise<void> {
  const symbol = input.value.trim().toLowerCase();
  if (!symbol) return;
  busy.value = true;
  msg.value = "";
  try {
    await store.addSymbol(symbol);
    input.value = "";
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

onMounted(() => {
  void store.loadWatchlist();
  timer = setInterval(() => {
    if (!busy.value) void store.loadWatchlist();
  }, 30000);
});

onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
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
