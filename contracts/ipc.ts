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
