import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AlertEvent, DeskStats, Signal, Snapshot } from "./types";

/** Tauri 2 注入 __TAURI_INTERNALS__；浏览器里为 undefined，用于降级到 demo 数据。 */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error("IPC 仅在 Tauri 运行时可调用");
  return invoke<T>(cmd, args);
}

export async function subscribeTick(handler: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("market:tick", (event) => handler(event.payload));
}

export async function subscribeFeed(
  handler: (s: { connected: boolean; source: string; latencyMs: number }) => void
): Promise<UnlistenFn> {
  return listen("feed:status", (event) => handler(event.payload as never));
}

export interface SignalsUpdate {
  signals: Signal[];
  stats: DeskStats;
  conclusion: string;
}

/** 每分钟分时刷新后，Rust 会重算信号并通过该事件推送。 */
export async function subscribeSignals(
  handler: (p: SignalsUpdate) => void
): Promise<UnlistenFn> {
  return listen<SignalsUpdate>("signals:update", (event) => handler(event.payload));
}

/** 批量扫描发现“通过数增加”时推送，用于应用内消息中心。 */
export async function subscribeAlerts(
  handler: (a: AlertEvent) => void
): Promise<UnlistenFn> {
  return listen<AlertEvent>("signals:alert", (event) => handler(event.payload));
}
