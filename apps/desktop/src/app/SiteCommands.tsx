import { useEffect, useRef, useState } from "react";
import { Button } from "../components/Button";
import { TextField } from "../components/TextField";
import { call, message, runJob, type JobEvent, type Site, type SiteCommands as Commands } from "./api";
import { useRead } from "./hooks";

/** The most lines kept on screen; older ones scroll away. */
const KEEP = 4000;

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

// The site's own commands (ADR 0084 §5): suggestions, any artisan/composer/vendor/bin
// command, its output as it comes, and Stop.
export function SiteCommands({ site }: { site: Site }) {
  const has = useRead<Commands>("site.commands", { host: site.host });
  const [text, setText] = useState("");
  const [lines, setLines] = useState<string[]>([]);
  const [shown, setShown] = useState<string | null>(null);
  const [job, setJob] = useState<number | null>(null);
  const [running, setRunning] = useState(false);
  const [end, setEnd] = useState<{ ok: boolean; text: string } | null>(null);
  const out = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (out.current) out.current.scrollTop = out.current.scrollHeight;
  }, [lines]);

  const start = async (words: string[]) => {
    if (running || words.length === 0) return;
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
      const exit = (done.last.data as { exit?: number | null } | undefined)?.exit;
      setEnd(done.ok ? { ok: true, text: "Done (exit code 0)." } : { ok: false, text: done.last.message ?? `Failed${exit != null ? ` (exit code ${exit})` : ""}.` });
    } catch (e) {
      setEnd({ ok: false, text: message(e) });
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
            <Button key={s.label} size="sm" disabled={running} title={s.args.join(" ")} onClick={() => start(s.args)}>
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
            {end && <span className={end.ok ? "" : "text-bad"}>{end.text}</span>}
          </div>
          <pre ref={out} className={`max-h-[420px] min-h-[120px] overflow-auto whitespace-pre-wrap rounded-lg border p-3 font-mono text-xs ${end && !end.ok ? "border-bad" : "border-line"}`}>
            {lines.length ? lines.join("\n") : running ? "Waiting for output…" : "(no output)"}
          </pre>
        </section>
      )}
    </div>
  );
}
