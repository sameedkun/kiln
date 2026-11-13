import { Checkmark16Regular } from "@fluentui/react-icons";
import { useEffect, useState, type ReactNode } from "react";
import { Button } from "../components/Button";
import { Logo } from "../components/Logo";
import { Meter } from "../components/Meter";
import { Select } from "../components/Select";
import { Switch } from "../components/Switch";
import { call, message, pickFolder, setOnboardingDismissed, siteName, type PhpAvailable, type SetupStatus } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";
import { useProgress } from "./RuntimePages";

// First-run onboarding (ADR 0057): each step says what it changes, every change runs as a job
// with its preview where the command has one, and only the web server and PHP can't be
// skipped. Steps already done say so.

interface Step {
  id: string;
  title: string;
  done: (s: SetupStatus) => boolean;
  required?: boolean;
}

export const steps: Step[] = [
  { id: "welcome", title: "Welcome", done: () => false },
  { id: "web", title: "Web server", done: (s) => s.web.installed && s.web.running, required: true },
  { id: "php", title: "PHP", done: (s) => s.php.versions.length > 0, required: true },
  { id: "https", title: "HTTPS", done: (s) => s.ca.exists && s.ca.trusted !== false },
  { id: "projects", title: "Projects", done: (s) => s.hosts.sites > 0 && s.hosts.missing.length === 0 },
  { id: "terminal", title: "Terminal", done: (s) => s.shims_on_path },
  { id: "signin", title: "Sign-in", done: (s) => s.login_item === "app" },
  { id: "done", title: "Done", done: () => false },
];

/** The first step that isn't done yet, after `from`. */
export function nextStep(status: SetupStatus | undefined, from: number): number {
  for (let i = from + 1; i < steps.length; i++) {
    if (!status || !steps[i].done(status) || steps[i].id === "done") return i;
  }
  return steps.length - 1;
}

export function Onboarding({ onClose }: { onClose: () => void }) {
  const status = useRead<SetupStatus>("setup.status");
  const [at, setAt] = useState(0);
  const step = steps[at];
  const done = status.data ? step.done(status.data) : false;
  const next = () => setAt((i) => nextStep(status.data, i));
  const finish = () => {
    setOnboardingDismissed(true);
    onClose();
  };

  return (
    <div role="dialog" aria-modal="true" aria-labelledby="onboarding-title" className="backdrop absolute inset-x-0 bottom-0 top-11 z-30 grid grid-cols-[260px_1fr]">
      <aside className="glass m-2 mr-0 flex flex-col rounded-panel p-5">
        <div className="mb-6 flex items-center gap-2.5 font-semibold">
          <Logo size={22} /> Set up Kiln
        </div>
        <ol className="m-0 flex list-none flex-col gap-1 p-0">
          {steps.map((s, i) => {
            const finished = status.data ? s.done(status.data) : false;
            return (
              <li key={s.id}>
                <button
                  type="button"
                  aria-current={i === at ? "step" : undefined}
                  onClick={() => setAt(i)}
                  className={`flex h-9 w-full items-center gap-3 rounded-item px-2.5 text-left text-[13px] ${
                    i === at ? "bg-glass-strong font-semibold text-text" : "text-text-2 hover:bg-hover"
                  }`}
                >
                  <span
                    className={`grid size-[22px] shrink-0 place-items-center rounded-full border text-[11px] ${
                      finished ? "border-transparent bg-ok text-white" : i === at ? "border-transparent bg-ink text-on-ink" : "border-line-strong"
                    }`}
                  >
                    {finished ? <Checkmark16Regular aria-label="done" className="size-3" /> : i + 1}
                  </span>
                  {s.title}
                </button>
              </li>
            );
          })}
        </ol>
        <p className="m-0 mt-auto text-xs text-text-3">Each change is shown before it's made, and History can undo it.</p>
      </aside>

      <main className="overflow-auto px-12 py-10">
        <div className="max-w-[640px]">
          {status.error && <p className="text-[13px] text-bad">{status.error}</p>}
          <StepBody id={step.id} status={status.data} done={done} reload={status.reload} />
          <div className="mt-8 flex items-center gap-2 border-t border-line pt-5">
            {step.id === "welcome" && (
              <>
                <Button variant="ink" onClick={next}>
                  Start
                </Button>
                <Button variant="quiet" onClick={finish}>
                  Not now
                </Button>
              </>
            )}
            {step.id === "done" && (
              <Button variant="ink" onClick={finish}>
                Open Kiln
              </Button>
            )}
            {step.id !== "welcome" && step.id !== "done" && (
              <>
                <Button variant={done ? "ink" : "default"} onClick={next} disabled={step.required && !done}>
                  {done ? "Next" : "Skip"}
                </Button>
                {step.required && !done && <span className="text-xs text-text-3">Kiln needs this one to serve sites.</span>}
                <Button variant="quiet" className="ml-auto" onClick={finish}>
                  Finish later
                </Button>
              </>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}

function Heading({ title, children }: { title: string; children: ReactNode }) {
  return (
    <>
      <h1 id="onboarding-title" className="m-0 mb-2 text-[26px] leading-9 font-[650] tracking-[-.02em]">
        {title}
      </h1>
      <div className="mb-6 max-w-[62ch] text-sm text-text-2">{children}</div>
    </>
  );
}

function Done({ children }: { children: ReactNode }) {
  return (
    <p className="m-0 flex items-center gap-2 text-sm text-ok">
      <Checkmark16Regular aria-hidden="true" /> {children}
    </p>
  );
}

function StepBody({ id, status, done, reload }: { id: string; status?: SetupStatus; done: boolean; reload: () => void }) {
  const { run, confirm } = useChanges();
  const { progress, onLine, clear } = useProgress();
  const [busy, setBusy] = useState(false);
  const work = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    await fn();
    clear();
    setBusy(false);
    reload();
  };
  const bar = progress && (
    <div className="mt-4 max-w-[420px]">
      <Meter label={progress.label} value={progress.done} max={progress.total ?? Math.max(progress.done, 1)} display={`${(progress.done / 1_048_576).toFixed(1)} MB`} />
    </div>
  );

  switch (id) {
    case "welcome":
      return (
        <Heading title="Welcome to Kiln">
          <p className="mt-0">Kiln serves your PHP projects at names like shop.test, with their own PHP and Node, the databases they need, and HTTPS. Setting it up takes a few steps:</p>
          <ul className="my-3 pl-5">
            <li>a web server and PHP, downloaded and checked against pinned hashes;</li>
            <li>its own certificate authority, so HTTPS works without warnings;</li>
            <li>your projects' names in the hosts file;</li>
            <li>the php, composer and node commands in your terminals;</li>
            <li>starting in the tray when you sign in.</li>
          </ul>
          <p className="m-0">Nothing changes until you say so: each change is shown first, and History can undo every one.</p>
        </Heading>
      );
    case "web":
      return (
        <>
          <Heading title="The web server">Kiln serves sites with Caddy, downloaded from its releases and checked against the hash Kiln pins. It runs as you, not as an administrator.</Heading>
          {done ? (
            <Done>Installed and running.</Done>
          ) : (
            <Button
              variant="ink"
              disabled={busy}
              onClick={() =>
                work(async () => {
                  if (!status?.web.installed && !(await run(["web", "install"], "The web server is installed.", onLine))) return;
                  await run(["web", "start"], "The web server is running.");
                })
              }
            >
              {busy ? "Working…" : status?.web.installed ? "Start it" : "Install and start it"}
            </Button>
          )}
          {bar}
        </>
      );
    case "php":
      return <PhpStep status={status} done={done} work={work} busy={busy} bar={bar} onLine={onLine} />;
    case "https":
      return (
        <>
          <Heading title="HTTPS">
            Kiln makes its own certificate authority, valid only for .test names, and asks Windows to trust it for your account. Windows shows a security prompt: check the name says Kiln, then
            click Yes.
          </Heading>
          {done ? (
            <Done>Kiln's certificates are trusted.</Done>
          ) : (
            <Button
              variant="ink"
              disabled={busy}
              onClick={() =>
                work(async () => {
                  if (!status?.ca.exists && !(await run(["ca", "init"], "Kiln's certificate authority is made."))) return;
                  confirm("Trust Kiln's certificate authority?", ["ca", "trust"], "Windows trusts Kiln's certificates now.");
                })
              }
            >
              {status?.ca.exists ? "Trust it…" : "Set up HTTPS…"}
            </Button>
          )}
        </>
      );
    case "projects":
      return (
        <>
          <Heading title="Your projects">
            Park a folder of projects and each folder in it becomes a site, or link one project. Then their names go in Windows' hosts file, so browsers find them: Windows asks for
            administrator approval for that.
          </Heading>
          <div className="flex flex-wrap gap-2">
            <Button
              disabled={busy}
              onClick={async () => {
                const folder = await pickFolder("Choose a folder of projects: each folder in it becomes a site");
                if (folder) work(() => run(["park", folder], `Parked ${folder}.`));
              }}
            >
              Park a folder…
            </Button>
            <Button
              disabled={busy}
              onClick={async () => {
                const folder = await pickFolder("Choose a project folder");
                // The same check as the Sites page: nothing a command line could misread.
                const name = siteName(folder?.split(/[\\/]/).filter(Boolean).pop()?.replace(/[^A-Za-z0-9-]/g, "-") ?? "");
                if (folder && name) work(() => run(["link", name, "--path", folder], `Linked ${name}.test.`));
              }}
            >
              Link one project…
            </Button>
          </div>
          {status && status.hosts.sites > 0 && (
            <div className="mt-6">
              {status.hosts.missing.length === 0 ? (
                <Done>All {status.hosts.sites} sites are in the hosts file.</Done>
              ) : (
                <>
                  <p className="m-0 mb-2 text-sm">
                    {status.hosts.missing.length} site{status.hosts.missing.length === 1 ? " isn't" : "s aren't"} in the hosts file yet: {status.hosts.missing.slice(0, 4).join(", ")}
                    {status.hosts.missing.length > 4 ? "…" : ""}
                  </p>
                  <Button variant="ink" onClick={() => confirm("Update the hosts file?", ["hosts", "sync"], "The hosts file has every site now.")}>
                    Add them…
                  </Button>
                </>
              )}
            </div>
          )}
        </>
      );
    case "terminal":
      return (
        <>
          <Heading title="The terminal">
            Kiln puts its php, composer and node commands first on your PATH, so each project's terminal runs the versions it asks for. Kiln shows the change to your PATH first.
          </Heading>
          {done ? <Done>They're on your PATH (new terminals pick them up).</Done> : <Button variant="ink" onClick={() => confirm("Put Kiln's commands on your PATH?", ["shims", "install"], "Kiln's commands are on your PATH: open a new terminal to use them.")}>Show the change…</Button>}
        </>
      );
    case "signin":
      return (
        <>
          <Heading title="Starting at sign-in">Kiln starts in the tray when you sign in, and keeps your sites and services running. Kiln shows the change to your sign-in items first.</Heading>
          {done ? <Done>Kiln starts when you sign in.</Done> : <Button variant="ink" onClick={() => confirm("Start Kiln when you sign in?", ["app", "enable"], "Kiln now starts in the tray when you sign in.")}>Show the change…</Button>}
        </>
      );
    default:
      return (
        <Heading title="Kiln is set up">
          <ul className="my-3 pl-5">
            <li>Web server: {status?.web.running ? "running" : "not running"}</li>
            <li>PHP: {status?.php.versions.join(", ") || "none yet"}</li>
            <li>HTTPS: {status?.ca.exists && status.ca.trusted !== false ? "on" : "not set up"}</li>
            <li>Sites: {status?.hosts.sites ?? 0}</li>
            <li>Commands on PATH: {status?.shims_on_path ? "yes" : "no"}</li>
            <li>Starts at sign-in: {status?.login_item === "app" ? "yes" : "no"}</li>
          </ul>
          Every change is listed in History, where it can be undone. Settings can open this again.
        </Heading>
      );
  }
}

function PhpStep({
  status,
  done,
  work,
  busy,
  bar,
  onLine,
}: {
  status?: SetupStatus;
  done: boolean;
  work: (fn: () => Promise<unknown>) => Promise<void>;
  busy: boolean;
  bar: ReactNode;
  onLine: Parameters<ReturnType<typeof useChanges>["run"]>[2];
}) {
  const { run } = useChanges();
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [line, setLine] = useState("");
  const [composer, setComposer] = useState(true);
  useEffect(() => {
    if (done) return;
    call<PhpAvailable>("php.available").then(
      (a) => {
        const offered = [...new Set(a.releases.map((r) => r.line))].sort((x, y) => Number(y.split(".")[1]) - Number(x.split(".")[1]) || y.localeCompare(x));
        setLines(offered);
        setLine(offered[0] ?? "");
      },
      (e) => setError(message(e)),
    );
  }, [done]);
  return (
    <>
      <Heading title="PHP">Kiln keeps its own PHP builds, side by side; each site uses the default unless it picks another. Composer comes with it if you like.</Heading>
      {done ? (
        <Done>PHP {status?.php.versions.join(", ")} installed.</Done>
      ) : (
        <>
          {error && <p className="text-[13px] text-text">{error}</p>}
          {!lines && !error && <p className="m-0 text-[13px] text-text-2">Asking which versions are available…</p>}
          {lines && (
            <div className="flex flex-col gap-4">
              <Select label="Version" value={line} onChange={(e) => setLine(e.target.value)} options={lines.map((l) => ({ value: l, label: `PHP ${l}` }))} />
              <div className="max-w-[360px]">
                <Switch checked={composer} onChange={setComposer} label="Composer too" description="Downloaded from getcomposer.org and checked" />
              </div>
              <div>
                <Button
                  variant="ink"
                  disabled={busy || !line}
                  onClick={() =>
                    work(async () => {
                      if (!(await run(["php", "install", line], `PHP ${line} is installed.`, onLine))) return;
                      if (composer && !status?.composer) await run(["composer", "install"], "Composer is installed.", onLine);
                    })
                  }
                >
                  {busy ? "Installing…" : `Install PHP ${line}`}
                </Button>
              </div>
            </div>
          )}
        </>
      )}
      {bar}
    </>
  );
}
