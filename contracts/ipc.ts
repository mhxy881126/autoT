/**
 * IPC 契约（单一来源）
 *
 * 规则：
 * 1. Rust 侧 serde 必须 `#[serde(rename_all = "camelCase")]`，字段名与此文件一致。
 * 2. 前端只做格式化，不做业务计算（可用数量、次数、盈亏都由 Rust 给）。
 * 3. 所有写操作带 clientId，Rust 侧唯一索引去重。
 *
 * 金额：Rust 内部用整数分（*_cents），跨 IPC 时统一转成元（number），
 * 避免前端出现 0.1 + 0.2 的浮点尾差。
 */

export type Regime = "震荡" | "上行" | "下行" | "极端";
export type TSide = "正T" | "倒T";
export type FillSide = "BUY" | "SELL";

export interface Snapshot {
  t: string;          // "14:57"
  price: number;
  avg: number;
  vwap: number;
  z: number;
  slope: number;
  volRatio: number;
  regime: Regime;
  latencyMs?: number;
}

export interface MinuteBar extends Snapshot {
  vol: number;
  high: number;
  low: number;
  dayRet: number;
  amp: number;
}

export interface KLine {
  d: string;          // "2026-10-19"
  o: number;
  c: number;
  h: number;
  l: number;
  v: number;
  ma5?: number | null;
  ma10?: number | null;
  ma20?: number | null;
}

export interface Signal {
  id: string;
  day: string;
  t: string;
  side: TSide;
  price: number;
  entry: number;
  target: number;
  stop: number;
  qty: number;
  gross: number;      // 毛利价差，小数（0.0045 = 0.45%）
  net: number;
  conf: number;
  regime: Regime;
  z: number;
  accepted: boolean;
  veto: string[];     // 否决理由，给用户看
  why: string[];      // 通过理由
}

export interface FillInput {
  clientId: string;   // UUID，幂等键
  symbol: string;     // "sh600519"，多票账务隔离
  day: string;        // trade_day，Asia/Shanghai
  t: string;
  side: FillSide;
  price: number;
  qty: number;
  fee?: number;
  signalId?: string;
  note?: string;
}

export interface Fill extends FillInput {
  id: number;
  fee: number;
  source: "manual" | "import";
  createdAt: string;
  voided: boolean;
}

export interface TPair {
  id: number;
  day: string;
  symbol: string;
  side: TSide;
  openFillId: number;
  closeFillId: number | null;
  qty: number;
  gross: number | null;
  cost: number | null;
  net: number | null;
  status: "OPEN" | "CLOSED" | "UNWOUND";
}

export interface Position {
  baseQty: number;
  available: number;
  frozen: number;
  tQty: number;
  baseCost: number;
  updatedAt: string;
}

export interface LedgerDay {
  day: string;
  symbol: string;
  position: Position;
  fills: Fill[];
  pairs: TPair[];
  realizedNet: number;
  unrealized: number;
  unresolved: number;   // 未闭环 T 笔数，次日必须提示
}

export interface PlanLevel {
  label: string;
  price: number;
}

export interface DailyPlan {
  day: string;
  symbol: string;
  bias: string;
  maxTrades: number;
  riskBudgetPct: number;
  levels: PlanLevel[];
  note: string | null;
  locked: boolean;
  lockedReason: string | null;
  updatedAt: string;
}

export interface PlanInput {
  day: string;
  symbol: string;
  bias: string;
  maxTrades: number;
  riskBudgetPct: number;
  levels: PlanLevel[];
  note?: string | null;
  reason?: string | null;
}

export interface PlanAudit {
  id: number;
  day: string;
  symbol: string;
  changedAt: string;
  reason: string | null;
  before: DailyPlan | null;
  after: DailyPlan;
}

export interface ActionAudit {
  id: number;
  action: string;
  target: string;
  changedAt: string;
  reason: string | null;
  before: Record<string, unknown> | boolean | null;
  after: Record<string, unknown> | boolean | null;
}

export interface MarketHealth {
  phase: string;
  tradingDay: boolean;
  lastBar: string;
  ageSecs: number;
  stale: boolean;
  healthy: boolean;
  emergency: boolean;
  calendarCovered: boolean;
  calendarProvisional: boolean;
  source: string;
  message: string;
}

export interface PrecheckInput {
  symbol: string;
  day: string;
  side: "BUY" | "SELL";
  price: number;
  qty: number;
  signalId?: string;
  signalT?: string;
  signalPrice?: number;
}

export interface PrecheckCheck {
  name: string;
  ok: boolean;
  detail: string;
}

export interface PrecheckResult {
  ok: boolean;
  blockers: string[];
  warnings: string[];
  checks: PrecheckCheck[];
}

export interface StrategyConfig {
  symbol: string;          // "sh600519"
  baseQty: number;
  baseCost: number;        // 0 = 用近 20 日均价
  cash: number;
  maxTRatio: number;       // 0.5
  zEntry: number;
  targetLo: number;
  targetHi: number;
  stopPct: number;
  maxSignals: number;
  cooldownMin: number;
  sessionStart: string;    // "09:45"
  sessionEnd: string;      // "14:50"
  paused: boolean;
  dailyLossLimit: number;       // 0.01
  maxConsecutiveLosses: number; // 3
  liquidityCap: number;         // 0.01
}

export interface RiskState {
  blocked: boolean;
  reasons: string[];
  dailyPnl: number;
  realizedPnl: number;
  unrealizedPnl: number;
  dailyLimit: number;
  consecutiveLosses: number;
  tradesToday: number;
  maxTrades: number;
  positionNotional: number;
  liquidityCap: number;
}

export interface JournalInput {
  symbol: string;
  day: string;
  openFillId: number;
  closeFillId?: number | null;
  planTarget?: number | null;
  planStop?: number | null;
  planned: boolean;
  reason?: string;
  emotion?: string;
  mistakes: string[];
  note?: string;
}

export interface JournalEntry {
  id: number;
  symbol: string;
  day: string;
  openFillId: number;
  closeFillId: number | null;
  planTarget: number | null;
  planStop: number | null;
  planned: boolean;
  reason: string | null;
  emotion: string | null;
  mistakes: string[];
  note: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface PnlBucket {
  trades: number;
  net: number;
  avg: number;
  wins: number;
  winRate: number;
}

export interface MistakePnl {
  name: string;
  count: number;
  net: number;
  avg: number;
}

export interface JournalStats {
  entries: number;
  planned: number;
  unplanned: number;
  adherence: number;
  mistakes: Array<{ name: string; count: number }>;
  emotions: Array<{ name: string; count: number }>;
  plannedPnl: PnlBucket;
  unplannedPnl: PnlBucket;
  pnlDeltaAvg: number;
  mistakePnl: MistakePnl[];
  planTargetNet: number;
  planStopNet: number;
  actualVsTarget: number;
  planTrades: number;
}

export type Config = StrategyConfig;

export interface AlertEvent {
  symbol: string;
  name: string;
  accepted: number;
  side: string;
  t: string;
  target: number | null;
  at: string;
  test: boolean;
}

export interface WatchItem {
  symbol: string;
  name: string;
  price: number;
  changePct: number;
  baseQty: number;
  paused: boolean;
}

export interface WatchScan {
  symbol: string;
  name: string;
  price: number;
  changePct: number;
  regime: string;
  candidates: number;
  accepted: number;
  paused: boolean;
  signalSide: string | null;
  signalT: string | null;
  signalTarget: number | null;
  cached: boolean;
  stale: boolean;
}

export interface BacktestTrade {
  signalId: string;
  side: string;
  entryT: string;
  entryPrice: number;
  exitT: string;
  exitPrice: number;
  qty: number;
  gross: number;
  fees: number;
  slippage: number;
  net: number;
  exitReason: string;
  barsHeld: number;
  maePct: number;
  mfePct: number;
}

export interface BacktestSummary {
  trades: number;
  wins: number;
  losses: number;
  gross: number;
  fees: number;
  slippage: number;
  net: number;
  winRate: number;
  profitFactor: number;
  maxDrawdown: number;
  maxDrawdownPct: number;
  avgHoldMin: number;
}

export interface BacktestResult {
  symbol: string;
  day: string;
  trades: BacktestTrade[];
  summary: BacktestSummary;
  skipped: string[];
  cashEnd: number;
  availableBase: number;
  startEquity: number;
}

export interface DayNet {
  day: string;
  trades: number;
  net: number;
  fees: number;
  slippage: number;
  noTrade: boolean;
}

export interface Scorecard {
  symbol: string;
  days: number;
  daysWithTrades: number;
  noTradeDays: number;
  trades: number;
  wins: number;
  losses: number;
  gross: number;
  fees: number;
  slippage: number;
  net: number;
  winRate: number;
  profitFactor: number;
  maxDrawdown: number;
  maxDrawdownPct: number;
  avgNetPerDay: number;
  edgeVsNoTrade: number;
  daily: DayNet[];
}

export interface HistoryImportResult {
  symbol: string;
  days: number;
  rows: number;
  skipped: number;
}

export interface MinuteDay {
  day: string;
  source: string;
}

export interface SampleResult {
  symbol: string;
  days: number;
  rows: number;
  seed: number;
}

export interface ParamCell {
  zEntry: number;
  targetLo: number;
  trades: number;
  net: number;
  winRate: number;
  maxDrawdown: number;
}

export interface SelectedParams {
  zEntry: number;
  targetLo: number;
  inSampleNet: number;
  inSampleTrades: number;
  outSampleNet: number;
  outSampleTrades: number;
}

export interface FoldResult {
  trainDays: number;
  testDays: number;
  zEntry: number;
  targetLo: number;
  trainNet: number;
  testNet: number;
  testTrades: number;
}

export interface ResearchReport {
  symbol: string;
  totalDays: number;
  inSampleDays: string[];
  outSampleDays: string[];
  note: string;
  demoDays: number;
  selection: SelectedParams | null;
  currentOutSample: Scorecard | null;
  bestOutSample: Scorecard | null;
  sensitivity: ParamCell[];
  walkForward: FoldResult[];
}

export interface FeedStatus {
  connected: boolean;
  source: "tencent" | "sina" | "none";
  latencyMs: number;
  lastError?: string;
}

export interface ImportResult {
  imported: number;
  duplicated: number;
  mismatched: number;
  manualOnly: number;
  rows: number;
  errors: string[];
  message: string;
}

export interface Bootstrap {
  version: string;
  config: StrategyConfig;
  position: Position;
  signals: Signal[];
  snapshot: Snapshot | null;
  feed: FeedStatus;
}
