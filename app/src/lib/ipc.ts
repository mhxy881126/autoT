import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Snapshot } from "./types";

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
