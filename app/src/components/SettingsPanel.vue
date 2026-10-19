<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import type { StrategyConfig } from "../lib/types";
import { call, isTauri } from "../lib/ipc";
import { useDeskStore } from "../stores/desk";

const emit = defineEmits<{ (e: "close"): void }>();
const store = useDeskStore();
const saving = ref(false);
const err = ref("");
const testing = ref(false);
const testMsg = ref("");

const form = reactive({
  symbol: "sh600519",
  baseQty: 1000,
  baseCost: 0,
  cash: 800000,
  maxTRatioPct: 50,
  zEntry: 1.2,
  targetLoPct: 0.45,
  targetHiPct: 0.9,
  stopPctPct: 0.35,
  maxSignals: 4,
  cooldownMin: 15,
  sessionStart: "09:45",
  sessionEnd: "14:50",
  paused: false
});

onMounted(() => {
  const c = store.config;
  if (!c) return;
  form.symbol = c.symbol;
  form.baseQty = c.baseQty;
  form.baseCost = c.baseCost;
  form.cash = c.cash;
  form.maxTRatioPct = Math.round(c.maxTRatio * 1000) / 10;
  form.zEntry = c.zEntry;
  form.targetLoPct = Math.round(c.targetLo * 10000) / 100;
  form.targetHiPct = Math.round(c.targetHi * 10000) / 100;
  form.stopPctPct = Math.round(c.stopPct * 10000) / 100;
  form.maxSignals = c.maxSignals;
  form.cooldownMin = c.cooldownMin;
  form.sessionStart = c.sessionStart;
  form.sessionEnd = c.sessionEnd;
  form.paused = c.paused;
});

async function save(): Promise<void> {  if (!isTauri()) {
    err.value = "浏览器模式不能保存配置，请在桌面端操作";
    return;
  }
  saving.value = true;
  err.value = "";
  const next: StrategyConfig = {
    symbol: form.symbol.trim(),
    baseQty: Number(form.baseQty),
    baseCost: Number(form.baseCost),
    cash: Number(form.cash),
    maxTRatio: Number(form.maxTRatioPct) / 100,
    zEntry: Number(form.zEntry),
    targetLo: Number(form.targetLoPct) / 100,
    targetHi: Number(form.targetHiPct) / 100,
    stopPct: Number(form.stopPctPct) / 100,
    maxSignals: Number(form.maxSignals),
    cooldownMin: Number(form.cooldownMin),
    sessionStart: form.sessionStart,
    sessionEnd: form.sessionEnd,
    paused: form.paused
  };
  try {
    await store.saveConfig(next);
    emit("close");
  } catch (e) {
    err.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}
async function testNotify(): Promise<void> {
  if (!isTauri()) {
    testMsg.value = "仅桌面端可测试系统通知";
    return;
  }
  testing.value = true;
  testMsg.value = "";
  try {
    await call<void>("test_notification");
    testMsg.value = "已发送，请检查系统通知";
  } catch (e) {
    testMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    testing.value = false;
  }
}
</script>

<template>
  <div class="mask" @click.self="emit('close')">
    <div class="modal">
      <div class="head">
        <b>策略与账户配置</b>
        <button class="x" @click="emit('close')">×</button>
      </div>

      <div class="grid">
        <label class="full">标的代码
          <input v-model="form.symbol" placeholder="sh600519 / sz000858" />
        </label>
        <label>底仓数量（股）
          <input v-model.number="form.baseQty" type="number" step="100" />
        </label>
        <label>底仓成本（0 = 近 20 日均价）
          <input v-model.number="form.baseCost" type="number" step="0.01" />
        </label>
        <label>可用资金（元）
          <input v-model.number="form.cash" type="number" step="1000" />
        </label>
        <label>单次 T 仓上限（%）
          <input v-model.number="form.maxTRatioPct" type="number" step="5" />
        </label>
        <label>z 入场阈值（σ）
          <input v-model.number="form.zEntry" type="number" step="0.1" />
        </label>
        <label>每日最多次数
          <input v-model.number="form.maxSignals" type="number" step="1" />
        </label>
        <label>目标价差下限（%）
          <input v-model.number="form.targetLoPct" type="number" step="0.05" />
        </label>
        <label>目标价差上限（%）
          <input v-model.number="form.targetHiPct" type="number" step="0.05" />
        </label>
        <label>止损（%）
          <input v-model.number="form.stopPctPct" type="number" step="0.05" />
        </label>
        <label>冷却（分钟）
          <input v-model.number="form.cooldownMin" type="number" step="1" />
        </label>
        <label>开始时间
          <input v-model="form.sessionStart" placeholder="09:45" />
        </label>
        <label>结束时间
          <input v-model="form.sessionEnd" placeholder="14:50" />
        </label>
        <label class="full check">
          <input v-model="form.paused" type="checkbox" />
          暂停策略（仍显示行情，不生成新信号）
        </label>
      </div>

      <div class="hint">
        保存后会写入本地 SQLite，并立即重新拉取该标的的分时、日 K 与信号。
        更换标的时，请确认底仓数量与代码匹配。
      </div>
      <div v-if="err" class="err">{{ err }}</div>

      <div class="actions">
        <button class="ghost" :disabled="testing" @click="testNotify">
          {{ testing ? "发送中…" : "测试系统通知" }}
        </button>
        <button class="ghost" @click="emit('close')">取消</button>
        <button class="primary" :disabled="saving" @click="save">
          {{ saving ? "保存中…" : "保存并重载" }}
        </button>
      </div>
      <div v-if="testMsg" class="hint">{{ testMsg }}</div>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  z-index: 1000;
  display: grid;
  place-items: center;
}
.modal {
  width: 720px;
  max-height: 88vh;
  overflow: auto;
  background: #111620;
  border: 1px solid #28313f;
  border-radius: 10px;
  padding: 18px;
  color: #dbe2ee;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 14px;
  font-size: 15px;
}
.x {
  background: none;
  border: 0;
  color: #76839a;
  font-size: 20px;
  cursor: pointer;
}
.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px 14px;
}
.grid label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 11px;
  color: #76839a;
}
.grid .full {
  grid-column: 1 / 3;
}
.grid .check {
  flex-direction: row;
  align-items: center;
  gap: 8px;
  color: #c3cddd;
  font-size: 12px;
}
input[type="text"],
input[type="number"],
input:not([type]) {
  background: #0b1017;
  border: 1px solid #28313f;
  border-radius: 4px;
  color: #dbe2ee;
  padding: 6px 8px;
  font: 12px Consolas, monospace;
}
input[type="checkbox"] {
  width: 14px;
  height: 14px;
}
.hint {
  margin-top: 12px;
  font-size: 11px;
  color: #4d5a70;
  line-height: 1.6;
}
.err {
  margin-top: 8px;
  font-size: 12px;
  color: #ff8a87;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}
.ghost,
.primary {
  border-radius: 5px;
  padding: 7px 16px;
  font-size: 12px;
  cursor: pointer;
}
.ghost {
  background: #1a2230;
  border: 1px solid #28313f;
  color: #9aa7bb;
}
.primary {
  background: #e8b339;
  border: 1px solid #e8b339;
  color: #1a1206;
  font-weight: 600;
}
.primary:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
