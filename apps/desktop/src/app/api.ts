// The app's way into Kiln (ADR 0054 §3): every read and change is a call to `kilnd` through
// the app's Rust side, and the daemon's events come back as Tauri events. The shapes are
// the CLI's `--json` data (ADR 0053 §3).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Whether the page runs inside the app, not in a plain browser. */
export const inApp = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type DaemonState = "starting" | "running" | "too-old" | "unreachable";

export interface Site {
  host: string;
  url: string;
  driver: string;
  document_root: string;
  secure: boolean;
  source: "parked" | "link" | "alias";
  parked_in: string | null;
  php: { version: string | null; pinned: string | null; from_project: boolean; fell_back: boolean } | null;
  in_hosts: boolean;
}
export interface Proxy {
  host: string;
  url: string;
  target: string | null;
  in_hosts: boolean;
}
export interface SitesList {
  parked: string[];
  sites: Site[];
  proxies: Proxy[];
  skipped: { folder: string; reason: string }[];
  warnings: string[];
}
export interface Instance {
  name: string;
  kind: string;
  version: string;
  ports: Record<string, number>;
  enabled: boolean;
  running: boolean;
  project: string | null;
}
export interface ServicesList {
  supervisor_running: boolean;
  instances: Instance[];
}
export interface Operation {
  id: number;
  at_ms: number | null;
  state: string;
  kind: string;
  description: string;
}
export interface HistoryList {
  operations: Operation[];
  hidden: number;
}

/** One line a job printed (ADR 0053 §1). */
export interface JobEvent {
  type: "note" | "warning" | "progress" | "preview" | "confirm";
  text?: string;
  label?: string;
  done?: number;
  total?: number | null;
  summary?: string;
  changes?: { target: string; before: string | null; after: string | null }[];
  question?: string;
}
export interface JobFinished {
  job: number;
  ok: boolean;
  exit: number | null;
  needs_confirmation: boolean;
  last: { type: "result" | "error"; message?: string; data?: unknown };
}

export function call<T>(method: string, params?: unknown): Promise<T> {
  return invoke<T>("api", { method, params: params ?? null });
}

export const daemonState = () => invoke<DaemonState>("daemon_state");
export const onDaemon = (handler: (state: DaemonState) => void): Promise<UnlistenFn> =>
  listen<DaemonState>("kiln-daemon", (e) => handler(e.payload));
export const onReconnected = (handler: () => void): Promise<UnlistenFn> => listen("kiln-reconnected", () => handler());

interface Notification {
  method: string;
  params: Record<string, unknown>;
}
export const onEvent = (handler: (note: Notification) => void): Promise<UnlistenFn> =>
  listen<Notification>("kiln-event", (e) => handler(e.payload));

/**
 * Runs a change as a job and waits for it to end, passing each line it prints to `onLine`.
 * `preview` stops at the command's confirmation (ADR 0053 §4).
 */
export async function runJob(args: string[], mode: "preview" | "apply", onLine?: (event: JobEvent) => void): Promise<JobFinished> {
  let id: number | null = null;
  let delivered = 0;
  const early: Notification[] = [];
  let settle: (done: JobFinished) => void = () => {};
  const finished = new Promise<JobFinished>((resolve) => (settle = resolve));
  const seen = (note: Notification) => {
    if (note.params.job !== id) return;
    if (note.method === "job.event") {
      delivered += 1;
      onLine?.(note.params.event as JobEvent);
    }
    if (note.method === "job.finished") settle(note.params as unknown as JobFinished);
  };
  // Subscribed before the job starts, so no line is missed; lines that arrive before its id
  // is known wait in `early`.
  const unlisten = await onEvent((note) => (id === null ? early.push(note) : seen(note)));
  try {
    id = (await call<{ job: number }>("jobs.start", { args, mode })).job;
    early.splice(0).forEach(seen);
    // A job that finished before the events arrived: its record has the end.
    const record = await call<{ running: boolean; finished: JobFinished | null; events: JobEvent[] }>("jobs.get", { job: id });
    if (!record.running && record.finished) {
      // Only the lines its events didn't already bring.
      record.events.slice(delivered).forEach((e) => onLine?.(e));
      return record.finished;
    }
    return await finished;
  } finally {
    unlisten();
  }
}

export const windowAction = (action: "minimise" | "maximise" | "close") => invoke("window_action", { action });
export const closeNoticeRead = () => invoke("close_notice_read");
export const showMain = () => invoke("show_main");
export const hidePopup = () => invoke("hide_tray_popup");
export const quit = () => invoke("quit");
export const openSite = (host: string) => invoke("open_site", { host });
export const setTray = (state: "running" | "attention" | "stopped", tooltip: string) => invoke("set_tray", { state, tooltip });
export const fitPopup = (height: number) => invoke("fit_popup", { height });

export interface AppSettings {
  start_in: "window" | "tray";
  close_notice_shown: boolean;
}
export const settingsGet = () => invoke<AppSettings>("settings_get");
/** What Kiln's login item starts (ADR 0054 §6). */
export const loginItem = () => invoke<"app" | "daemon" | "other" | "none">("login_item");
export const setStartIn = (startIn: "window" | "tray") => invoke("set_start_in", { startIn });

/** A job's error, or what a failed call said, in words for the page. */
export function message(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
