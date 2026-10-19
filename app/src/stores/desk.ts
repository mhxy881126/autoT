import { defineStore } from "pinia";
import { computed, ref } from "vue";
import demoJson from "../data/demo.json";
import type {
  AlertEvent,
  AlertItem,
  AuditIssue,
  BacktestResult,
  DeskPayload,
  Fill,
  FillInput,
  HistoryImportResult,
  ImportResult,
  LedgerDay,
  MinuteBar,
  MinuteDay,
  ResearchReport,
  SampleResult,
  Scorecard,
  Snapshot,
  StrategyConfig,
  WatchItem,
  WatchScan
} from "../lib/types";
import { call, isTauri, subscribeAlerts, subscribeSignals, subscribeTick } from "../lib/ipc";

const demo = demoJson as unknown as DeskPayload;

export const useDeskStore = defineStore("desk", () => {
  const payload = ref<DeskPayload>(demo);
  const mode = ref<"demo" | "live">("demo");
  const live = ref<Snapshot | null>(null);
  const bootError = ref<string | null>(null);
  const version = ref<string>("web");
  const fills = ref<Fill[]>([]);
  const ledger = ref<LedgerDay | null>(null);
  const config = ref<StrategyConfig | null>(null);
  const watchlist = ref<WatchItem[]>([]);
  const scan = ref<WatchScan[]>([]);
  const alerts = ref<AlertItem[]>([]);
  const audit = ref<AuditIssue[]>([]);
  const unread = computed(() => alerts.value.filter((a) => !a.read).length);

  function pushAlert(event: AlertEvent): void {
    alerts.value.unshift({
      ...event,
      id: Date.now() + Math.random(),
      read: false
    });
    if (alerts.value.length > 50) alerts.value.length = 50;
  }

  function markAlertsRead(): void {
    alerts.value.forEach((a) => {
      a.read = true;
    });
  }

  function clearAlerts(): void {
    alerts.value = [];
  }

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
      await loadConfig();
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
      await subscribeAlerts((a) => pushAlert(a));
      await loadAudit();
      await loadFills(boot.meta.date);
      await loadLedger(boot.meta.date);
      await loadWatchlist();
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function loadConfig(): Promise<void> {
    if (!isTauri()) return;
    try {
      config.value = await call<StrategyConfig>("get_config");
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function reload(): Promise<void> {
    if (!isTauri()) return;
    const boot = await call<DeskPayload>("bootstrap");
    payload.value = boot;
    live.value = null;
    mode.value = "live";
    await loadFills(boot.meta.date);
    await loadLedger(boot.meta.date);
  }

  async function saveConfig(next: StrategyConfig): Promise<StrategyConfig> {
    const saved = await call<StrategyConfig>("update_config", { next });
    config.value = saved;
    await reload();
    await loadWatchlist();
    return saved;
  }

  async function loadAudit(): Promise<void> {
    if (!isTauri()) return;
    try {
      audit.value = await call<AuditIssue[]>("audit_report");
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function loadWatchlist(): Promise<void> {
    if (!isTauri()) return;
    try {
      watchlist.value = await call<WatchItem[]>("list_watchlist");
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function scanWatchlist(): Promise<void> {
    if (!isTauri()) return;
    try {
      scan.value = await call<WatchScan[]>("scan_watchlist");
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function switchSymbol(symbol: string): Promise<void> {
    const cfg = await call<StrategyConfig>("set_symbol", { symbol });
    config.value = cfg;
    await reload();
    await loadWatchlist();
  }

  async function addSymbol(symbol: string): Promise<void> {
    const cfg = await call<StrategyConfig>("add_symbol", { symbol });
    config.value = cfg;
    await reload();
    await loadWatchlist();
  }

  async function removeSymbol(symbol: string): Promise<void> {
    await call<void>("remove_symbol", { symbol });
    await loadConfig();
    await reload();
    await loadWatchlist();
  }

  const minuteDays = ref<MinuteDay[]>([]);

  async function loadMinuteDays(symbol: string): Promise<MinuteDay[]> {
    if (!isTauri()) return [];
    const days = await call<MinuteDay[]>("list_minute_days", { symbol });
    minuteDays.value = days;
    return days;
  }

  async function generateSample(symbol: string): Promise<SampleResult> {
    return call<SampleResult>("generate_sample_data", { symbol, days: 10 });
  }

  async function clearSample(symbol: string): Promise<number> {
    return call<number>("clear_sample_data", { symbol });
  }

  async function importMinutesCsv(
    symbol: string,
    bytes: number[]
  ): Promise<HistoryImportResult> {
    return call<HistoryImportResult>("import_minutes_csv", { symbol, bytes });
  }

  async function backtestDay(
    symbol: string,
    day: string,
    prevClose?: number
  ): Promise<BacktestResult> {
    return call<BacktestResult>("backtest_day", { symbol, day, prevClose });
  }

  async function backtestRange(symbol: string, days: string[]): Promise<Scorecard> {
    return call<Scorecard>("backtest_range", { symbol, days });
  }

  async function backtestResearch(symbol: string, days: string[]): Promise<ResearchReport> {
    return call<ResearchReport>("backtest_research", { symbol, days });
  }

  function activeSymbol(): string {
    return config.value?.symbol ?? payload.value.meta.symbolId ?? "";
  }

  async function loadFills(day: string): Promise<void> {
    if (!isTauri()) return;
    try {
      fills.value = await call<Fill[]>("list_fills", { day, symbol: activeSymbol() });
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
      ledger.value = await call<LedgerDay>("ledger_day", { day, symbol: activeSymbol() });
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function voidFill(id: number, day: string): Promise<void> {
    await call<void>("void_fill", { id });
    await loadFills(day);
    await loadLedger(day);
  }

  async function importStatement(
    day: string,
    fileName: string,
    bytes: number[]
  ): Promise<ImportResult> {
    const result = await call<ImportResult>("import_statement", {
      day,
      symbol: activeSymbol(),
      fileName,
      bytes
    });
    await loadFills(day);
    await loadLedger(day);
    return result;
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
    config,
    watchlist,
    scan,
    alerts,
    audit,
    unread,
    pushAlert,
    markAlertsRead,
    clearAlerts,
    loadAudit,
    load,
    loadConfig,
    reload,
    saveConfig,
    loadWatchlist,
    scanWatchlist,
    switchSymbol,
    addSymbol,
    removeSymbol,
    minuteDays,
    loadMinuteDays,
    generateSample,
    clearSample,
    importMinutesCsv,
    backtestDay,
    backtestRange,
    backtestResearch,
    loadFills,
    loadLedger,
    submitFill,
    voidFill,
    importStatement
  };
});
