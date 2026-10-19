import { defineStore } from "pinia";
import { computed, ref } from "vue";
import demoJson from "../data/demo.json";
import type { DeskPayload, Fill, FillInput, MinuteBar, Quote, Snapshot } from "../lib/types";
import { call, isTauri, subscribeTick } from "../lib/ipc";

const demo = demoJson as unknown as DeskPayload;

function toClock(ts: string): string {
  // 腾讯返回 "20261019161457"
  if (ts.length >= 12) return `${ts.slice(8, 10)}:${ts.slice(10, 12)}`;
  return "--:--";
}

export const useDeskStore = defineStore("desk", () => {
  const payload = ref<DeskPayload>(demo);
  const mode = ref<"demo" | "live">("demo");
  const live = ref<Snapshot | null>(null);
  const bootError = ref<string | null>(null);
  const version = ref<string>("web");
  const fills = ref<Fill[]>([]);

  const minutes = computed<MinuteBar[]>(() => {
    const base = payload.value.minutes.slice();
    const s = live.value;
    if (!s) return base;
    const i = base.findIndex((m) => m.t === s.t);
    if (i >= 0) {
      base[i] = { ...base[i], ...s } as MinuteBar;
    } else if (base.length) {
      base.push({ ...base[base.length - 1], ...s } as MinuteBar);
    }
    return base;
  });

  const last = computed(() => {
    const l = live.value ?? payload.value.last;
    return { t: l.t, regime: l.regime, z: l.z, price: l.price, vwap: l.vwap };
  });

  async function load(): Promise<void> {
    if (!isTauri()) return;
    try {
      const info = await call<{ version: string }>("app_info");
      version.value = info.version;
      const q = await call<Quote>("fetch_quote");
      live.value = {
        t: toClock(q.time),
        price: q.price,
        avg: q.price,
        vwap: q.price,
        z: 0,
        slope: 0,
        volRatio: 0,
        regime: "震荡",
        latencyMs: 0
      };
      mode.value = "live";
      await subscribeTick((s) => {
        live.value = s;
      });
      await loadFills(payload.value.meta.date);
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function loadFills(day: string): Promise<void> {
    if (!isTauri()) return;
    try {
      fills.value = await call<Fill[]>("list_fills", { day });
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function submitFill(input: FillInput): Promise<Fill> {
    const fill = await call<Fill>("submit_fill", { input });
    await loadFills(input.day);
    return fill;
  }

  return { payload, mode, live, last, minutes, bootError, version, fills, load, loadFills, submitFill };
});
