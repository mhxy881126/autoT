<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import type { KLine } from "../lib/types";

const props = defineProps<{ kline: KLine[] }>();
const el = ref<HTMLDivElement | null>(null);
let chart: ReturnType<typeof echarts.init> | null = null;
let ro: ResizeObserver | null = null;

function buildOption(): any {
  const k = props.kline;
  return {
    animation: false,
    backgroundColor: "transparent",
    grid: { left: 58, right: 16, top: 14, bottom: 22 },
    xAxis: {
      type: "category",
      data: k.map((x) => x.d.slice(5)),
      axisLine: { lineStyle: { color: "#2a3342" } },
      axisTick: { show: false },
      axisLabel: { color: "#6b7688", fontSize: 10, interval: 9 }
    },
    yAxis: {
      scale: true,
      splitNumber: 4,
      splitLine: { lineStyle: { color: "#161d29" } },
      axisLabel: { color: "#6b7688", fontSize: 10, formatter: (v: number) => v.toFixed(0) }
    },
    tooltip: {
      trigger: "axis",
      axisPointer: { type: "cross", lineStyle: { color: "#3a4354" } },
      backgroundColor: "rgba(12,16,24,.95)",
      borderColor: "#2a3342",
      textStyle: { color: "#dbe2ee", fontSize: 11 },
      formatter: (ps: any[]) => {
        const x = k[ps[0].dataIndex];
        if (!x) return "";
        return `<b>${x.d}</b><br>开 ${x.o.toFixed(2)}　高 ${x.h.toFixed(2)}<br>低 ${x.l.toFixed(2)}　收 <b>${x.c.toFixed(2)}</b>`;
      }
    },
    series: [
      {
        name: "K",
        type: "candlestick",
        data: k.map((x) => [x.o, x.c, x.l, x.h]),
        itemStyle: { color: "#ef5350", color0: "#26a69a", borderColor: "#ef5350", borderColor0: "#26a69a" }
      },
      { name: "MA5", type: "line", data: k.map((x) => x.ma5 ?? null), symbol: "none", lineStyle: { width: 1, color: "#e8b339" } },
      { name: "MA10", type: "line", data: k.map((x) => x.ma10 ?? null), symbol: "none", lineStyle: { width: 1, color: "#4fc3f7" } },
      { name: "MA20", type: "line", data: k.map((x) => x.ma20 ?? null), symbol: "none", lineStyle: { width: 1, color: "#9b8cff" } }
    ]
  };
}

function render() {
  if (chart) chart.setOption(buildOption(), { notMerge: true });
}

onMounted(() => {
  if (!el.value) return;
  chart = echarts.init(el.value, undefined, { renderer: "canvas" });
  render();
  ro = new ResizeObserver(() => chart?.resize());
  ro.observe(el.value);
});

watch(() => props.kline, render, { deep: false });

onBeforeUnmount(() => {
  ro?.disconnect();
  chart?.dispose();
});
</script>

<template>
  <div ref="el" class="chart"></div>
</template>
