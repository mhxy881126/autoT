export * from "../../../contracts/ipc";

import type { AlertEvent, KLine, MinuteBar, Signal, Snapshot } from "../../../contracts/ipc";

export interface AlertItem extends AlertEvent {
  id: number;
  read: boolean;
}

export interface AuditIssue {
  day: string;
  symbol: string;
  count: number;
}

export interface DeskMeta {
  symbol: string;
  symbolId: string;
  code: string;
  market: string;
  date: string;
  prevClose: number;
  open: number;
  high: number;
  low: number;
  close: number;
  atrPct: number;
  generatedAt: string;
  mode: string;
  conclusion: string;
}

export interface DeskAccount {
  baseQty: number;
  baseCost: number;
  available: number;
  cash: number;
  lot: number;
  maxTRatio: number;
}

export interface Trade {
  id: string;
  side: string;
  entryT: string;
  exitT: string;
  entry: number;
  exit: number;
  qty: number;
  grossPct: number;
  netPct: number;
  grossPnl: number;
  cost: number;
  netPnl: number;
  reason: string;
  holdMin: number;
}

export interface DeskStats {
  candidates: number;
  accepted: number;
  filled: number;
  wins: number;
  losses: number;
  winRate: number;
  avgNetPct: number;
  avgWin: number;
  avgLoss: number;
  totalNet: number;
  totalCost: number;
  totalGross: number;
  costRatio: number | null;
  maxDd: number;
  vetoReasons: Record<string, number>;
}

export interface Scenario {
  key: string;
  label: string;
  desc: string;
  stats: DeskStats;
  trades: Trade[];
}

export interface SensitivityRow {
  threshold: number;
  accepted: number;
  filled: number;
  net: number;
  winRate: number;
}

export interface LastState {
  t: string;
  regime: string;
  z: number;
  price: number;
  vwap: number;
  slope: number;
}

export interface Quote {
  name: string;
  price: number;
  prevClose: number;
  open: number;
  high: number;
  low: number;
  time: string;
}

export interface DeskPayload {
  meta: DeskMeta;
  account: DeskAccount;
  minutes: MinuteBar[];
  kline: KLine[];
  signals: Signal[];
  trades: Trade[];
  equity: Array<{ t: string; pnl: number }>;
  stats: DeskStats;
  scenarios: Scenario[];
  sensitivity: SensitivityRow[];
  last: LastState;
}

export type { KLine, MinuteBar, Signal, Snapshot };
