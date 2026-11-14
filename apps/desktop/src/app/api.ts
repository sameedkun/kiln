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
  /** The project's folder (ADR 0055 §1). */
  project: string;
  /** What it's built on (P4C-04): `laravel`, `statamic`, `symfony`, … `php`, `static`. */
  framework?: string;
  /** The Git branch checked out in its folder, or the commit when detached (P4C-05). */
  git_branch?: string | null;
  /** The group it's shown in (ADR 0074). */
  group?: string | null;
  framework_version: string | null;
  node: { version: string | null; asked_by: string | null; asks_for: string | null; fell_back: boolean };
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
  /** The port apps connect to (for Mailpit, SMTP, not its web page). */
  main_port: number | null;
  /** The sites whose .env points at it (ADR 0056 §1). */
  used_by: string[];
  /** The host of the site serving its web page, such as mailpit.test. */
  web: string | null;
  /** It holds databases: dump, restore and browse. */
  databases: boolean;
  enabled: boolean;
  running: boolean;
  project: string | null;
  /** What `kiln env use` writes into a site's .env (ADR 0068 §3). */
  env: [string, string][];
}
/** `services.databases` (ADR 0068 §1). */
export interface ServiceDatabases {
  databases: { name: string; bytes: number; used_by: string[] }[];
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

export interface Finding {
  area: string;
  status: "ok" | "info" | "warn" | "fail";
  message: string;
  fix?: string;
}
export interface DoctorReport {
  findings: Finding[];
}
export interface PortSetting {
  port: number;
  auto: boolean;
}
export interface KilnSettings {
  tld: string;
  http: PortSetting;
  https: PortSetting;
  php_workers: number;
  mcp_allow_changes?: boolean;
  check_kiln_updates?: boolean;
}

export interface LogSource {
  id: string;
  group: string;
  label: string;
  path: string;
  exists: boolean;
  bytes: number | null;
  modified_ms: number | null;
}
export interface LogSources {
  sources: LogSource[];
}
export interface LogTail {
  source: string;
  path: string;
  lines: string[];
  offset: number;
  reset: boolean;
}

export interface Footprint {
  processes: { label: string; count: number; memory_bytes: number; cpu_percent: number }[];
  memory_bytes: number;
  folders: { label: string; path: string; bytes: number }[];
  disk_bytes: number;
  cache_bytes: number;
}

export interface Updates {
  php: { version: string; newest: string; update: boolean }[];
  kiln: { version: string; checking: boolean; latest: string | null; update: boolean; error: string | null; page: string };
}

export interface SiteRequests {
  host: string;
  count: number;
  failures: number;
  median_ms: number;
  p95_ms: number;
  since: number;
  slowest: { at: number; method: string; uri: string; status: number; ms: number }[];
  /** Requests in each of the last 24 hours, oldest first (P4C-05). */
  hours?: number[];
  /** Of those, the ones that failed (5xx). */
  failing_hours?: number[];
  statuses?: { "2xx": number; "3xx": number; "4xx": number; "5xx": number };
}
export interface RequestsList {
  log: string;
  sites: SiteRequests[];
}

export interface ServiceKind {
  kind: string;
  name: string;
  license: string;
  versions: string[];
  ports: { role: string; usual: number; what: string }[];
  runs_in_project: boolean;
}
export interface ServicesCatalog {
  kinds: ServiceKind[];
}
export interface ServiceLogs {
  files: { path: string; lines: string[] }[];
}
export interface EnvShow {
  path: string;
  keys: { key: string; value: string; secret: boolean; kiln: boolean }[];
}

/** What's set up (`kiln setup`, ADR 0057 §1). */
export interface SetupStatus {
  ready: boolean;
  web: { installed: boolean; running: boolean };
  php: { versions: string[]; default: string | null };
  composer: boolean;
  node: number;
  ca: { exists: boolean; trusted: boolean | null };
  hosts: { sites: number; missing: string[] };
  shims_on_path: boolean;
  login_item: "app" | "daemon" | "other" | "none";
}

export interface PhpList {
  default: string | null;
  versions: { version: string; dir: string; installed: boolean; system: boolean; xdebug?: boolean; xdebug_available?: boolean }[];
}
export interface PhpAvailable {
  releases: { line: string; version: string; archive: string; installed: boolean }[];
}
export interface PhpExtensions {
  php: string;
  ini: string;
  static_build: boolean;
  toggles: { name: string; on: boolean }[];
  built_in: string[];
  distribution: string[];
}
export interface NodeList {
  default: string | null;
  versions: { version: string; lts: string | null; dir: string }[];
}
export interface NodeAvailable {
  releases: { line: number; version: string; lts: string | null; date: string; installed: boolean }[];
}

/** One line a job printed (ADR 0053 §1). */
export interface JobEvent {
  type: "note" | "warning" | "progress" | "step" | "preview" | "confirm";
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
/** A site's folder in Explorer, by its host name (ADR 0055 §2). */
export const openFolder = (host: string) => invoke("open_folder", { host });
export const openReleases = () => invoke("open_releases");
/** Windows' open dialog for a .sql file (ADR 0056 §3). */
export const pickSql = (title: string) => invoke<string | null>("pick_sql", { title });
/** Windows' save dialog, for where a dump goes. */
export const pickSave = (title: string, name: string) => invoke<string | null>("pick_save", { title, name });
/** Windows' folder dialog: the folder chosen, or null. */
export const pickFolder = (title: string) => invoke<string | null>("pick_folder", { title });

/** A site name as Kiln takes it: nothing a command line could read as an option (ADR 0055 §3). */
export function siteName(text: string): string | null {
  const name = text.trim().toLowerCase().replace(/\.test$/, "");
  return /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/.test(name) ? name : null;
}
/** A proxy's target: a port, or a loopback address with a port. */
export function proxyTarget(text: string): string | null {
  const t = text.trim();
  if (/^\d{1,5}$/.test(t)) return Number(t) > 0 && Number(t) < 65536 ? t : null;
  const m = /^(?:https?:\/\/)?(127\.0\.0\.1|localhost|\[::1\]):(\d{1,5})$/.exec(t);
  return m && Number(m[2]) > 0 && Number(m[2]) < 65536 ? t : null;
}
export const setTray = (state: "running" | "attention" | "stopped", tooltip: string) => invoke("set_tray", { state, tooltip });
export const fitPopup = (height: number) => invoke("fit_popup", { height });

export interface AppSettings {
  start_in: "window" | "tray";
  close_notice_shown: boolean;
  onboarding_dismissed: boolean;
}
export const settingsGet = () => invoke<AppSettings>("settings_get");
/** What Kiln's login item starts (ADR 0054 §6). */
export const loginItem = () => invoke<"app" | "daemon" | "other" | "none">("login_item");
export const setOnboardingDismissed = (dismissed: boolean) => invoke("set_onboarding_dismissed", { dismissed });
export const setStartIn = (startIn: "window" | "tray") => invoke("set_start_in", { startIn });

/** A job's error, or what a failed call said, in words for the page. */
export function message(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
