<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import type { PlanAudit, PlanLevel } from "../lib/types";
import { useDeskStore } from "../stores/desk";

const store = useDeskStore();
const plan = computed(() => store.payload.plan);
const audit = computed(() => store.planAudit);
const showAudit = ref(false);
const show = ref(false);
const saving = ref(false);
const msg = ref("");

const form = reactive({
  bias: "双向",
  maxTrades: 4,
  riskBudgetPct: 1,
  note: "",
  reason: "",
  levels: [] as PlanLevel[]
});

function open(): void {
  const p = plan.value;
  form.bias = p?.bias ?? "双向";
  form.maxTrades = p?.maxTrades ?? store.config?.maxSignals ?? 4;
  form.riskBudgetPct = p?.riskBudgetPct ?? (store.config ? store.config.dailyLossLimit * 100 : 1);
  form.note = p?.note ?? "";
  form.reason = "";
  form.levels = p ? p.levels.map((l) => ({ ...l })) : [];
  msg.value = "";
  show.value = true;
}

function addLevel(): void {
  if (form.levels.length >= 10) return;
  form.levels.push({ label: "", price: store.last.price });
}

function removeLevel(i: number): void {
  form.levels.splice(i, 1);
}

function auditLine(a: PlanAudit): string {
  if (!a.before) return "初次制定";
  return `${a.before.bias} ${a.before.maxTrades}次 → ${a.after.bias} ${a.after.maxTrades}次`;
}

function shortTime(value: string): string {
  return value.slice(5, 16).replace("T", " ");
}

onMounted(() => {
  void store.loadPlanAudit(
    store.payload.meta.planDate ?? store.payload.meta.date,
    store.payload.meta.symbolId
  );
});

watch(
  () => [store.payload.meta.planDate, store.payload.meta.symbolId],
  () => {
    void store.loadPlanAudit(
      store.payload.meta.planDate ?? store.payload.meta.date,
      store.payload.meta.symbolId
    );
  }
);

async function save(): Promise<void> {
  saving.value = true;
  msg.value = "";
  try {
    await store.savePlan({
      day: store.payload.meta.planDate ?? store.payload.meta.date,
      symbol: store.payload.meta.symbolId,
      bias: form.bias,
      maxTrades: Number(form.maxTrades),
      riskBudgetPct: Number(form.riskBudgetPct),
      levels: form.levels.filter((l) => l.label.trim() && l.price > 0),
      note: form.note || null,
      reason: form.reason || null
    });
    show.value = false;
  } catch (e) {
    msg.value = e instanceof Error ? e.message : String(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="panel card plan">
    <div class="ph">
      <span class="t">盘前计划</span>
      <span v-if="plan" class="badge" :class="plan.locked ? 'locked' : 'open'">
        {{ plan.locked ? "已锁定" : "可编辑" }}
      </span>
      <span v-else class="badge open">未制定</span>
    </div>

    <template v-if="plan">
      <div class="row">
        <span class="bias">{{ plan.bias }}</span>
        <span>最多 {{ plan.maxTrades }} 次</span>
        <span>风险 {{ plan.riskBudgetPct.toFixed(2) }}%</span>
      </div>
      <div v-if="plan.levels.length" class="levels">
        <div v-for="l in plan.levels" :key="l.label" class="level">
          <span>{{ l.label }}</span>
          <b>{{ l.price.toFixed(2) }}</b>
        </div>
      </div>
      <div v-if="plan.note" class="note">{{ plan.note }}</div>
      <div v-if="plan.lockedReason" class="reason">锁定理由：{{ plan.lockedReason }}</div>
    </template>
    <div v-else class="empty">开盘前写下今天的方向、关键价位和风险预算。</div>

    <button class="edit" @click="open">{{ plan ? "修改计划" : "制定计划" }}</button>
    <button v-if="audit.length" class="audit-toggle" @click="showAudit = !showAudit">
      修改记录 {{ audit.length }} {{ showAudit ? "▴" : "▾" }}
    </button>

    <div v-if="showAudit && audit.length" class="audit-list">
      <div v-for="a in audit.slice(0, 6)" :key="a.id" class="audit-item">
        <div class="audit-head">
          <span>{{ shortTime(a.changedAt) }}</span>
          <span class="audit-reason">{{ a.reason ?? "未填理由" }}</span>
        </div>
        <div class="audit-change">{{ auditLine(a) }}</div>
      </div>
    </div>

    <div v-if="show" class="mask" @click.self="show = false">
      <div class="modal">
        <div class="mh">
          <span>盘前计划 · {{ store.payload.meta.symbolId }}</span>
          <button class="x" @click="show = false">×</button>
        </div>

        <div class="grid">
          <label>方向偏好
            <select v-model="form.bias">
              <option>正T</option>
              <option>倒T</option>
              <option>双向</option>
              <option>不做</option>
            </select>
          </label>
          <label>最大 T 次数
            <input v-model.number="form.maxTrades" type="number" min="0" max="20" />
          </label>
          <label>风险预算（%）
            <input v-model.number="form.riskBudgetPct" type="number" step="0.1" min="0" max="10" />
          </label>
        </div>

        <div class="lbl">关键价位</div>
        <div v-for="(l, i) in form.levels" :key="i" class="level-edit">
          <input v-model="l.label" placeholder="VWAP / 支撑 / 压力" />
          <input v-model.number="l.price" type="number" step="0.01" />
          <button class="rm" @click="removeLevel(i)">×</button>
        </div>
        <button class="add" @click="addLevel">+ 加一个价位</button>

        <label>备注
          <textarea v-model="form.note" rows="3" placeholder="今天只做什么、不做什么" />
        </label>

        <label v-if="plan?.locked" class="reason-input">
          修改理由（必填）
          <input v-model="form.reason" placeholder="为什么开盘后改计划" />
        </label>

        <div v-if="msg" class="err">{{ msg }}</div>
        <div class="foot">
          <span class="dim">{{ plan?.locked ? "计划已锁定，修改会留痕" : "开盘前可自由修改" }}</span>
          <button class="primary" :disabled="saving" @click="save">
            {{ saving ? "保存中…" : "保存计划" }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.plan {
  padding: 8px 10px;
  margin-bottom: 6px;
}
.ph {
  display: flex;
  align-items: center;
  gap: 6px;
}
.t {
  font-size: 11px;
  color: #dbe2ee;
  font-weight: 600;
}
.badge {
  margin-left: auto;
  font: 9px Consolas, monospace;
  border-radius: 3px;
  padding: 1px 5px;
}
.badge.locked {
  color: #e8b339;
  border: 1px solid rgba(232, 179, 57, 0.4);
}
.badge.open {
  color: #67d6a3;
  border: 1px solid rgba(103, 214, 163, 0.35);
}
.row {
  display: flex;
  gap: 10px;
  margin-top: 5px;
  font: 9px Consolas, monospace;
  color: #9aa7bb;
}
.bias {
  color: #e8b339;
}
.levels {
  margin-top: 4px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.level {
  display: flex;
  justify-content: space-between;
  font: 9px Consolas, monospace;
  color: #76839a;
}
.level b {
  color: #c3cddd;
  font-weight: 500;
}
.note {
  margin-top: 4px;
  font-size: 9px;
  color: #6b7688;
  line-height: 1.5;
}
.reason {
  margin-top: 3px;
  font-size: 9px;
  color: #c8a24a;
}
.empty {
  margin-top: 5px;
  font-size: 9px;
  color: #5f6d84;
  line-height: 1.5;
}
.edit {
  margin-top: 6px;
  width: 100%;
  background: #1a2230;
  border: 1px solid #3a4354;
  color: #e8b339;
  border-radius: 3px;
  padding: 4px 0;
  font-size: 10px;
  cursor: pointer;
}
.audit-toggle {
  margin-top: 4px;
  width: 100%;
  background: none;
  border: 0;
  color: #5f6d84;
  font-size: 9px;
  cursor: pointer;
  text-align: left;
  padding: 2px 0;
}
.audit-list {
  margin-top: 3px;
  border-top: 1px solid #1d2431;
  padding-top: 4px;
}
.audit-item {
  padding: 3px 0;
}
.audit-head {
  display: flex;
  justify-content: space-between;
  font: 9px Consolas, monospace;
  color: #76839a;
}
.audit-reason {
  color: #c8a24a;
  max-width: 60%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.audit-change {
  font: 9px Consolas, monospace;
  color: #9aa7bb;
  margin-top: 1px;
}
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.58);
  z-index: 1200;
  display: grid;
  place-items: center;
}
.modal {
  width: 560px;
  background: #111620;
  border: 1px solid #28313f;
  border-radius: 8px;
  padding: 14px;
  color: #dbe2ee;
  font-size: 11px;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.55);
}
.mh {
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
  grid-template-columns: 1fr 1fr 1fr;
  gap: 8px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 10px;
  color: #76839a;
}
input,
select,
textarea {
  background: #0b1017;
  border: 1px solid #28313f;
  border-radius: 3px;
  color: #dbe2ee;
  padding: 5px 7px;
  font: 11px Consolas, monospace;
}
.lbl {
  margin-top: 8px;
  font-size: 10px;
  color: #76839a;
}
.level-edit {
  display: grid;
  grid-template-columns: 1fr 110px 26px;
  gap: 5px;
  margin-top: 4px;
}
.rm {
  background: none;
  border: 1px solid #3a2a2a;
  color: #ef5350;
  border-radius: 3px;
  cursor: pointer;
}
.add {
  margin-top: 5px;
  background: none;
  border: 1px dashed #3a4354;
  color: #9aa7bb;
  border-radius: 3px;
  padding: 3px 8px;
  font-size: 10px;
  cursor: pointer;
}
.reason-input {
  margin-top: 8px;
  color: #e8b339;
}
textarea {
  resize: vertical;
}
.err {
  margin-top: 6px;
  color: #ff8a87;
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
</style>
