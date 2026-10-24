export type Phase =
  | "checking"
  | "update_available"
  | "updating"
  | "idle"
  | "starting"
  | "running"
  | "stopping"
  | "repairing"
  | "cancelling"
  | "error";
export type Operation = "check" | "update" | "start" | "stop" | "repair";
export interface Progress {
  title: string;
  detail: string;
  subtitle: string;
  progress: number;
  uv_progress?: { progress: number; detail: string; task_id: number };
  is_error: boolean;
  terminal: boolean;
  sequence: number;
}
export interface Snapshot {
  sequence: number;
  task_id: number;
  phase: Phase;
  operation: Operation | null;
  update_version: string | null;
  update_checked: boolean;
  backend_running: boolean;
  progress: Progress | null;
  error: string | null;
  error_code: number | null;
  connection_error: string | null;
  message: string | null;
  can_start: boolean;
  can_repair: boolean;
  can_stop: boolean;
  can_check: boolean;
  can_update: boolean;
  can_cancel: boolean;
}
export interface Context {
  locale: string;
  version: string;
  platform: string;
}
export const busy = (s: Snapshot) =>
  [
    "checking",
    "updating",
    "starting",
    "stopping",
    "repairing",
    "cancelling",
  ].includes(s.phase);
export function acceptSnapshot(
  current: Snapshot | null,
  next: Snapshot,
): Snapshot | null {
  return Number.isSafeInteger(next.sequence) &&
    next.sequence >= 0 &&
    (!current || next.sequence > current.sequence)
    ? next
    : current;
}
