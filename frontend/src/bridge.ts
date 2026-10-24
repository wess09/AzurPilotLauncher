import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  acceptSnapshot,
  type Context,
  type Snapshot,
  type Phase,
} from "./types";

export const native = "__TAURI_INTERNALS__" in window;
const preview = import.meta.env.DEV && !native;
let simulated: Snapshot = {
  sequence: 1,
  task_id: 1,
  phase: "idle",
  operation: null,
  update_version: null,
  update_checked: true,
  backend_running: false,
  progress: null,
  error: null,
  error_code: null,
  connection_error: null,
  message: null,
  can_start: true,
  can_repair: true,
  can_stop: false,
  can_check: true,
  can_update: false,
  can_cancel: false,
};
const scenario = new URLSearchParams(location.search).get("preview");
if (preview && scenario) {
  const phase = (
    {
      progress: "starting",
      repair: "repairing",
      error: "error",
      running: "running",
      update: "update_available",
    } as Record<string, Phase>
  )[scenario];
  if (phase)
    simulated = {
      ...simulated,
      phase,
      operation: phase === "repairing" ? "repair" : "start",
      backend_running: phase === "running",
      can_start: phase === "error",
      can_repair: phase === "error",
      can_stop: phase === "running",
      can_check: false,
      can_update: phase === "update_available",
      can_cancel: phase === "starting" || phase === "repairing",
      update_version: phase === "update_available" ? "3.0.0" : null,
      error:
        phase === "error"
          ? "Failed to connect to the local service.\nConnection refused: 127.0.0.1:22267"
          : null,
      progress: ["starting", "repairing"].includes(phase)
        ? {
            title: "",
            detail: "uv sync · Python 3.14.6",
            subtitle: "",
            progress: 64.5,
            uv_progress: {
              progress: 38.5,
              detail: "Downloading dependencies · 4.2 MB/s",
              task_id: 1,
            },
            is_error: false,
            terminal: false,
            sequence: 1,
          }
        : null,
    };
}
let previewListener: ((state: Snapshot) => void) | undefined;
export async function command<T = void>(
  name: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!preview) return invoke<T>(name, args);
  if (name === "launcher_context")
    return {
      locale: new URLSearchParams(location.search).get("lang") ?? "zh-CN",
      version: "2.3.0",
      platform: "windows",
    } as T;
  if (name === "launcher_snapshot") return simulated as T;
  if (name === "launcher_start")
    simulated = {
      ...simulated,
      phase: "starting",
      operation: "start",
      can_start: false,
      can_repair: false,
      can_cancel: true,
    };
  if (name === "launcher_stop" || name === "launcher_back")
    simulated = {
      ...simulated,
      phase: "idle",
      backend_running: false,
      can_start: true,
      can_repair: true,
      can_stop: false,
      error: null,
      progress: null,
    };
  previewListener?.({ ...simulated, sequence: ++simulated.sequence });
  return undefined as T;
}
export async function subscribe(
  callback: (s: Snapshot) => void,
  closeRequested: () => void,
) {
  let current: Snapshot | null = null;
  let disposed = false;
  const receive = (next: Snapshot) => {
    const accepted = acceptSnapshot(current, next);
    if (!disposed && accepted && accepted !== current) {
      current = accepted;
      callback(accepted);
    }
  };
  const unlisten = preview
    ? () => {
        previewListener = undefined;
      }
    : await listen<Snapshot>("launcher://state", (e) => receive(e.payload));
  if (preview) previewListener = receive;
  let closeUnlisten: (() => void) | undefined;
  try {
    if (!preview)
      closeUnlisten = await listen(
        "launcher://close-requested",
        closeRequested,
      );
    receive(await command<Snapshot>("launcher_snapshot"));
  } catch (error) {
    unlisten();
    closeUnlisten?.();
    throw error;
  }
  return () => {
    disposed = true;
    unlisten();
    closeUnlisten?.();
  };
}
export const getContext = () => command<Context>("launcher_context");
// 日志归档打包进度（done/total 字节）；preview 模拟无此事件。
export async function subscribeLogArchiveProgress(
  callback: (progress: { done: number; total: number }) => void,
): Promise<() => void> {
  if (preview) return () => {};
  const unlisten = await listen<{ done: number; total: number }>(
    "launcher://log-archive-progress",
    (e) => callback(e.payload),
  );
  return () => unlisten();
}
