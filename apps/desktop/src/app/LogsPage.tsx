import { useCallback, useEffect, useRef, useState } from "react";
import { Chips } from "../components/Chips";
import { LogView, type LogLine } from "../components/LogView";
import { Page } from "../components/Page";
import { Switch } from "../components/Switch";
import { TextField } from "../components/TextField";
import { call, message, type LogSources, type LogTail } from "./api";
import { useRead } from "./hooks";

type Level = LogLine["level"];

const pad = (n: number) => String(n).padStart(2, "0");
const clock = (d: Date) => (Number.isNaN(d.getTime()) ? "" : `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`);

/** A level from the word a log uses for it. */
export function levelOf(word: string): Level {
  const w = word.toLowerCase();
  if (/^(error|err|fatal|panic|critical|crit|alert|emergency|emerg|dpanic)/.test(w) || w.includes("fatal") || w.includes("parse error")) return "error";
  if (/^warn/.test(w) || w.includes("warning")) return "warn";
  if (/^debug|^trace/.test(w)) return "debug";
  return "info";
}

const months: Record<string, number> = { jan: 0, feb: 1, mar: 2, apr: 3, may: 4, jun: 5, jul: 6, aug: 7, sep: 8, oct: 9, nov: 10, dec: 11 };

/**
 * One raw line as a log line, or `null` when it carries no time or level of its own and so
 * continues the line before (a stack trace, say). Knows Caddy's JSON, Laravel's, PHP's
 * error log, Kiln's daemon and workers, and timestamped lines such as MySQL's; anything else
 * is shown as it is, with a level only when it names one.
 */
export function parseLine(raw: string): LogLine | null {
  const line = raw.replace(/\s+$/, "");
  // Caddy, and other JSON lines.
  if (line.startsWith("{")) {
    try {
      const j = JSON.parse(line) as Record<string, unknown>;
      const ts = typeof j.ts === "number" ? new Date(j.ts * 1000) : typeof j.time === "string" ? new Date(j.time) : null;
      const rest = Object.entries(j)
        .filter(([k]) => !["level", "ts", "time", "msg", "message", "logger"].includes(k))
        .map(([k, v]) => `${k}=${typeof v === "string" ? v : JSON.stringify(v)}`);
      const msg = String(j.msg ?? j.message ?? "");
      return {
        time: ts ? clock(ts) : "",
        level: levelOf(String(j.level ?? "info")),
        text: [j.logger ? `${j.logger}: ${msg}` : msg, ...rest].filter(Boolean).join("  "),
      };
    } catch {
      // Not JSON after all: shown as it is below.
    }
  }
  // Laravel: [2026-11-13 10:00:00] local.ERROR: message
  let m = /^\[(\d{4}-\d\d-\d\d)[ T](\d\d:\d\d:\d\d)[^\]]*\]\s+[\w-]+\.(\w+):\s?(.*)$/.exec(line);
  if (m) return { time: m[2], level: levelOf(m[3]), text: m[4] };
  // PHP's error log: [13-Nov-2026 10:00:00 UTC] PHP Warning:  message
  m = /^\[(\d\d)-(\w{3})-(\d{4}) (\d\d):(\d\d):(\d\d)(?: ([A-Z]+|[\w/+-]+))?\]\s+(.*)$/.exec(line);
  if (m) {
    const utc = m[7] === "UTC";
    const args = [Number(m[3]), months[m[2].toLowerCase()] ?? 0, Number(m[1]), Number(m[4]), Number(m[5]), Number(m[6])] as const;
    const at = utc ? new Date(Date.UTC(...args)) : new Date(...args);
    const php = /^PHP ([\w ]+?):\s+(.*)$/.exec(m[8]);
    return { time: clock(at), level: php ? levelOf(php[1]) : levelOf(m[8].split(/\s/)[0] ?? ""), text: m[8] };
  }
  // Kiln's daemon: 2026-11-13 11:57:55.123 +00:00:00 text (UTC)
  m = /^(\d{4}-\d\d-\d\d) (\d\d:\d\d:\d\d)(\.\d+)? \+00:00:00 (.*)$/.exec(line);
  if (m) return { time: clock(new Date(`${m[1]}T${m[2]}Z`)), level: guess(m[4]), text: m[4] };
  // Kiln's PHP workers: seconds since 1970, then the text.
  m = /^(\d{10}) (.*)$/.exec(line);
  if (m) return { time: clock(new Date(Number(m[1]) * 1000)), level: guess(m[2]), text: m[2] };
  // ISO timestamps (MySQL, MariaDB, PostgreSQL, Meilisearch, Mailpit…).
  m = /^\[?(\d{4}-\d\d-\d\d)[T ](\d\d:\d\d:\d\d)(\.\d+)?(Z|[+-]\d\d:?\d\d)?\]?\s+(.*)$/.exec(line);
  if (m) {
    const at = m[4] ? new Date(`${m[1]}T${m[2]}${m[4].length === 5 ? `${m[4].slice(0, 3)}:${m[4].slice(3)}` : m[4]}`) : new Date(`${m[1]}T${m[2]}`);
    return { time: clock(at), level: guess(m[5]), text: m[5] };
  }
  // Redis: 1234:M 13 Nov 2026 10:00:00.123 * message
  m = /^\d+:[A-Z] (\d\d? \w{3} \d{4}) (\d\d:\d\d:\d\d)\.\d+ ([.\-*#]) (.*)$/.exec(line);
  if (m) return { time: m[2], level: m[3] === "#" ? "warn" : m[3] === "." || m[3] === "-" ? "debug" : "info", text: m[4] };
  // A continuation: indented, a stack frame, or Laravel's [stacktrace] block.
  if (/^(\s|#\d+ |\[stacktrace\]|\{main\}|Stack trace:|"\})/.test(line) || line === "") return null;
  return { time: "", level: guess(line), text: line };
}

/** A level from words in the text: [ERROR], level=warn, "fatal". */
function guess(text: string): Level {
  const m = /\[(error|warn(?:ing)?|note|info|debug|system|fatal|crit(?:ical)?)\]|\blevel=(\w+)|\b(ERROR|FATAL|PANIC|WARN(?:ING)?)\b/i.exec(text);
  if (!m) return "info";
  const word = m[1] ?? m[2] ?? m[3];
  return /note|system/i.test(word) ? "info" : levelOf(word);
}

/** Raw lines as log lines, continuations joined to the line they belong to. */
export function parseLines(raw: string[], before: LogLine[] = []): LogLine[] {
  const out = [...before];
  for (const r of raw) {
    const line = parseLine(r);
    const last = out.at(-1);
    if (line) out.push(line);
    else if (last) out[out.length - 1] = { ...last, text: `${last.text}\n${r.replace(/\s+$/, "")}` };
    else if (r.trim()) out.push({ time: "", level: "info", text: r });
  }
  return out;
}

/** The most lines kept while following. */
const KEPT = 3000;
const EVERY_MS = 1000;

/** A log's last lines, then (while following) what's written to it, about once a second. */
export function useLogFollow(source: string | null, follow: boolean) {
  const [lines, setLines] = useState<LogLine[]>([]);
  const [error, setError] = useState<string>();
  const [path, setPath] = useState<string>();
  const offset = useRef<number | null>(null);
  const busy = useRef(false);

  const poll = useCallback(async () => {
    if (!source || busy.current) return;
    busy.current = true;
    try {
      const tail = await call<LogTail>("logs.tail", offset.current === null ? { source, lines: 500 } : { source, after: offset.current });
      setPath(tail.path);
      setError(undefined);
      const fresh = offset.current === null || tail.reset;
      offset.current = tail.offset;
      if (fresh) setLines(parseLines(tail.lines).slice(-KEPT));
      else if (tail.lines.length) setLines((old) => parseLines(tail.lines, old).slice(-KEPT));
    } catch (e) {
      setError(message(e));
    } finally {
      busy.current = false;
    }
  }, [source]);

  useEffect(() => {
    offset.current = null;
    setLines([]);
    setPath(undefined);
    setError(undefined);
    poll();
  }, [poll]);

  useEffect(() => {
    if (!follow || !source) return;
    const timer = setInterval(poll, EVERY_MS);
    return () => clearInterval(timer);
  }, [follow, source, poll]);

  return { lines, error, path, reload: poll };
}

type Shown = "all" | "warn" | "error";

/** A log, its filters and follow switch: the Logs page's right side, and a site's Logs tab. */
export function LogPanel({ source, label, tall = false }: { source: string; label: string; tall?: boolean }) {
  const [follow, setFollow] = useState(true);
  const [shown, setShown] = useState<Shown>("all");
  const [find, setFind] = useState("");
  const { lines, error, path } = useLogFollow(source, follow);
  const needle = find.trim().toLowerCase();
  const visible = lines.filter(
    (l) => (shown === "all" || (shown === "warn" ? l.level === "warn" || l.level === "error" : l.level === "error")) && (!needle || l.text.toLowerCase().includes(needle)),
  );
  return (
    <div className="min-w-0">
      <div className="mb-3 flex flex-wrap items-end justify-between gap-x-4 gap-y-2">
        <Chips
          label="Show"
          value={shown}
          onChange={setShown}
          options={[
            { id: "all", label: "Everything" },
            { id: "warn", label: "Warnings and errors", count: lines.filter((l) => l.level === "warn" || l.level === "error").length },
            { id: "error", label: "Errors", count: lines.filter((l) => l.level === "error").length },
          ]}
        />
        <div className="flex items-end gap-4">
          <TextField label="Find" className="w-[180px]" value={find} onChange={(e) => setFind(e.target.value)} placeholder="Text in a line" />
          <div className="w-[120px] pb-1">
            <Switch label="Follow" checked={follow} onChange={setFollow} />
          </div>
        </div>
      </div>
      {error && <p className="m-0 mb-2 text-[13px] text-bad">{error}</p>}
      {lines.length === 0 && !error ? (
        <p className="m-0 rounded-xl border border-line px-4 py-6 text-[13px] text-text-2">{path ? `${label}: nothing written yet.` : "Reading…"}</p>
      ) : (
        <LogView label={`${label} log`} lines={visible} follow={follow} tall={tall} />
      )}
      {path && <p className="m-0 mt-2 break-all font-mono text-[11.5px] text-text-3">{path}</p>}
    </div>
  );
}

// Logs (ADR 0059): every log Kiln knows of on the left, the chosen one followed on the right.
export function LogsPage({ first }: { first?: string }) {
  const sources = useRead<LogSources>("logs.sources");
  const [source, setSource] = useState<string | null>(first ?? null);
  const all = sources.data?.sources ?? [];
  const current = all.find((s) => s.id === source) ?? all.find((s) => s.exists) ?? all[0];
  const groups = [...new Set(all.map((s) => s.group))];
  return (
    <Page title="Logs">
      {sources.error && <p className="m-0 px-1 text-[13px] text-text-2">{sources.error}</p>}
      <div className="grid grid-cols-[200px_1fr] gap-6">
        <nav aria-label="Logs">
          {groups.map((g) => (
            <div key={g} className="mb-3">
              <div className="px-2 pb-1 text-xs font-medium text-text-3">{g}</div>
              {all
                .filter((s) => s.group === g)
                .map((s) => (
                  <button
                    key={s.id}
                    type="button"
                    aria-current={s.id === current?.id ? "true" : undefined}
                    onClick={() => setSource(s.id)}
                    title={s.path}
                    className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] ${
                      s.id === current?.id ? "bg-hover font-medium text-text forced-colors:outline forced-colors:outline-1" : "text-text-2 hover:bg-hover hover:text-text"
                    }`}
                  >
                    <span className="flex-1 truncate">{s.label}</span>
                    {!s.exists && <span className="text-[11px] text-text-3">none yet</span>}
                  </button>
                ))}
            </div>
          ))}
          {sources.data && !groups.includes("Sites") && (
            <p className="m-0 px-2 text-xs text-text-3">A Laravel site's log shows here once it writes one, in storage/logs.</p>
          )}
        </nav>
        {current ? <LogPanel key={current.id} source={current.id} label={current.label} tall /> : <span />}
      </div>
    </Page>
  );
}
