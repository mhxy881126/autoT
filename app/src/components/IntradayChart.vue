<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import * as echarts from "echarts";
import type { MinuteBar, Signal } from "../lib/types";

const props = defineProps<{
  minutes: MinuteBar[];
  signals: Signal[];
  prevClose: number;
}>();

const el = ref<HTMLDivElement | null>(null);
let chart: ReturnType<typeof echarts.init> | null = null;
let ro: ResizeObserver | null = null;

const REGIME_COLOR: Record<string, string> = {
  上行: "rgba(239,83,80,.065)",
  下行: "rgba(38,166,154,.065)",
  震荡: "rgba(255,255,255,.016)",
  极端: "rgba(232,179,57,.10)"
};

function regimeAreas(mins: MinuteBar[]) {
  const out: unknown[] = [];
  if (!mins.length) return out;
  let start = 0;
  for (let i = 1; i <= mins.length; i++) {
    if (i === mins.length || mins[i].regime !== mins[start].regime) {
      out.push([
        {
          xAxis: mins[start].t,
          itemStyle: { color: REGIME_COLOR[mins[start].regime] ?? "transparent" }
        },
        { xAxis: mins[i - 1].t }
      ]);
      start = i;
    }
  }
  return out;
}

function buildOption(): any {
  const mins = props.minutes;
  const times = mins.map((m) => m.t);

  const buyPts: any[] = [];
  const sellPts: any[] = [];
  props.signals.forEach((s) => {
    const ok = s.accepted;
    const pt = {
      value: [s.t, s.price],
      symbolSize: ok ? 13 : 8,
      itemStyle: {
        color: ok ? (s.side === "正T" ? "#ef5350" : "#26a69a") : "rgba(120,132,152,.45)",
        borderColor: "#0a0d13",
        borderWidth: 1
      },
      label: {
        show: ok,
        formatter: s.side === "正T" ? "正" : "倒",
        position: "top",
        color: "#cfd8e6",
        fontSize: 9,
        fontWeight: 600
      }
    };
    (s.side === "正T" ? buyPts : sellPts).push(pt);
  });

  const volData = mins.map((m, i) => {
    const p = i > 0 ? mins[i - 1].price : m.price;
    return {
      value: m.vol,
      itemStyle: { color: m.price >= p ? "rgba(239,83,80,.42)" : "rgba(38,166,154,.42)" }
    };
  });

  return {
    animation: false,
    backgroundColor: "transparent",
    grid: [
      { left: 58, right: 16, top: 18, bottom: "27%" },
      { left: 58, right: 16, height: "15%", bottom: 26 }
    ],
    xAxis: [
      {
        type: "category",
        data: times,
        boundaryGap: false,
        axisLine: { lineStyle: { color: "#2a3342" } },
        axisTick: { show: false },
        axisLabel: { show: false },
        splitLine: { show: false }
      },
      {
        type: "category",
        data: times,
        gridIndex: 1,
        boundaryGap: false,
        axisLine: { lineStyle: { color: "#2a3342" } },
        axisTick: { show: false },
        axisLabel: {
          color: "#6b7688",
          fontSize: 10,
          interval: 0,
          hideOverlap: true,
          formatter: (v: string) => (v.endsWith(":00") || v.endsWith(":30") ? v : "")
        }
      }
    ],
    yAxis: [
      {
        scale: true,
        gridIndex: 0,
        splitNumber: 5,
        splitLine: { lineStyle: { color: "#161d29" } },
        axisLabel: { color: "#6b7688", fontSize: 10, formatter: (v: number) => v.toFixed(0) }
      },
      {
        gridIndex: 1,
        splitNumber: 1,
        axisLabel: { show: false },
        axisLine: { show: false },
        axisTick: { show: false },
        splitLine: { show: false }
      }
    ],
    tooltip: {
      trigger: "axis",
      axisPointer: { type: "cross", lineStyle: { color: "#3a4354" }, crossStyle: { color: "#3a4354" } },
      backgroundColor: "rgba(12,16,24,.95)",
      borderColor: "#2a3342",
      borderWidth: 1,
      textStyle: { color: "#dbe2ee", fontSize: 11 },
      formatter: (ps: any[]) => {
        const i = ps[0].dataIndex;
        const m = mins[i];
        if (!m) return "";
        const sig = props.signals.find((s) => s.t === m.t);
        let html = `<div style="font-family:Consolas,monospace;line-height:1.7">
          <b style="color:#fff">${m.t}</b>　价格 <b>${m.price.toFixed(2)}</b>　均价 ${m.avg.toFixed(2)}<br>
          VWAP ${m.vwap.toFixed(2)}　偏离 <b style="color:${m.z >= 0 ? "#ef5350" : "#26a69a"}">${m.z >= 0 ? "+" : ""}${m.z.toFixed(2)}σ</b><br>
          量比 ${m.volRatio.toFixed(2)}　regime <b>${m.regime}</b>`;
        if (sig) {
          html += `<br><span style="color:${sig.accepted ? "#e8b339" : "#8c99ad"}">${
            sig.accepted ? "通过" : "否决"
          }：${sig.side} ${sig.accepted ? "目标 " + sig.target.toFixed(2) : sig.veto[0] ?? ""}</span>`;
        }
        return html + "</div>";
      }
    },
    series: [
      {
        name: "价格",
        type: "line",
        data: mins.map((m) => m.price),
        symbol: "none",
        lineStyle: { width: 1.3, color: "#dbe2ee" },
        areaStyle: {
          color: {
            type: "linear",
            x: 0,
            y: 0,
            x2: 0,
            y2: 1,
            colorStops: [
              { offset: 0, color: "rgba(219,226,238,.10)" },
              { offset: 1, color: "rgba(219,226,238,0)" }
            ]
          }
        },
        markArea: { silent: true, data: regimeAreas(mins) },
        markLine: {
          silent: true,
          symbol: "none",
          data: [{ yAxis: props.prevClose }],
          lineStyle: { color: "#3a4354", type: "dashed", width: 1 },
          label: {
            formatter: "昨收 " + props.prevClose.toFixed(2),
            color: "#6b7688",
            fontSize: 10,
            position: "insideEndTop"
          }
        }
      },
      { name: "均价", type: "line", data: mins.map((m) => m.avg), symbol: "none", lineStyle: { width: 1, color: "#e8b339" } },
      { name: "VWAP20", type: "line", data: mins.map((m) => m.vwap), symbol: "none", lineStyle: { width: 1, type: "dashed", color: "#4fc3f7" } },
      { name: "正T", type: "scatter", data: buyPts, symbol: "triangle", symbolSize: 13, z: 6 },
      { name: "倒T", type: "scatter", data: sellPts, symbol: "triangle", symbolRotate: 180, symbolSize: 13, z: 6 },
      { name: "成交量", type: "bar", data: volData, xAxisIndex: 1, yAxisIndex: 1, barWidth: "62%" }
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

watch(() => props.minutes, render, { deep: false });
watch(() => props.signals, render, { deep: false });

onBeforeUnmount(() => {
  ro?.disconnect();
  chart?.dispose();
});
</script>

<template>
  <div ref="el" class="chart"></div>
</template>
