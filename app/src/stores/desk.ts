import { defineStore } from "pinia";
import { computed, ref } from "vue";
import demoJson from "../data/demo.json";
import type {
  AlertEvent,
  AlertItem,
  AuditIssue,
  BacktestResult,
  DeskPayload,
  DeskStats,
  DailyPlan,
  Fill,
  FillInput,
  HistoryImportResult,
  ImportResult,
  JournalEntry,
  JournalInput,
  JournalStats,
  LedgerDay,
  MarketHealth,
  MinuteBar,
  MinuteDay,
  PrecheckInput,
  PrecheckResult,
  PlanAudit,
  PlanInput,
  ResearchReport,
  RiskState,
  SampleResult,
  Scorecard,
  Signal,
  Snapshot,
  StrategyConfig,
  WatchItem,
  WatchScan
} from "../lib/types";
import {
  call,
  isTauri,
  subscribeAlerts,
  subscribeHealth,
  subscribeLedger,
  subscribeRisk,
  subscribeSignals,
  subscribeTick
} from "../lib/ipc";

const demo = demoJson as unknown as DeskPayload;

export const useDeskStore = defineStore("desk", () => {
  const payload = ref<DeskPayload>(demo);
  const mode = ref<"demo" | "live">("demo");
  const live = ref<Snapshot | null>(null);
  const bootError = ref<string | null>(null);
  const emergency = ref(false);
  const version = ref<string>("web");
  const fills = ref<Fill[]>([]);
  const ledger = ref<LedgerDay | null>(null);
  const config = ref<StrategyConfig | null>(null);
  const watchlist = ref<WatchItem[]>([]);
  const scan = ref<WatchScan[]>([]);
  const alerts = ref<AlertItem[]>([]);
  const audit = ref<AuditIssue[]>([]);
  const journal = ref<JournalEntry[]>([]);
  const journalStats = ref<JournalStats | null>(null);
  const planAudit = ref<PlanAudit[]>([]);
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
      await subscribeRisk((r) => {
        payload.value.risk = r;
      });
      await subscribeHealth((h) => {
        payload.value.health = h;
        emergency.value = h.emergency;
      });
      await subscribeLedger((l) => {
        ledger.value = l;
      });
      emergency.value = boot.health?.emergency ?? false;
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

  async function savePlan(input: PlanInput): Promise<DailyPlan> {
    const plan = await call<DailyPlan>("daily_plan_save", { input });
    payload.value.plan = plan;
    await loadPlanAudit(input.day, input.symbol);
    return plan;
  }

  async function loadPlanAudit(day: string, symbol: string): Promise<void> {
    if (!isTauri() || !day || !symbol) return;
    planAudit.value = await call<PlanAudit[]>("plan_audit_list", { day, symbol });
  }

  async function precheckOrder(input: PrecheckInput): Promise<PrecheckResult> {
    return call<PrecheckResult>("precheck_order", { input });
  }

  async function setEmergencyStop(active: boolean): Promise<boolean> {
    const next = await call<boolean>("set_emergency_stop", { active });
    emergency.value = next;
    if (payload.value.health) payload.value.health.emergency = next;
    return next;
  }

  async function loadJournal(day: string, symbol: string): Promise<void> {
    if (!isTauri()) return;
    try {
      journal.value = await call<JournalEntry[]>("journal_list", { day, symbol });
      journalStats.value = await call<JournalStats>("journal_stats", { day, symbol });
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function saveJournal(input: JournalInput): Promise<JournalEntry> {
    const entry = await call<JournalEntry>("journal_upsert", { input });
    await loadJournal(input.day, input.symbol);
    return entry;
  }

  async function deleteJournal(id: number, day: string, symbol: string): Promise<void> {
    await call<void>("journal_delete", { id });
    await loadJournal(day, symbol);
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
    await refreshSignals().catch(() => {});
    return fill;
  }

  async function refreshSignals(): Promise<void> {
    if (!isTauri()) return;
    const r = await call<{
      signals: Signal[];
      stats: DeskStats;
      conclusion: string;
      risk: RiskState;
    }>("refresh_signals");
    payload.value.signals = r.signals;
    payload.value.stats = r.stats;
    payload.value.meta.conclusion = r.conclusion;
    payload.value.risk = r.risk;
  }

  async function loadLedger(day: string): Promise<void> {
    if (!isTauri()) return;
    try {
      ledger.value = await call<LedgerDay>("ledger_day", {
        day,
        symbol: activeSymbol(),
        price: payload.value.last.price
      });
    } catch (e) {
      bootError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function voidFill(id: number, day: string): Promise<void> {
    await call<void>("void_fill", { id });
    await loadFills(day);
    await loadLedger(day);
    await refreshSignals().catch(() => {});
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
    await refreshSignals().catch(() => {});
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
    emergency,
    precheckOrder,
    setEmergencyStop,
    savePlan,
    planAudit,
    loadPlanAudit,
    journal,
    journalStats,
    loadJournal,
    saveJournal,
    deleteJournal,
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
    importStatement,
    refreshSignals
  };
});
