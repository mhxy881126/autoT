<script setup lang="ts">
import { onMounted } from "vue";
import type { ActionAudit } from "../lib/types";
import { useDeskStore } from "../stores/desk";

const emit = defineEmits<{ (e: "close"): void }>();
const store = useDeskStore();

const LABELS: Record<string, string> = {
  zEntry: "z",
  targetLo: "目标",
  targetHi: "目标上限",
  stopPct: "止损",
  maxSignals: "次数",
  paused: "暂停",
  dailyLossLimit: "日亏",
  liquidityCap: "流动性",
  maxConsecutiveLosses: "连亏",
  baseQty: "底仓",
  maxTRatio: "T仓",
  cash: "现金",
  cooldownMin: "冷却"
};

const PERCENT = new Set(["targetLo", "targetHi", "stopPct", "dailyLossLimit", "liquidityCap", "maxTRatio"]);

function fmtValue(key: string, value: unknown): string {
  if (typeof value === "boolean") return value ? "开" : "关";
  if (typeof value === "number") {
    if (PERCENT.has(key)) return `${(value * 100).toFixed(2)}%`;
    return Number.isInteger(value) ? String(value) : value.toFixed(2);
  }
  return String(value ?? "—");
}

function diff(a: ActionAudit): string {
  if (a.action === "emergency") {
    return `${a.before ? "开启" : "关闭"} → ${a.after ? "开启" : "关闭"}`;
  }
  if (
    typeof a.before !== "object" ||
    a.before === null ||
    typeof a.after !== "object" ||
    a.after === null
  ) {
    return "—";
  }
  const before = a.before as Record<string, unknown>;
  const after = a.after as Record<string, unknown>;
  const parts: string[] = [];
  for (const key of Object.keys(LABELS)) {
    if (before[key] !== after[key]) {
      parts.push(`${LABELS[key]} ${fmtValue(key, before[key])}→${fmtValue(key, after[key])}`);
    }
  }
  return parts.length ? parts.join("；") : "无关键字段变化";
}

function shortTime(value: string): string {
  return value.slice(5, 16).replace("T", " ");
}

function label(action: string): string {
  return action === "emergency" ? "急停" : action === "config" ? "配置" : action;
}

onMounted(() => {
  void store.loadActionAudit();
});
</script>

<template>
  <div class="mask" @click.self="emit('close')">
    <div class="modal">
      <div class="head">
        <span>操作审计</span>
        <span class="count">{{ store.actionAudit.length }} 条</span>
        <button class="x" @click="emit('close')">×</button>
      </div>

      <div v-if="!store.actionAudit.length" class="empty">暂无急停或配置修改记录</div>
      <div v-else class="list">
        <div v-for="a in store.actionAudit" :key="a.id" class="row">
          <div class="top">
            <span class="kind" :class="a.action">{{ label(a.action) }}</span>
            <span class="target">{{ a.target }}</span>
            <span class="time">{{ shortTime(a.changedAt) }}</span>
          </div>
          <div class="change">{{ diff(a) }}</div>
          <div class="reason">{{ a.reason ?? "未填理由" }}</div>
        </div>
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
.modal {
  width: 620px;
  max-height: 78vh;
  overflow: auto;
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
  align-items: center;
  gap: 8px;
  font-size: 12px;
  margin-bottom: 10px;
}
.count {
  color: #5f6d84;
  font-size: 9px;
}
.x {
  margin-left: auto;
  background: none;
  border: 0;
  color: #6b7688;
  font-size: 16px;
  cursor: pointer;
}
.empty {
  color: #5f6d84;
  padding: 16px 4px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.row {
  background: #0b1017;
  border: 1px solid #1d2431;
  border-radius: 4px;
  padding: 6px 8px;
}
.top {
  display: flex;
  align-items: center;
  gap: 8px;
}
.kind {
  font: 9px Consolas, monospace;
  border-radius: 3px;
  padding: 1px 5px;
}
.kind.emergency {
  color: #ff8a87;
  border: 1px solid rgba(239, 83, 80, 0.4);
}
.kind.config {
  color: #e8b339;
  border: 1px solid rgba(232, 179, 57, 0.4);
}
.target {
  color: #9aa7bb;
  font: 9px Consolas, monospace;
}
.time {
  margin-left: auto;
  color: #5f6d84;
  font: 9px Consolas, monospace;
}
.change {
  margin-top: 4px;
  color: #c3cddd;
  font: 9px Consolas, monospace;
  line-height: 1.6;
}
.reason {
  margin-top: 2px;
  color: #76839a;
  font-size: 9px;
}
</style>
