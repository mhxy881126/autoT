import { defineStore } from "pinia";
import { computed, ref } from "vue";
import demoJson from "../data/demo.json";
import type { DeskPayload, Fill, FillInput, LedgerDay, MinuteBar, Snapshot } from "../lib/types";
import { call, isTauri, subscribeSignals, subscribeTick } from "../lib/ipc";

const demo = demoJson as unknown as DeskPayload;

export const useDeskStore = defineStore("desk", () => {
  const payload = ref<DeskPayload>(demo);
  const mode = ref<"demo" | "live">("demo");
  const live = ref<Snapshot | null>(null);
  const bootError = ref<string | null>(null);
  const version = ref<string>("web");
  const fills = ref<Fill[]>([]);
  const ledger = ref<LedgerDay | null>(null);

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
      const boot = await call<DeskPayload>("bootstrap");
      payload.value = boot;
      mode.value = "live";
      await subscribeTick((s) => {
        live.value = s;
      });
      await subscribeSignals((p) => {
        payload.value.signals = p.signals;
        payload.value.stats = p.stats;
        payload.value.meta.conclusion = p.conclusion;
      });
      await loadFills(boot.meta.date);
      await loadLedger(boot.meta.date);
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
    await loadLedger(input.day);
    return fill;
  }

  async function loadLedger(day: string): Promise<void> {
    if (!isTauri()) return;
    try {
      ledger.value = await call<LedgerDay>("ledger_day", { day });
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function voidFill(id: number, day: string): Promise<void> {
    await call<void>("void_fill", { id });
    await loadFills(day);
    await loadLedger(day);
  }

  return {
    payload,
    mode,
    live,
    last,
    minutes,
    bootError,
    version,
    fills,
    ledger,
    load,
    loadFills,
    loadLedger,
    submitFill,
    voidFill
  };
});
