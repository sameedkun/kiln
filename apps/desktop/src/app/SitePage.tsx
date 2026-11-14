import { useState } from "react";
import { ArrowLeft16Regular, Folder16Regular, Open16Regular } from "@fluentui/react-icons";
import { BarChart } from "../components/BarChart";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Chips } from "../components/Chips";
import { Notice } from "../components/Notice";
import { Page } from "../components/Page";
import { Row, RowName, Section } from "../components/Rows";
import { Select } from "../components/Select";
import { Status } from "../components/Status";
import { Switch } from "../components/Switch";
import { Tabs } from "../components/Tabs";
import { Tag } from "../components/Tag";
import { useToast } from "../components/Toast";
import { TextField } from "../components/TextField";
import { call, openFolder, type EnvShow, type GitBranches, type Instance, type LogSources, type RequestsList, type PhpList, type ServicesList, type Site, type SitesList } from "./api";
import { useChanges } from "./Change";
import { LogPanel } from "./LogsPage";
import { useRead } from "./hooks";
import { framework, frameworkId } from "./SitesPage";
import { FrameworkMark } from "../components/FrameworkMark";

const minor = (version: string) => version.split(".").slice(0, 2).join(".");

/** Where a site's PHP comes from, in words. */
function phpWhy(site: Site, defaultVersion: string | null): string {
  const php = site.php;
  if (!php) return "A static site: no PHP.";
  if (!php.version) return "No PHP is installed yet: install one on the PHP page.";
  if (php.pinned && php.fell_back) return `It asks for ${php.pinned}, which isn't installed, so it uses the default.`;
  if (php.pinned && php.from_project) return `Its kiln.toml asks for ${php.pinned}.`;
  if (php.pinned) return `Pinned to ${php.pinned} on this PC.`;
  return `The default${defaultVersion ? ` (${minor(defaultVersion)})` : ""}.`;
}

/** Where a site's Node comes from, in words. */
function nodeWhy(site: Site): string {
  const node = site.node;
  if (!node.version) return node.asks_for ? `${node.asked_by} asks for ${node.asks_for}; Kiln has no Node installed.` : "Kiln has no Node installed.";
  if (node.asked_by && node.fell_back) return `${node.asked_by} asks for ${node.asks_for}, which isn't installed, so the default.`;
  if (node.asked_by) return `${node.asked_by} asks for ${node.asks_for}.`;
  return "The default: no kiln.toml, .nvmrc or .node-version asks for another.";
}

// One site (ADR 0055): how it runs, where it is, and what can change.
export function SitePage({ host, go }: { host: string; go: (page: string) => void }) {
  const sites = useRead<SitesList>("sites.list");
  const php = useRead<PhpList>("php.list");
  // Without Kiln's certificate authority, HTTPS needs setting up first.
  const ca = useRead<{ intermediate_days_left: number | null }>("ca.status");
  const caReady = ca.data ? ca.data.intermediate_days_left !== null : true;
  const { run, ask, confirm, open } = useChanges();
  const [tab, setTab] = useState("overview");
  // The log the Logs tab shows: a file of the app's or one of its processes' (ADR 0069).
  const [logSource, setLogSource] = useState<string | null>(null);
  // Switching PHP starts the other version's workers and reloads the web server: seconds.
  const [switching, setSwitching] = useState(false);
  const site = sites.data?.sites.find((s) => s.host === host);

  const back = <Button variant="quiet" aria-label="Back to Sites" icon={<ArrowLeft16Regular />} onClick={() => go("sites")} />;
  if (sites.data && !site) {
    return (
      <Page title={host} leading={back}>
        <p className="m-0 text-[13px] text-text-2">Kiln doesn't serve {host} any more.</p>
      </Page>
    );
  }
  if (!site) return <Page title={host} leading={back}>{null}</Page>;

  const lines = [...new Set((php.data?.versions ?? []).filter((v) => v.installed).map((v) => minor(v.version)))];
  const current = site.php?.pinned && !site.php.fell_back ? minor(site.php.pinned) : "default";
  const facts: [string, string][] = [
    ["Address", site.url],
    ["Folder", site.project],
    ["Served from", site.document_root],
    ["Found", site.source === "parked" ? `in the parked folder ${site.parked_in}` : site.source === "link" ? "linked on its own" : "an alias in its kiln.toml"],
  ];

  return (
    <Page
      title={site.host}
      leading={back}
      after={
        <>
          <FrameworkMark framework={frameworkId(site)} label={framework(site)} />
          <Tag>{framework(site)}</Tag>
        </>
      }
      actions={
        <>
          <Button variant="quiet" aria-label="Open the folder" title="Open the folder" icon={<Folder16Regular />} onClick={() => openFolder(site.host)} />
          <Button icon={<Open16Regular />} onClick={() => open(site.host)}>
            Open {site.host}
          </Button>
        </>
      }
    >
      {!site.in_hosts && (
        <Notice tone="attention" title={`${site.host} isn't in the hosts file`} actions={<Button variant="ink" onClick={() => confirm("Update the hosts file?", ["hosts", "sync"], "The hosts file has every site now.")}>Add it</Button>}>
          Browsers can't find it by name until it is. Windows asks for administrator approval.
        </Notice>
      )}

      <Tabs
        tabs={[
          { id: "overview", label: "Overview" },
          { id: "requests", label: "Requests" },
          { id: "logs", label: "Logs" },
          { id: "dumps", label: "Dumps" },
          { id: "env", label: ".env" },
        ]}
        current={tab}
        onSelect={setTab}
      >
      {tab === "env" ? (
        <SiteEnv site={site} />
      ) : tab === "requests" ? (
        <SiteRequestsTab host={site.host} />
      ) : tab === "logs" ? (
        <SiteLog site={site} first={logSource} onChoose={setLogSource} />
      ) : tab === "dumps" ? (
        <SiteDumps site={site} />
      ) : (
      <>
      <SiteGlance site={site} />
      <SiteGit site={site} go={go} />
      <SiteShare site={site} />
      <Section title="How it runs">
        <li className="flex flex-wrap items-start gap-x-10 gap-y-4 border-b border-line px-1 py-4">
          {site.php ? (
            <div>
              <Select
                label="PHP"
                value={current}
                disabled={switching || site.source === "alias" || lines.length === 0}
                onChange={async (e) => {
                  const v = e.target.value;
                  setSwitching(true);
                  if (v === "default") await run(["unisolate", site.host], `${site.host} uses the default PHP now.`);
                  else await run(["isolate", v, site.host], `${site.host} uses PHP ${v} now.`);
                  setSwitching(false);
                }}
                options={[{ value: "default", label: "Default" }, ...lines.map((l) => ({ value: l, label: `PHP ${l}` }))]}
              />
              <p className="m-0 mt-1.5 max-w-[34ch] text-xs text-text-3" aria-live="polite">
                {switching ? (
                  "Switching: starting that version's workers…"
                ) : (
                  <>
                    {site.php.version ? `Runs ${site.php.version}. ` : ""}
                    {phpWhy(site, php.data?.default ?? null)}
                    {site.source === "alias" ? " An alias follows its main site." : ""}
                  </>
                )}
              </p>
            </div>
          ) : (
            <p className="m-0 text-[13px] text-text-2">A static site: no PHP.</p>
          )}
          <div>
            <div className="text-[13px] text-text-2">Node</div>
            <div className="figures mt-1 text-sm font-medium">{site.node.version ?? "none"}</div>
            <p className="m-0 mt-1 max-w-[40ch] text-xs text-text-3">{nodeWhy(site)}</p>
          </div>
          <div className="w-[260px]">
            <Switch
              checked={site.secure}
              disabled={!caReady && !site.secure}
              onChange={(on) => run([on ? "secure" : "unsecure", site.host], `${site.host} is served over ${on ? "HTTPS" : "HTTP"} now.`)}
              label="HTTPS"
              description={caReady ? "With Kiln's own certificate" : "Needs Kiln's certificate authority, set up once for every site"}
            />
            {!caReady && (
              <Button
                size="sm"
                className="mt-2"
                onClick={async () => {
                  if (await run(["ca", "init"], "Kiln's certificate authority is set up.")) {
                    confirm("Trust Kiln's certificate authority?", ["ca", "trust"], "Windows trusts Kiln's certificates now: turn HTTPS on.");
                  }
                }}
              >
                Set up HTTPS
              </Button>
            )}
          </div>
        </li>
      </Section>

      <Section title="Where it is">
        {facts.map(([label, value]) => (
          <Row key={label} columns="140px 1fr">
            <span className="text-[13px] text-text-2">{label}</span>
            <span className="font-mono text-xs break-all">{value}</span>
          </Row>
        ))}
      </Section>

      <SiteServices site={site} go={go} />

      <SiteProcesses
        site={site}
        onLog={(source) => {
          setLogSource(source);
          setTab("logs");
        }}
      />

      {site.source === "link" && (
        <Button
          onClick={() =>
            ask(`Unlink ${site.host}?`, `Kiln stops serving it. Its folder (${site.project}) stays as it is, and History can undo this.`, "Unlink", ["unlink", site.host], `Unlinked ${site.host}.`)
          }
        >
          Unlink {site.host}
        </Button>
      )}
      {site.source === "parked" && <p className="m-0 px-1 text-[13px] text-text-2">It's served because its folder is in {site.parked_in}. Moving it out, or unparking that folder, stops it.</p>}
      </>
      )}
      </Tabs>
    </Page>
  );
}

/** The services this site's .env points at (ADR 0056 §5). */
function SiteServices({ site, go }: { site: Site; go: (page: string) => void }) {
  const services = useRead<ServicesList>("services.list");
  const used = (services.data?.instances ?? []).filter((i) => i.used_by.includes(site.host));
  return (
    <Section
      title="Services"
      meta="from its .env"
      action={
        <Button variant="quiet" size="sm" onClick={() => go("services")}>
          Wire one on the Services page
        </Button>
      }
    >
      {services.data && used.length === 0 && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">Its .env doesn't point at any of Kiln's services.</li>}
      {used.map((i) => (
        <Row key={i.name} columns="minmax(160px,1fr) 1fr 140px">
          <span className="text-sm font-semibold">{i.name}</span>
          <span className="figures text-[12.5px] text-text-2">
            {i.kind} {i.version}
          </span>
          <span className="text-right text-[12.5px]">{i.running ? "Running" : i.enabled ? "Not running" : "Stopped"}</span>
        </Row>
      ))}
    </Section>
  );
}

/** A project's development processes, by kind (ADR 0060). */
export const processes: { kind: string; label: string; what: string; laravel: boolean }[] = [
  { kind: "queue", label: "Queue worker", what: "Runs queued jobs (php artisan queue:listen), picking up code changes.", laravel: true },
  { kind: "scheduler", label: "Scheduler", what: "Runs the schedule every minute (php artisan schedule:work).", laravel: true },
  { kind: "vite", label: "Vite", what: "Serves scripts and styles with hot reload, from the project's node_modules.", laravel: false },
  { kind: "dumps", label: "Dumps", what: "Collects dump() output here instead of in the page (its .env sends it), shown in the Dumps tab.", laravel: true },
  { kind: "stripe", label: "Stripe webhooks", what: "Forwards Stripe's test events to the site's /stripe/webhook (stripe listen), with the STRIPE_SECRET in its .env. Its signing secret is in its log.", laravel: true },
];

/** The queue worker, scheduler and Vite for this site's project: started here, kept up by Kiln. */
/** The most queue workers a site runs (ADR 0069 §2). */
const MOST_WORKERS = 8;

/**
 * A site's process instances in the order of `processes`, each with the name the page shows:
 * "Queue worker", "Queue worker 2", … (ADR 0069 §1).
 */
export function siteInstances(instances: Instance[], project: string): { instance: Instance; label: string }[] {
  return processes.flatMap((p) =>
    instances
      .filter((i) => i.kind === p.kind && i.project === project)
      .sort((a, b) => a.name.length - b.name.length || a.name.localeCompare(b.name, undefined, { numeric: true }))
      .map((instance, n) => ({ instance, label: n === 0 ? p.label : `${p.label} ${n + 1}` })),
  );
}

/** The name for one more queue worker: the first one's name and the next free number. */
export function nextWorkerName(workers: Instance[]): string | null {
  const base = workers[0]?.name;
  if (!base) return null;
  for (let n = 2; n <= MOST_WORKERS; n++) {
    if (!workers.some((w) => w.name === `${base}-${n}`)) return `${base}-${n}`;
  }
  return null;
}

function SiteProcesses({ site, onLog }: { site: Site; onLog: (source: string) => void }) {
  const services = useRead<ServicesList>("services.list");
  const { run, ask, confirm } = useChanges();
  if (site.source === "alias") return null;
  const shown = processes.filter((p) => !p.laravel || site.driver === "laravel");
  return (
    <Section title="Dev processes" meta="kept running by Kiln">
      {shown.map((p) => {
        const all = siteInstances(services.data?.instances ?? [], site.project)
          .filter((x) => x.instance.kind === p.kind)
          .map((x) => x.instance);
        const instance = all[0];
        const port = instance && Object.values(instance.ports)[0];
        const lower = p.label.toLowerCase();
        const toggle = instance
          ? async () => {
              // Start and Stop act on every worker (ADR 0069 §3).
              const verb = instance.enabled ? "stop" : "start";
              for (const i of all) {
                if (i.enabled === instance.enabled) await run(["service", verb, i.name], `${instance.enabled ? "Stopped" : "Started"} ${i.name}.`);
              }
            }
          : async () => {
              if (!(await run(["service", "add", p.kind, "--project", site.project], `The ${lower} runs for ${site.host} now.`))) return;
              // A process with .env keys (the dump server) is wired through its preview.
              if (p.kind !== "dumps") return;
              const now = await call<ServicesList>("services.list").catch(() => null);
              const added = now?.instances.find((i) => i.kind === p.kind && i.project === site.project);
              if (added) confirm(`Send ${site.host}'s dumps to Kiln?`, ["env", "use", added.name], `${site.host}'s dump() output shows in its Dumps tab now.`);
            };
        const next = p.kind === "queue" ? nextWorkerName(all) : null;
        const last = all.length > 1 ? all[all.length - 1] : null;
        return (
          <Row
            key={p.kind}
            columns="minmax(130px,1fr) minmax(200px,2.2fr) 190px"
            actions={
              instance && (
                <>
                  <Button variant="quiet" size="sm" onClick={() => onLog(`service:${instance.name}`)}>
                    Log
                  </Button>
                  {/* With several workers, "−" takes them away one at a time first. */}
                  {all.length === 1 && (
                    <Button
                      variant="quiet"
                      size="sm"
                      onClick={() => ask(`Remove ${instance.name}?`, `Kiln stops running the ${lower} for ${site.host}. The project stays as it is.`, "Remove", ["service", "remove", instance.name], `Removed ${instance.name}.`)}
                    >
                      Remove
                    </Button>
                  )}
                </>
              )
            }
          >
            <span className="text-sm font-semibold">{p.label}</span>
            <span className="text-[12.5px] text-text-2">
              {p.what}
              {port ? <span className="figures"> On 127.0.0.1:{port}.</span> : null}
              {p.kind === "queue" && instance && (
                <span className="mt-1.5 flex items-center gap-2">
                  <span className="figures text-text">
                    {all.length} worker{all.length === 1 ? "" : "s"}
                  </span>
                  <Button
                    size="sm"
                    variant="quiet"
                    aria-label="One worker fewer"
                    disabled={!last}
                    onClick={() => last && run(["service", "remove", last.name], `${site.host} runs ${all.length - 1} queue worker${all.length - 1 === 1 ? "" : "s"} now.`)}
                  >
                    −
                  </Button>
                  <Button
                    size="sm"
                    variant="quiet"
                    aria-label="One more worker"
                    disabled={!next}
                    onClick={() => next && run(["service", "add", "queue", "--project", site.project, "--name", next], `${site.host} runs ${all.length + 1} queue workers now.`)}
                  >
                    +
                  </Button>
                </span>
              )}
            </span>
            <span className="flex items-center justify-end gap-3">
              {instance ? (
                <Status state={instance.running ? "running" : instance.enabled ? "attention" : "stopped"} label={instance.enabled && !instance.running ? "Not running" : undefined} />
              ) : (
                <span className="text-[12.5px] text-text-3">Off</span>
              )}
              {services.data && (
                <Button size="sm" aria-label={`${instance?.enabled ? "Stop" : "Start"} the ${lower}`} onClick={toggle}>
                  {instance?.enabled ? "Stop" : "Start"}
                </Button>
              )}
            </span>
          </Row>
        );
      })}
    </Section>
  );
}

/** The hour labels under the day's chart: every sixth hour, as a clock time. */
export function hourLabels(now: Date): string[] {
  return Array.from({ length: 24 }, (_, i) => {
    const at = new Date(now.getTime() - (23 - i) * 3_600_000);
    return i % 6 === 5 ? `${String(at.getHours()).padStart(2, "0")}:00` : "";
  });
}

/**
 * The site at a glance (P4C-05): its last day of requests by hour, how they answered, its
 * timing, and what it runs on.
 */
function SiteGlance({ site }: { site: Site }) {
  const requests = useRead<RequestsList>("requests.get", { host: site.host });
  const services = useRead<ServicesList>("services.list");
  const r = requests.data?.sites.find((s) => s.host === site.host);
  const used = (services.data?.instances ?? []).filter((i) => i.used_by.includes(site.host));
  const procs = siteInstances(services.data?.instances ?? [], site.project);
  const running = procs.filter((p) => p.instance.running).length;
  const figure = (label: string, value: string, bad = false) => (
    <div className="min-w-[110px]">
      <div className="text-xs text-text-3">{label}</div>
      <div className={`figures mt-1 text-xl font-semibold ${bad ? "text-bad" : ""}`}>{value}</div>
    </div>
  );
  const day = r?.hours?.reduce((n, h) => n + h, 0) ?? 0;
  const labels = hourLabels(new Date());
  const statuses = r?.statuses;
  return (
    <Section title="At a glance" meta={r ? "the last 24 hours, from the web server's log" : undefined}>
      <li className="flex flex-wrap gap-x-10 gap-y-4 border-b border-line px-1 py-4">
        {figure("Requests today", r ? String(day) : "—")}
        {figure("Median", r ? `${r.median_ms} ms` : "—")}
        {figure("Slowest 5% over", r ? `${r.p95_ms} ms` : "—")}
        {figure("Failed (5xx)", r ? String(r.failures) : "—", (r?.failures ?? 0) > 0)}
        {figure("Branch", site.git_branch ?? "—")}
        {figure("Services", used.length ? String(used.length) : "none")}
        {figure("Processes", procs.length ? `${running} of ${procs.length} running` : "none")}
      </li>
      {r?.hours && day > 0 && (
        <li className="border-b border-line px-1 py-4">
          <BarChart
            label="Requests by hour"
            bars={r.hours.map((value, i) => ({
              label: labels[i],
              title: `${i === 23 ? "This hour" : `${23 - i} h ago`}: ${value} request${value === 1 ? "" : "s"}${r.failing_hours?.[i] ? `, ${r.failing_hours[i]} failed` : ""}`,
              value,
              tone: (r.failing_hours?.[i] ?? 0) > 0 ? ("bad" as const) : undefined,
            }))}
          />
          {statuses && (
            <p className="figures m-0 mt-3 text-xs text-text-2">
              {(["2xx", "3xx", "4xx", "5xx"] as const)
                .filter((k) => statuses[k] > 0)
                .map((k) => `${statuses[k]} × ${k}`)
                .join(" · ")}
            </p>
          )}
        </li>
      )}
      {requests.data && !r && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">No requests to {site.host} recorded yet: open it and its figures show here.</li>}
      {used.length > 0 && (
        <li className="border-b border-line px-1 py-3 text-[12.5px] text-text-2">
          Uses {used.map((i) => `${i.name} (${i.kind})`).join(", ")}.
        </li>
      )}
    </Section>
  );
}

/** A site's Git (ADR 0075): its branch to switch, never over uncommitted changes, and its worktrees served as sites. */
function SiteGit({ site, go }: { site: Site; go: (page: string) => void }) {
  const git = useRead<GitBranches>("git.branches", { host: site.host });
  const { run, ask, open } = useChanges();
  const [branch, setBranch] = useState("");
  const [adding, setAdding] = useState(false);
  if (git.error) {
    return (
      <Section title="Git">
        <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">{/not a git repository/i.test(git.error) ? "Not a Git repository." : git.error}</li>
      </Section>
    );
  }
  if (!git.data) return null;
  const g = git.data;
  const target = branch || g.current;
  return (
    <Section
      title="Git"
      meta={g.dirty ? "uncommitted changes" : undefined}
      action={
        <Button variant="quiet" size="sm" onClick={() => setAdding(true)}>
          New worktree…
        </Button>
      }
    >
      <li className="flex flex-wrap items-end gap-3 border-b border-line px-1 py-4">
        <Select label="Branch" value={target} onChange={(e) => setBranch(e.target.value)} options={g.branches.map((b) => ({ value: b, label: b === g.current ? `${b} (current)` : b }))} />
        <Button disabled={target === g.current || g.dirty} onClick={async () => (await run(["git", "switch", site.host, target], `${site.host} is on ${target} now.`)) && setBranch("")}>
          Switch
        </Button>
        {g.dirty && <span className="pb-2 text-xs text-text-3">Commit or stash its changes to switch: Kiln never switches over them.</span>}
      </li>
      {g.worktrees.map((w) => (
        <Row
          key={w.path}
          columns="minmax(140px,1fr) minmax(160px,1.6fr) auto"
          actions={
            w.branch && (
              <Button
                variant="quiet"
                size="sm"
                onClick={() =>
                  ask(
                    `Remove the worktree of ${w.branch}?`,
                    `Kiln stops serving it, then Git removes ${w.path}; Git refuses if it has uncommitted changes. The branch stays.`,
                    "Remove",
                    ["git", "worktree", "remove", site.host, w.branch!],
                    `Removed the worktree of ${w.branch}.`,
                  )
                }
              >
                Remove
              </Button>
            )
          }
        >
          <RowName sub={w.path} mono>
            {w.branch ?? "detached"}
          </RowName>
          <span className="text-[12.5px] text-text-2">
            {w.host ? (
              <button type="button" className="hover:underline" onClick={() => go(`site:${w.host}`)}>
                {w.host}
              </button>
            ) : (
              "not served"
            )}
          </span>
          <span>{w.host && <Button variant="quiet" size="sm" aria-label={`Open ${w.host}`} icon={<Open16Regular />} onClick={() => open(w.host!)} />}</span>
        </Row>
      ))}
      {adding && <WorktreeDialog site={site} branches={g.branches} onClose={() => setAdding(false)} />}
    </Section>
  );
}

/** A branch's worktree beside the project, served as <branch>.<site> (ADR 0075). */
function WorktreeDialog({ site, branches, onClose }: { site: Site; branches: string[]; onClose: () => void }) {
  const { run } = useChanges();
  const [branch, setBranch] = useState("");
  const [busy, setBusy] = useState(false);
  const name = branch.trim();
  const valid = name !== "" && !name.startsWith("-") && !/\s|\.\.|[~^:?*[\\]/.test(name);
  const isNew = valid && !branches.includes(name);
  return (
    <Dialog
      open
      onClose={() => !busy && onClose()}
      title="New worktree"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="ink"
            disabled={!valid || busy}
            onClick={async () => {
              setBusy(true);
              await run(["git", "worktree", "add", site.host, name, ...(isNew ? ["--new"] : [])], `The worktree of ${name} is served.`);
              onClose();
            }}
          >
            {busy ? "Making…" : "Make it"}
          </Button>
        </>
      }
    >
      <p className="mt-0">A second checkout of a branch, in a folder beside {site.host}'s, served as a site of its own: work on two branches at once.</p>
      <TextField label="Branch" list="kiln-branches" placeholder="feature/checkout" value={branch} onChange={(e) => setBranch(e.target.value)} error={branch && !valid ? "Not a branch name Git takes." : undefined} />
      <datalist id="kiln-branches">
        {branches.map((b) => (
          <option key={b} value={b} />
        ))}
      </datalist>
      <p className="mb-0 mt-2 text-xs text-text-3">{isNew ? `A new branch, from the one ${site.host} is on.` : valid ? "An existing branch." : ""}</p>
    </Dialog>
  );
}

/** Sharing a site through a Cloudflare quick tunnel (ADR 0076): asked for in words, its public address shown while it runs. */
function SiteShare({ site }: { site: Site }) {
  const services = useRead<ServicesList>("services.list");
  const { run, ask } = useChanges();
  const toast = useToast();
  const tunnel = services.data?.instances.find((i) => i.kind === "tunnel" && i.project === site.project);
  if (site.source === "alias" || !services.data) return null;
  return (
    <Section title="Sharing" meta={tunnel ? "anyone with the address can open it" : "only this PC can open it"}>
      <li className="flex flex-wrap items-center gap-3 border-b border-line px-1 py-4">
        {tunnel ? (
          <>
            <span className="min-w-0 flex-1 truncate font-mono text-xs" title={tunnel.shared_url ?? undefined}>
              {tunnel.shared_url ?? "Starting the tunnel…"}
            </span>
            {tunnel.shared_url && (
              <Button
                size="sm"
                onClick={() =>
                  navigator.clipboard
                    .writeText(tunnel.shared_url!)
                    .then(() => toast({ message: "Copied the address." }))
                    .catch(() => toast({ message: "Couldn't copy it." }))
                }
              >
                Copy
              </Button>
            )}
            <Button size="sm" onClick={() => run(["share", site.host, "--stop"], `${site.host} isn't shared any more.`)}>
              Stop sharing
            </Button>
          </>
        ) : (
          <>
            <span className="flex-1 text-[13px] text-text-2">A public https address that reaches {site.host} while it's shared, through a Cloudflare quick tunnel. No account needed.</span>
            <Button
              size="sm"
              onClick={() =>
                ask(
                  `Share ${site.host}?`,
                  `Kiln starts a Cloudflare quick tunnel: a random https://….trycloudflare.com address that reaches ${site.host} from anywhere while it's shared. Anyone with the address can open it, so share it only with people you trust, and stop it when you're done. Kiln downloads cloudflared (about 55 MB) the first time.`,
                  "Share it",
                  ["share", site.host],
                  `${site.host} is shared: its address is under Sharing.`,
                )
              }
            >
              Share…
            </Button>
          </>
        )}
      </li>
    </Section>
  );
}

/** How fast the site answers, from the web server's access log (ADR 0065). */
function SiteRequestsTab({ host }: { host: string }) {
  const requests = useRead<RequestsList>("requests.get", { host });
  const r = requests.data?.sites.find((s) => s.host === host);
  const figure = (label: string, value: string, bad = false) => (
    <div className="min-w-[120px]">
      <div className="text-xs text-text-3">{label}</div>
      <div className={`figures mt-1 text-xl font-semibold ${bad ? "text-bad" : ""}`}>{value}</div>
    </div>
  );
  return (
    <Section
      title="Requests"
      meta={r ? `since ${new Date(r.since * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" })}` : undefined}
      action={
        <Button variant="quiet" size="sm" onClick={requests.reload}>
          Read again
        </Button>
      }
    >
      {requests.error && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">{requests.error}</li>}
      {requests.data && !r && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">No requests to {host} recorded yet. Open it, then read again.</li>}
      {r && (
        <>
          <li className="flex flex-wrap gap-x-10 gap-y-4 border-b border-line px-1 py-4">
            {figure("Requests", String(r.count))}
            {figure("Failed (5xx)", String(r.failures), r.failures > 0)}
            {figure("Median", `${r.median_ms} ms`)}
            {figure("Slowest 5% over", `${r.p95_ms} ms`)}
          </li>
          <li className="px-1 pb-1 pt-4 text-xs font-medium text-text-3">The slowest</li>
          {r.slowest.map((q, i) => (
            <Row key={`${q.at}-${i}`} columns="90px 56px minmax(160px,1fr) 70px">
              <span className="figures text-right text-[12.5px] font-medium">{q.ms} ms</span>
              <span className={`figures text-[12.5px] ${q.status >= 500 ? "text-bad font-medium" : "text-text-2"}`}>{q.status}</span>
              <span className="truncate font-mono text-xs" title={`${q.method} ${q.uri}`}>
                {q.method} {q.uri}
              </span>
              <span className="figures text-right text-xs text-text-3">{new Date(q.at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
            </Row>
          ))}
        </>
      )}
    </Section>
  );
}

/** What the site's dump() calls sent to its dump server (ADR 0062). */
function SiteDumps({ site }: { site: Site }) {
  const services = useRead<ServicesList>("services.list");
  if (!services.data) return null;
  const dumps = services.data.instances.find((i) => i.kind === "dumps" && i.project === site.project);
  if (!dumps) {
    return (
      <p className="m-0 px-1 text-[13px] text-text-2">
        Start Dumps under Dev processes on the Overview tab: dump() output from {site.host} then shows here, with the file and line it came from, instead of in the page.
      </p>
    );
  }
  return <LogPanel source={`service:${dumps.name}`} label={`${site.host}'s dumps`} />;
}

/**
 * The site's logs (ADR 0069 §1): its app's files in storage/logs (newest laravel*.log first,
 * ADR 0059 §4), then each of its processes' logs.
 */
function SiteLog({ site, first, onChoose }: { site: Site; first: string | null; onChoose: (source: string) => void }) {
  const sources = useRead<LogSources>("logs.sources");
  const services = useRead<ServicesList>("services.list");
  const main = `site:${site.host}`;
  const all = sources.data?.sources ?? [];
  // The site's own files: "site:<host>" and "site:<host>/<file>".
  const files = all.filter((s) => s.id === main || s.id.startsWith(`${main}/`));
  const procs = siteInstances(services.data?.instances ?? [], site.project).flatMap((p) => {
    const source = all.find((s) => s.id === `service:${p.instance.name}` || s.id.startsWith(`service:${p.instance.name}/`));
    return source ? [{ label: p.label, source }] : [];
  });
  const chosen = first ?? files[0]?.id ?? procs[0]?.source.id ?? null;
  const isApp = chosen === null || chosen === main || chosen.startsWith(`${main}/`);
  const log = isApp ? (files.find((s) => s.id === chosen) ?? files[0]) : all.find((s) => s.id === chosen);
  if (!sources.data) return null;
  const choices = [...(files.length ? [{ id: "app", label: "App" }] : []), ...procs.map((p) => ({ id: p.source.id, label: p.label }))];
  return (
    <div>
      {choices.length > 1 && (
        <div className="mb-3">
          <Chips label="Which log" value={isApp ? "app" : (chosen ?? "app")} onChange={(id) => onChoose(id === "app" ? (files[0]?.id ?? main) : id)} options={choices} />
        </div>
      )}
      {isApp && files.length > 1 && (
        <div className="mb-3 max-w-[320px]">
          <Select
            label="File in storage/logs"
            value={log?.id ?? main}
            onChange={(e) => onChoose(e.target.value)}
            options={files.map((f) => ({ value: f.id, label: f.id === main ? `${f.path.split(/[\\/]/).pop()} (main)` : f.label }))}
          />
        </div>
      )}
      {log ? (
        <LogPanel key={log.id} source={log.id} label={log.label} />
      ) : (
        <p className="m-0 px-1 text-[13px] text-text-2">
          {site.host} has no log yet: Kiln shows the newest file in its storage/logs once the site writes one, and its processes' logs once they run. Kiln's own logs (the web server, PHP's errors, services) are on the Logs page.
        </p>
      )}
    </div>
  );
}

/** The site's .env: secrets hidden until shown; Kiln's own keys tagged (ADR 0056 §5). */
function SiteEnv({ site }: { site: Site }) {
  const env = useRead<EnvShow>("env.show", { project: site.project });
  const [shown, setShown] = useState<string[]>([]);
  return (
    <Section title=".env" meta={env.data?.path}>
      {env.error && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">{env.error}</li>}
      {env.data?.keys.map((k) => {
        const hidden = k.secret && k.value !== "" && !shown.includes(k.key);
        return (
          <Row key={k.key} columns="minmax(160px,220px) minmax(160px,1fr) 100px">
            <span className="font-mono text-xs font-medium">{k.key}</span>
            <span className="truncate font-mono text-xs text-text-2" title={hidden ? undefined : k.value}>
              {hidden ? "••••••••" : k.value || "(empty)"}
              {k.secret && k.value !== "" && (
                <button
                  type="button"
                  className="ml-2 font-sans text-xs text-text hover:underline"
                  onClick={() => setShown((s) => (s.includes(k.key) ? s.filter((x) => x !== k.key) : [...s, k.key]))}
                >
                  {hidden ? "Show" : "Hide"}
                </button>
              )}
            </span>
            <span className="text-right">{k.kiln && <Tag>set by Kiln</Tag>}</span>
          </Row>
        );
      })}
    </Section>
  );
}
