import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { useDismissToast, useToast } from "../components/Toast";
import { message, openSite, runJob, type JobEvent } from "./api";

// Making a change from the app (ADR 0053 §4): commands that ask before changing anything
// show their preview in a dialog first, and run only when the user confirms; the others
// run straight away. Either way the outcome is a toast.

interface Ask {
  title: string;
  args: string[];
  done: string;
  events: JobEvent[];
  phase: "previewing" | "ready" | "applying";
  error?: string;
}

interface Changes {
  /** Runs a command that changes something at once; says how it went. `onLine` sees its
   * lines as they come, such as a download's progress. */
  run: (args: string[], done: string, onLine?: (event: JobEvent) => void) => Promise<boolean>;
  /** Shows a command's preview, and runs it if the user confirms. */
  confirm: (title: string, args: string[], done: string) => void;
  /** Opens a site in the browser; if its name isn't in the hosts file yet, offers to add it. */
  open: (host: string) => void;
  /** For a change without a preview of its own (unlink, remove): asks in words first. */
  ask: (title: string, body: string, action: string, args: string[], done: string) => void;
}

/** Whether a command adds names that the hosts file needs (a site, a parked folder, a tool). */
export function addsNames(args: string[]): boolean {
  return ["link", "park", "proxy"].includes(args[0]) || (args[0] === "tool" && args[1] === "add") || (args[0] === "service" && args[1] === "add");
}

/** A download's progress in a few words: "Downloading mysql.zip · 34 of 120 MB". */
export function progressText(e: JobEvent): string {
  const mb = (n: number) => (n / 1_048_576).toFixed(0);
  const label = e.label ?? "Downloading";
  const done = e.done ?? 0;
  return e.total ? `${label} · ${mb(done)} of ${mb(e.total)} MB` : `${label} · ${mb(done)} MB`;
}

const Context = createContext<Changes>({ run: async () => false, confirm: () => {}, open: () => {}, ask: () => {} });
export const useChanges = () => useContext(Context);

export function ChangeProvider({ children }: { children: ReactNode }) {
  const toast = useToast();
  const dismiss = useDismissToast();
  // Changes running now, by their arguments: a second click on the same one waits for it.
  const running = useRef(new Set<string>());
  const [ask, setAsk] = useState<Ask | null>(null);
  const [plain, setPlain] = useState<{ title: string; body: string; action: string; args: string[]; done: string; busy: boolean } | null>(null);

  // After a change that adds site names: the hosts file, if it lacks any of them. Writing it
  // needs Windows' administrator prompt, so it's offered, not done; nothing shows when every
  // name is already there.
  const offerHosts = useCallback(() => {
    const args = ["hosts", "sync"];
    const events: JobEvent[] = [];
    runJob(args, "preview", (e) => events.push(e))
      .then((end) => {
        if (end.needs_confirmation) {
          setAsk({ title: "Add the new names to the hosts file?", args, done: "Browsers find the new sites now.", events, phase: "ready" });
        }
      })
      .catch(() => {});
  }, []);

  const run = useCallback(
    async (args: string[], done: string, onLine?: (event: JobEvent) => void) => {
      const key = args.join("\u0000");
      if (running.current.has(key)) {
        toast({ message: "Already on it: that change is still running." });
        return false;
      }
      running.current.add(key);
      // A change that takes a moment says so, and what it's doing, until it ends.
      let working: number | null = null;
      let latest = "Working…";
      const say = (text: string) => {
        latest = text;
        if (working !== null) toast({ message: text, sticky: true, replaces: working });
      };
      const timer = setTimeout(() => (working = toast({ message: latest, sticky: true })), 400);
      try {
        const end = await runJob(args, "apply", (e) => {
          if (e.type === "progress") say(progressText(e));
          else if (e.type === "step" && e.text) say(`${e.text}…`);
          else if (e.type === "note" && e.text?.trim()) say(e.text.trim().length > 90 ? `${e.text.trim().slice(0, 89)}…` : e.text.trim());
          onLine?.(e);
        });
        if (end.ok) {
          toast({ message: done });
          if (addsNames(args)) offerHosts();
          return true;
        }
        toast({ message: end.last.message ?? `kiln ${args.join(" ")} failed` });
      } catch (e) {
        toast({ message: message(e) });
      } finally {
        clearTimeout(timer);
        if (working !== null) dismiss(working);
        running.current.delete(key);
      }
      return false;
    },
    [toast, dismiss, offerHosts],
  );

  const confirm = useCallback((title: string, args: string[], done: string) => {
    const events: JobEvent[] = [];
    setAsk({ title, args, done, events, phase: "previewing" });
    runJob(args, "preview", (e) => events.push(e))
      .then((end) =>
        setAsk((a) =>
          a && a.args === args
            ? end.needs_confirmation
              ? { ...a, events: [...events], phase: "ready" }
              : { ...a, events: [...events], phase: "ready", error: end.ok ? "Nothing needs changing." : end.last.message }
            : a,
        ),
      )
      .catch((e) => setAsk((a) => (a ? { ...a, phase: "ready", error: message(e) } : a)));
  }, []);

  const apply = async () => {
    if (!ask) return;
    setAsk({ ...ask, phase: "applying" });
    await run(ask.args, ask.done);
    setAsk(null);
  };

  const previews = ask?.events.filter((e) => e.type === "preview") ?? [];
  // What the command said before it asked; after the question comes only the preview run's
  // own "Nothing was changed.", which reads wrong in a dialog that is still asking.
  const asked = ask?.events.findIndex((e) => e.type === "confirm") ?? -1;
  const notes = (asked >= 0 ? ask?.events.slice(0, asked) : ask?.events)?.filter((e) => e.type === "note" || e.type === "warning") ?? [];
  const question = ask?.events.find((e) => e.type === "confirm")?.question;

  return (
    <Context.Provider
      value={{
        run,
        confirm,
        open: (host) =>
          openSite(host).catch((e) => {
            const text = message(e);
            if (text.startsWith("not-in-hosts: ")) {
              confirm(`Add ${host} to the hosts file?`, ["hosts", "sync"], `Browsers find ${host} now: open it again.`);
            } else {
              toast({ message: text });
            }
          }),
        ask: (title, body, action, args, done) => setPlain({ title, body, action, args, done, busy: false }),
      }}
    >
      {children}
      <Dialog
        open={ask !== null}
        onClose={() => ask?.phase !== "applying" && setAsk(null)}
        title={ask?.title ?? ""}
        wide
        footer={
          <>
            <Button onClick={() => setAsk(null)} disabled={ask?.phase === "applying"}>
              Cancel
            </Button>
            <Button variant="ink" onClick={apply} disabled={ask?.phase !== "ready" || Boolean(ask?.error)}>
              {ask?.phase === "applying" ? "Working…" : "Confirm"}
            </Button>
          </>
        }
      >
        {ask?.phase === "previewing" && <p className="m-0">Working out what will change…</p>}
        {ask?.error && <p className="m-0 text-text">{ask.error}</p>}
        {notes.map((n, i) => (
          <p key={i} className="m-0 mb-1">
            {n.text}
          </p>
        ))}
        {previews.map((p, i) => (
          <div key={i} className="mt-3">
            <p className="m-0 mb-2 font-medium text-text">{p.summary}</p>
            {p.changes?.map((c, j) => (
              <div key={j} className="mb-2 rounded-lg border border-line p-2.5 font-mono text-xs">
                <div className="mb-1 break-all text-text-2">{c.target}</div>
                <div className="break-all">
                  <span className="text-text-3">before </span>
                  {c.before ?? "(none)"}
                </div>
                <div className="break-all">
                  <span className="text-text-3">after </span>
                  {c.after ?? "(none)"}
                </div>
              </div>
            ))}
          </div>
        ))}
        {question && !ask?.error && <p className="m-0 mt-3 text-text">{question}</p>}
      </Dialog>
      <Dialog
        open={plain !== null}
        onClose={() => !plain?.busy && setPlain(null)}
        title={plain?.title ?? ""}
        footer={
          <>
            <Button onClick={() => setPlain(null)} disabled={plain?.busy}>
              Cancel
            </Button>
            <Button
              variant="ink"
              disabled={plain?.busy}
              onClick={async () => {
                if (!plain) return;
                setPlain({ ...plain, busy: true });
                await run(plain.args, plain.done);
                setPlain(null);
              }}
            >
              {plain?.busy ? "Working…" : plain?.action}
            </Button>
          </>
        }
      >
        {plain?.body}
      </Dialog>
    </Context.Provider>
  );
}
