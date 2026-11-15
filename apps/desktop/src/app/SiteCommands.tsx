import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "../components/Button";
import { Notice } from "../components/Notice";
import { TextField } from "../components/TextField";
import { call, message, runJob, type JobEvent, type JobFinished, type Site, type SiteCommands as Commands } from "./api";
import { useRead } from "./hooks";

/** The most lines kept on screen; older ones scroll away. */
const KEEP = 4000;

type Suggestion = Commands["suggested"][number];
interface JobSummary {
  job: number;
  args: string[];
  running: boolean;
  stoppable?: boolean;
}

/** A typed command as words: spaces split, "quotes" keep spaces. */
export function splitCommand(text: string): string[] {
  const words: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  for (const m of text.matchAll(re)) words.push(m[1] ?? m[2] ?? m[3]);
  return words;
}

/** Why Kiln won't run these words for this site, or null (ADR 0084 §1). */
export function commandError(words: string[], has: Commands | undefined): string | null {
  const first = words[0];
  if (!first) return null;
  if (first === "composer") return null;
  if (first === "artisan") return has && !has.artisan ? "This site has no artisan." : null;
  const tool = first.replace(/^vendor[\\/]bin[\\/]/, "");
  if (has && has.tools.includes(tool)) return null;
  return `Starts with artisan, composer or a tool in vendor/bin${has?.tools.length ? ` (${has.tools.slice(0, 6).join(", ")}${has.tools.length > 6 ? ", …" : ""})` : ""}.`;
}

/** How a finished run reads: done, stopped (neutral), or failed with Kiln's message. */
export function endText(done: Pick<JobFinished, "ok" | "last">): { tone: "ok" | "neutral" | "bad"; text: string } {
  if (done.ok) return { tone: "ok", text: "Done (exit code 0)." };
  if (done.last.message === "stopped") return { tone: "neutral", text: "Stopped." };
  return { tone: "bad", text: done.last.message ?? "Failed." };
}

// The site's own commands (ADR 0084 §5): suggestions, any artisan/composer/vendor/bin
// command, its output as it comes, and Stop. Kept mounted by the site page while hidden, and
// re-attached to a run still going after the page was left.
export function SiteCommands({ site }: { site: Site }) {
  const has = useRead<Commands>("site.commands", { host: site.host });
  const [text, setText] = useState("");
  const [lines, setLines] = useState<string[]>([]);
  const [shown, setShown] = useState<string | null>(null);
  const [job, setJob] = useState<number | null>(null);
  const [running, setRunning] = useState(false);
  const [end, setEnd] = useState<ReturnType<typeof endText> | null>(null);
  const [asking, setAsking] = useState<Suggestion | null>(null);
  const out = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (out.current) out.current.scrollTop = out.current.scrollHeight;
  }, [lines]);

  const noteLines = (events: JobEvent[]) => events.filter((e) => e.type === "note" && e.text !== undefined).map((e) => e.text!);

  // A run of this site's still going (the page was left and opened again): follow it.
  const follow = useCallback(async (id: number, words: string[]) => {
    setShown(words.join(" "));
    setJob(id);
    setRunning(true);
    setEnd(null);
    for (;;) {
      const record = await call<{ running: boolean; finished: JobFinished | null; events: JobEvent[] }>("jobs.get", { job: id }).catch(() => null);
      if (!record) break;
      setLines(noteLines(record.events).slice(-KEEP));
      if (!record.running) {
        if (record.finished) setEnd(endText(record.finished));
        break;
      }
      await new Promise((r) => setTimeout(r, 1000));
    }
    setRunning(false);
    setJob(null);
  }, []);

  useEffect(() => {
    let live = true;
    call<JobSummary[]>("jobs.list")
      .then((list) => {
        const going = list.find((j) => j.running && j.stoppable && j.args[0] === "run" && j.args[1] === site.host);
        if (going && live) void follow(going.job, going.args.slice(2));
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [site.host, follow]);

  const start = async (words: string[]) => {
    if (running || words.length === 0) return;
    setAsking(null);
    setShown(words.join(" "));
    setLines([]);
    setEnd(null);
    setRunning(true);
    try {
      const done = await runJob(
        ["run", site.host, ...words],
        "apply",
        (e: JobEvent) => {
          if (e.type === "note" && e.text !== undefined) setLines((l) => [...l, e.text!].slice(-KEEP));
        },
        setJob,
      );
      setEnd(endText(done));
    } catch (e) {
      setEnd({ tone: "bad", text: message(e) });
    }
    setRunning(false);
    setJob(null);
  };

  const words = splitCommand(text);
  const problem = commandError(words, has.data);

  return (
    <div className="flex flex-col gap-4">
      {has.data && (has.data.suggested.length > 0 || has.data.scripts.length > 0) && (
        <div className="flex flex-wrap gap-2">
          {has.data.suggested.map((s) => (
            <Button key={s.label} size="sm" disabled={running} title={s.args.join(" ")} onClick={() => (s.warn ? setAsking(s) : start(s.args))}>
              {s.label}
            </Button>
          ))}
          {has.data.scripts.map((name) => (
            <Button key={name} size="sm" variant="quiet" disabled={running} title={`composer ${name}`} onClick={() => start(["composer", name])}>
              composer {name}
            </Button>
          ))}
        </div>
      )}
      {asking && (
        <Notice
          tone="attention"
          title={`${asking.label}?`}
          actions={
            <>
              <Button size="sm" onClick={() => setAsking(null)}>
                Cancel
              </Button>
              <Button size="sm" variant="ink" onClick={() => start(asking.args)}>
                Run them
              </Button>
            </>
          }
        >
          {asking.warn}
        </Notice>
      )}
      {has.error && <p className="m-0 text-[13px] text-text-2">{has.error}</p>}
      <div className="flex flex-wrap items-start gap-3">
        <TextField
          className="min-w-[280px] flex-1"
          label="Command"
          mono
          placeholder="artisan migrate:status"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !problem) void start(words);
          }}
          hint={`With ${site.host}'s PHP${site.php?.version ? ` (${site.php.version})` : ""}, in its folder. It runs your project's code; it can't ask questions here.`}
          error={problem ?? undefined}
        />
        <div className="flex gap-2 pt-[22px]">
          <Button variant="ink" disabled={running || words.length === 0 || problem !== null} onClick={() => start(words)}>
            {running ? "Running…" : "Run"}
          </Button>
          {running && (
            <Button disabled={job === null} onClick={() => job !== null && call("jobs.stop", { job }).catch(() => {})}>
              Stop
            </Button>
          )}
        </div>
      </div>
      {shown !== null && (
        <section aria-label="Output">
          <div className="mb-1.5 flex items-center gap-2 text-xs text-text-2">
            <code className="font-mono text-text">{shown}</code>
            {end && <span className={end.tone === "bad" ? "text-bad" : ""}>{end.text}</span>}
          </div>
          <pre ref={out} className={`max-h-[420px] min-h-[120px] overflow-auto whitespace-pre-wrap rounded-lg border p-3 font-mono text-xs ${end?.tone === "bad" ? "border-bad" : "border-line"}`}>
            {lines.length ? lines.join("\n") : running ? "Waiting for output…" : "(no output)"}
          </pre>
        </section>
      )}
    </div>
  );
}
