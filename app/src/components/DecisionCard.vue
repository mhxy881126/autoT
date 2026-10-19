<script setup lang="ts">
import { computed } from "vue";
import type { DeskPayload } from "../lib/types";
import { fmt, money } from "../lib/format";

const props = defineProps<{
  payload: DeskPayload;
  last: { t: string; regime: string; z: number; price: number; vwap: number };
}>();

const signals = computed(() => props.payload.signals);
const accepted = computed(() => signals.value.filter((s) => s.accepted));
const lastSig = computed(() => signals.value[signals.value.length - 1]);
const loose = computed(() => props.payload.scenarios.find((s) => s.key === "loose"));

const chips = computed(() =>
  Object.entries(props.payload.stats.vetoReasons ?? {}).map(([k, v]) => ({ k, v }))
);
</script>

<template>
  <div class="panel card">
    <div class="card-head">
      <span class="t">决策卡片</span>
      <span class="pill" :class="accepted.length ? 'pass' : 'reject'">
        {{ signals.length === 0 ? "等待信号" : accepted.length ? "通过" : "拒绝交易" }}
      </span>
    </div>

    <template v-if="signals.length === 0">
      <div class="verdict">等待信号</div>
      <div class="verdict-sub">
        Rust 侧尚未产出候选（行情链路已就绪，信号引擎在 P2 接入）
      </div>
      <div class="note">
        没有信号时，系统不应该"猜"一个方向。空仓等待也是决策。
      </div>
    </template>

    <template v-else-if="!accepted.length && lastSig">
      <div class="verdict">今日不做 T</div>
      <div class="verdict-sub">
        {{ signals.length }} 个候选，0 个通过。收盘 regime：{{ last.regime }}　偏离
        {{ last.z.toFixed(2) }}σ
      </div>

      <div class="chips">
        <span v-for="c in chips" :key="c.k" class="chip">{{ c.k }} ×{{ c.v }}</span>
      </div>

      <div class="divider"></div>

      <div class="mini">
        <span>最近候选 <b>{{ lastSig.id }}</b> · {{ lastSig.t }} · {{ lastSig.side }}</span>
        <span>{{ lastSig.conf ? "置信 " + lastSig.conf + "%" : "已否决" }}</span>
      </div>

      <div class="kv">
        <div>触发价 <span>{{ lastSig.price.toFixed(2) }}</span></div>
        <div>偏离 <span>{{ lastSig.z.toFixed(2) }}σ</span></div>
        <div>目标 <span>{{ lastSig.target.toFixed(2) }}</span></div>
        <div>止损 <span>{{ lastSig.stop.toFixed(2) }}</span></div>
        <div>毛价差 <span>{{ (lastSig.gross * 100).toFixed(2) }}%</span></div>
        <div>净价差 <span>{{ (lastSig.net * 100).toFixed(2) }}%</span></div>
      </div>

      <div class="why">否决：{{ lastSig.veto.join("；") }}</div>

      <div v-if="loose && loose.stats.filled > 0" class="note">
        纪律收益估算：若放宽到 0.25% 门槛，今日会做 {{ loose.stats.filled }} 笔、净收益
        {{ money(loose.stats.totalNet) }}（其中成本 ¥{{ fmt(loose.stats.totalCost) }}）。<br />
        系统只给决策与参数，成交需在券商 App 手工执行并回填。
      </div>
      <div v-else class="note">
        系统只给决策与参数，成交需在券商 App 手工执行并回填。
      </div>
    </template>

    <template v-else-if="lastSig">
      <div class="verdict">{{ lastSig.side }} · {{ lastSig.price.toFixed(2) }}</div>
      <div class="verdict-sub">
        置信 {{ lastSig.conf }}%　有效期 3 分钟　regime {{ lastSig.regime }}
      </div>

      <div class="chips">
        <span v-for="w in lastSig.why" :key="w" class="chip">{{ w }}</span>
      </div>

      <div class="divider"></div>

      <div class="kv">
        <div>入场区间 <span>{{ lastSig.entry.toFixed(2) }}</span></div>
        <div>数量 <span>{{ lastSig.qty }} 股</span></div>
        <div>目标 <span>{{ lastSig.target.toFixed(2) }}</span></div>
        <div>止损 <span>{{ lastSig.stop.toFixed(2) }}</span></div>
        <div>预期毛利 <span>{{ (lastSig.gross * 100).toFixed(2) }}%</span></div>
        <div>预期净利 <span>{{ (lastSig.net * 100).toFixed(2) }}%</span></div>
      </div>

      <div class="note">
        只提示，不下单。复制参数到券商 App 执行，成交后回填。
      </div>
    </template>
  </div>
</template>
