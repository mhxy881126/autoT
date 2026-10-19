<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { PrecheckResult, Signal } from "../lib/types";
import { useDeskStore } from "../stores/desk";

const props = defineProps<{ signal: Signal; symbol: string }>();
const emit = defineEmits<{ (e: "close"): void; (e: "confirmed"): void }>();
const store = useDeskStore();

const price = ref(props.signal.entry || props.signal.price);
const qty = ref(props.signal.qty);
const result = ref<PrecheckResult | null>(null);
const checking = ref(false);
const submitting = ref(false);
const msg = ref("");
const side = computed<"BUY" | "SELL">(() => (props.signal.side === "正T" ? "BUY" : "SELL"));
const sideLabel = computed(() => (side.value === "BUY" ? "买入" : "卖出"));

const ticketText = computed(
  () =>
    `${props.symbol} ${sideLabel.value} ${qty.value} 股 @ ${Number(price.value).toFixed(2)}\n` +
    `止损 ${props.signal.stop.toFixed(2)} · 目标 ${props.signal.target.toFixed(2)}\n` +
    `信号 ${props.signal.id} ${props.signal.t} ${props.signal.side}\n` +
    `计划数量 ${props.signal.qty} 股`
);

async function check(): Promise<void> {
  checking.value = true;
  msg.value = "";
  try {
    result.value = await store.precheckOrder({
      symbol: props.symbol,
      day: store.payload.meta.date,
      side: side.value,
      price: Number(price.value),
      qty: Number(qty.value),
      signalId: props.signal.id,
      signalT: props.signal.t,
      signalPrice: props.signal.price
    });
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    checking.value = false;
  }
}

async function copy(): Promise<void> {
  const text = ticketText.value;
  try {
    await navigator.clipboard.writeText(text);
    msg.value = "票据已复制";
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    ta.remove();
    msg.value = "票据已复制";
  }
}

async function confirm(): Promise<void> {
  if (!result.value?.ok || submitting.value) return;
  submitting.value = true;
  msg.value = "";
  try {
    const sideValue = side.value;
    await store.submitFill({
      clientId: crypto.randomUUID(),
      symbol: props.symbol,
      day: store.payload.meta.date,
      t: store.payload.last.t,
      side: sideValue,
      price: Number(price.value),
      qty: Number(qty.value),
      signalId: props.signal.id,
      note: "票据确认"
    });
    msg.value = "已写入成交，账本与风控已刷新";
    emit("confirmed");
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    submitting.value = false;
  }
}

onMounted(check);
</script>

<template>
  <div class="mask" @click.self="emit('close')">
    <div class="ticket">
      <div class="head">
        <span>订单票据 · {{ symbol }}</span>
        <button class="x" @click="emit('close')">×</button>
      </div>

      <div class="grid">
        <label>方向
          <input :value="sideLabel" disabled />
        </label>
        <label>限价
          <input v-model.number="price" type="number" step="0.01" />
        </label>
        <label>数量
          <input v-model.number="qty" type="number" step="100" />
        </label>
        <label>信号
          <input :value="`${signal.id} ${signal.t}`" disabled />
        </label>
      </div>

      <div class="plan">
        <span>止损 {{ signal.stop.toFixed(2) }}</span>
        <span>目标 {{ signal.target.toFixed(2) }}</span>
        <span>预计净利 {{ (signal.net * 100).toFixed(2) }}%</span>
        <span>信号价 {{ signal.price.toFixed(2) }}</span>
      </div>

      <div class="actions">
        <button class="btn" :disabled="checking" @click="check">
          {{ checking ? "预检中…" : "重新预检" }}
        </button>
        <button class="btn" @click="copy">复制票据</button>
      </div>

      <div v-if="result" class="checks">
        <div v-for="c in result.checks" :key="c.name" class="check" :class="{ bad: !c.ok }">
          <span class="mark">{{ c.ok ? "✓" : "✕" }}</span>
          <span class="name">{{ c.name }}</span>
          <span class="detail">{{ c.detail }}</span>
        </div>
      </div>

      <div v-if="result && !result.ok" class="blockers">
        <b>禁止确认：</b>{{ result.blockers.join("；") }}
      </div>
      <div v-if="result?.warnings.length" class="warnings">
        提醒：{{ result.warnings.join("；") }}
      </div>
      <div v-if="msg" class="msg">{{ msg }}</div>

      <div class="foot">
        <span class="dim">确认后写入本地账本，不会发往券商</span>
        <button class="primary" :disabled="!result?.ok || submitting" @click="confirm">
          {{ submitting ? "写入中…" : "确认成交" }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.58);
  z-index: 1200;
  display: grid;
  place-items: center;
}
.ticket {
  width: 560px;
  background: #111620;
  border: 1px solid #28313f;
  border-radius: 8px;
  padding: 14px;
  color: #dbe2ee;
  font-size: 11px;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.55);
}
.head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 12px;
  margin-bottom: 10px;
}
.x {
  background: none;
  border: 0;
  color: #6b7688;
  font-size: 16px;
  cursor: pointer;
}
.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 10px;
  color: #76839a;
}
input {
  background: #0b1017;
  border: 1px solid #28313f;
  border-radius: 3px;
  color: #dbe2ee;
  padding: 5px 7px;
  font: 11px Consolas, monospace;
}
input:disabled {
  color: #6b7688;
}
.plan {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 8px;
  padding: 6px 8px;
  background: #0b1017;
  border: 1px solid #1d2431;
  border-radius: 3px;
  color: #9aa7bb;
  font: 10px Consolas, monospace;
}
.actions {
  display: flex;
  gap: 6px;
  margin-top: 8px;
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
.checks {
  margin-top: 8px;
  border: 1px solid #1d2431;
  border-radius: 3px;
  overflow: hidden;
}
.check {
  display: grid;
  grid-template-columns: 16px 90px 1fr;
  gap: 4px;
  padding: 3px 6px;
  font: 9px Consolas, monospace;
  color: #9aa7bb;
}
.check + .check {
  border-top: 1px solid #151c27;
}
.check.bad {
  background: rgba(239, 83, 80, 0.08);
  color: #ffb3b1;
}
.mark {
  color: #67d6a3;
}
.check.bad .mark {
  color: #ef5350;
}
.name {
  color: #c3cddd;
}
.blockers {
  margin-top: 8px;
  color: #ff8a87;
  font-size: 10px;
  line-height: 1.6;
}
.warnings {
  margin-top: 5px;
  color: #e8b339;
  font-size: 10px;
}
.msg {
  margin-top: 6px;
  color: #c8b483;
  font-size: 10px;
}
.foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 10px;
}
.dim {
  color: #5f6d84;
  font-size: 9px;
}
.primary {
  background: #e8b339;
  border: 0;
  color: #171207;
  border-radius: 3px;
  padding: 6px 16px;
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
}
.primary:disabled {
  background: #3a3630;
  color: #777;
  cursor: default;
}
</style>
