import {
  Add16Regular,
  ArrowDownload16Regular,
  ArrowUpload16Regular,
  Delete16Regular,
  MoreHorizontal16Regular,
  Open16Regular,
  Play16Filled,
  Stop16Filled,
  Table16Regular,
} from "@fluentui/react-icons";
import { useState, type ReactNode } from "react";
import { Button } from "../components/Button";
import { Chips } from "../components/Chips";
import { Dialog } from "../components/Dialog";
import { Menu, type MenuItem } from "../components/Menu";
import { Page } from "../components/Page";
import { Detail, Row, RowName, Section } from "../components/Rows";
import { Select } from "../components/Select";
import { Status } from "../components/Status";
import { Switch } from "../components/Switch";
import { TextField } from "../components/TextField";
import { pickSave, pickSql, type Instance, type ServiceKind, type ServiceLogs, type ServicesCatalog, type ServicesList, type SitesList } from "./api";
import { useChanges } from "./Change";
import { mainPort, serviceState } from "./Home";
import { useRead } from "./hooks";
import { useProgress } from "./RuntimePages";
import { Meter } from "../components/Meter";

/** The kinds of service, for grouping and the filter chips. */
const categories: { id: string; label: string; kinds: string[] }[] = [
  { id: "databases", label: "Databases", kinds: ["mysql", "mariadb", "postgres", "mongodb"] },
  { id: "cache", label: "Cache", kinds: ["redis"] },
  { id: "search", label: "Search", kinds: ["meilisearch"] },
  { id: "mail", label: "Mail", kinds: ["mailpit"] },
  { id: "storage", label: "Storage", kinds: ["rustfs"] },
  { id: "realtime", label: "Realtime", kinds: ["reverb"] },
];
export const categoryOf = (kind: string) => categories.find((c) => c.kinds.includes(kind))?.id ?? "other";

/** A database name as the dump and restore commands take it. */
export const databaseName = (text: string) => (/^[A-Za-z0-9_][A-Za-z0-9_$-]{0,63}$/.test(text.trim()) ? text.trim() : null);

const cols = "minmax(180px,1.2fr) 80px minmax(140px,1.3fr) 150px 112px";

type Open =
  | { kind: "add" }
  | { kind: "logs"; instance: Instance }
  | { kind: "ports"; instance: Instance }
  | { kind: "wire"; instance: Instance }
  | { kind: "dump"; instance: Instance }
  | { kind: "restore"; instance: Instance }
  | { kind: "remove"; instance: Instance };

// Every service instance, by kind, and the catalog to add more from (ADR 0056 §4).
export function ServicesPage() {
  const services = useRead<ServicesList>("services.list");
  const sites = useRead<SitesList>("sites.list");
  const { run, open: visit } = useChanges();
  const [category, setCategory] = useState("all");
  const [open, setOpen] = useState<Open | null>(null);
  const instances = services.data?.instances ?? [];
  const hosts = [...(sites.data?.sites ?? []).map((s) => s.host), ...(sites.data?.proxies ?? []).map((p) => p.host)];
  const tool = (name: string) => hosts.find((h) => h.startsWith(`${name}.`));

  const browse = (i: Instance): MenuItem[] => {
    if (!i.databases || i.kind === "mongodb") return [];
    const offers = i.kind === "postgres" ? [["adminer", "Adminer"]] : [["pma", "phpMyAdmin"], ["adminer", "Adminer"]];
    return offers.map(([site, label], n) => {
      const host = tool(site);
      return {
        label: host ? `Browse in ${label}` : `Add ${label} to browse it`,
        icon: <Table16Regular />,
        groupStart: n === 0,
        onSelect: () => (host ? visit(host) : run(["tool", "add", site === "pma" ? "phpmyadmin" : "adminer"], `${label} is set up at ${site}.test.`)),
      };
    });
  };

  const row = (i: Instance) => {
    const state = serviceState(i);
    const running = state === "running";
    return (
      <Row
        key={i.name}
        columns={cols}
        actions={
          <>
            {i.web && <Button variant="quiet" aria-label={`Open ${i.name}'s page`} title={`Open ${i.web}`} icon={<Open16Regular />} onClick={() => visit(i.web!)} />}
            <Button
              variant="quiet"
              aria-label={`${running ? "Stop" : "Start"} ${i.name}`}
              icon={running ? <Stop16Filled /> : <Play16Filled />}
              onClick={() => run(["service", running ? "stop" : "start", i.name], `${running ? "Stopped" : "Started"} ${i.name}.`)}
            />
            <Menu
              label={`More for ${i.name}`}
              trigger={(props) => <Button variant="quiet" aria-label={`More for ${i.name}`} icon={<MoreHorizontal16Regular />} {...props} />}
              items={[
                { label: "Restart", onSelect: () => run(["service", "restart", i.name], `Restarted ${i.name}.`) },
                { label: "View its log", onSelect: () => setOpen({ kind: "logs", instance: i }) },
                { label: "Change a port…", onSelect: () => setOpen({ kind: "ports", instance: i }) },
                ...(i.kind === "reverb" ? [] : [{ label: "Wire a site to it…", onSelect: () => setOpen({ kind: "wire", instance: i }) }]),
                ...(i.databases && i.kind !== "mongodb"
                  ? [
                      { label: "Dump a database…", icon: <ArrowDownload16Regular />, groupStart: true, onSelect: () => setOpen({ kind: "dump", instance: i }) },
                      { label: "Restore a dump…", icon: <ArrowUpload16Regular />, onSelect: () => setOpen({ kind: "restore", instance: i }) },
                    ]
                  : []),
                ...browse(i),
                { label: "Remove…", icon: <Delete16Regular />, danger: true, groupStart: true, onSelect: () => setOpen({ kind: "remove", instance: i }) },
              ]}
            />
          </>
        }
      >
        <RowName sub={i.project ? `runs in ${i.project}` : i.kind}>
          {i.name} <span className="figures text-[12.5px] font-normal text-text-2">{i.version}</span>
        </RowName>
        <Detail mono>{mainPort(i) ? `:${mainPort(i)}` : ""}</Detail>
        <Detail>{i.used_by.length ? `used by ${i.used_by.join(", ")}` : "no site uses it"}</Detail>
        <span className="flex justify-end">
          <Status state={state} label={state === "attention" ? "Not running" : undefined} />
        </span>
      </Row>
    );
  };

  const running = instances.filter((i) => i.running).length;
  const chips = [
    { id: "all", label: "All", count: instances.length },
    ...categories.map((c) => ({ id: c.id, label: c.label, count: instances.filter((i) => c.kinds.includes(i.kind)).length })).filter((c) => c.count > 0),
  ];
  const shown = categories.filter((c) => category === "all" || category === c.id);

  return (
    <Page
      title="Services"
      after={services.data && <span className="figures text-[13px] text-text-3">{`${running} of ${instances.length} running`}</span>}
      actions={
        <Button icon={<Add16Regular />} onClick={() => setOpen({ kind: "add" })}>
          Add a service
        </Button>
      }
    >
      {services.error && <p className="text-[13px] text-bad">{services.error}</p>}
      {services.data && instances.length === 0 && (
        <p className="m-0 max-w-[60ch] text-[13px] text-text-2">No services yet. "Add a service" downloads one, checks it against Kiln's pinned hash, and runs it on this PC only.</p>
      )}
      {instances.length > 0 && (
        <div className="mb-6">
          <Chips label="Kind of service" value={category} onChange={setCategory} options={chips} />
        </div>
      )}
      {shown.map((c) => {
        const list = instances.filter((i) => c.kinds.includes(i.kind));
        return list.length ? (
          <Section key={c.id} title={c.label}>
            {list.map(row)}
          </Section>
        ) : null;
      })}

      {open?.kind === "add" && <AddDialog sites={sites.data} onClose={() => setOpen(null)} />}
      {open?.kind === "logs" && <LogsDialog instance={open.instance} onClose={() => setOpen(null)} />}
      {open?.kind === "ports" && <PortsDialog instance={open.instance} onClose={() => setOpen(null)} />}
      {open?.kind === "wire" && <WireDialog instance={open.instance} sites={sites.data} onClose={() => setOpen(null)} />}
      {open?.kind === "dump" && <DumpDialog instance={open.instance} onClose={() => setOpen(null)} />}
      {open?.kind === "restore" && <RestoreDialog instance={open.instance} onClose={() => setOpen(null)} />}
      {open?.kind === "remove" && <RemoveDialog instance={open.instance} onClose={() => setOpen(null)} />}
    </Page>
  );
}

/** A dialog whose main button runs one job, then closes. */
function JobDialog({
  title,
  action,
  ready,
  work,
  onClose,
  children,
  wide,
}: {
  title: string;
  action: string;
  ready: boolean;
  work: () => Promise<unknown>;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open
      wide={wide}
      onClose={() => !busy && onClose()}
      title={title}
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="ink"
            disabled={!ready || busy}
            onClick={async () => {
              setBusy(true);
              await work();
              setBusy(false);
              onClose();
            }}
          >
            {busy ? "Working…" : action}
          </Button>
        </>
      }
    >
      {children}
    </Dialog>
  );
}

function AddDialog({ sites, onClose }: { sites?: SitesList; onClose: () => void }) {
  const catalog = useRead<ServicesCatalog>("services.catalog");
  const { run } = useChanges();
  const { progress, onLine } = useProgress();
  const [kind, setKind] = useState<ServiceKind | null>(null);
  const [version, setVersion] = useState("");
  const [project, setProject] = useState("");
  const projects = (sites?.sites ?? []).filter((s) => s.driver === "laravel");
  const ready = kind !== null && (kind.runs_in_project ? project !== "" : version !== "");
  return (
    <JobDialog
      title="Add a service"
      action={kind ? `Add ${kind.name}` : "Add"}
      ready={ready}
      wide
      onClose={onClose}
      work={() =>
        run(
          kind!.runs_in_project ? ["service", "add", kind!.kind, "--project", project] : ["service", "add", kind!.kind, "--version", version],
          `${kind!.name} is running.`,
          onLine,
        )
      }
    >
      <p className="mt-0">Kiln downloads it once, checks it against a pinned hash, and runs it on this PC only (127.0.0.1).</p>
      {catalog.error && <p className="text-text">{catalog.error}</p>}
      <div role="radiogroup" aria-label="Service" className="grid max-h-[300px] grid-cols-2 gap-2 overflow-auto">
        {catalog.data?.kinds.map((k) => (
          <button
            key={k.kind}
            type="button"
            role="radio"
            aria-checked={kind?.kind === k.kind}
            disabled={!k.runs_in_project && k.versions.length === 0}
            onClick={() => {
              setKind(k);
              setVersion(k.versions[0] ?? "");
            }}
            className={`rounded-lg border px-3 py-2 text-left disabled:opacity-40 ${kind?.kind === k.kind ? "border-ember bg-ember-soft" : "border-line hover:bg-hover"}`}
          >
            <div className="text-sm font-medium text-text">{k.name}</div>
            <div className="truncate text-xs text-text-3" title={k.license}>
              {k.runs_in_project ? "runs in a Laravel project" : k.versions.join(" · ") || "not for this PC yet"}
            </div>
          </button>
        ))}
      </div>
      {kind && !kind.runs_in_project && (
        <div className="mt-4 flex items-center gap-4">
          <Select label="Version" value={version} onChange={(e) => setVersion(e.target.value)} options={kind.versions.map((v) => ({ value: v, label: v }))} />
          <span className="text-xs text-text-3">{kind.license}</span>
        </div>
      )}
      {kind?.runs_in_project && (
        <div className="mt-4">
          {projects.length ? (
            <Select
              label="In the project"
              value={project}
              onChange={(e) => setProject(e.target.value)}
              options={[{ value: "", label: "Choose a site" }, ...projects.map((s) => ({ value: s.project, label: s.host }))]}
            />
          ) : (
            <p className="m-0">It runs inside a Laravel project: link one first.</p>
          )}
        </div>
      )}
      {progress && (
        <div className="mt-4">
          <Meter label={progress.label} value={progress.done} max={progress.total ?? Math.max(progress.done, 1)} display={`${(progress.done / 1_048_576).toFixed(1)} MB`} />
        </div>
      )}
    </JobDialog>
  );
}

function LogsDialog({ instance, onClose }: { instance: Instance; onClose: () => void }) {
  const logs = useRead<ServiceLogs>("services.logs", { name: instance.name, lines: 300 });
  return (
    <Dialog open wide onClose={onClose} title={`${instance.name}'s log`} footer={<Button onClick={onClose}>Close</Button>}>
      {logs.error && <p className="m-0 text-text">{logs.error}</p>}
      {logs.data && logs.data.files.length === 0 && <p className="m-0">It hasn't written anything yet.</p>}
      {logs.data?.files.map((f) => (
        <div key={f.path} className="mb-3">
          <div className="mb-1 font-mono text-xs text-text-3">{f.path}</div>
          <pre tabIndex={0} className="m-0 max-h-[360px] overflow-auto rounded-lg border border-line p-3 font-mono text-xs leading-[18px] whitespace-pre-wrap break-words text-text">
            {f.lines.join("\n")}
          </pre>
        </div>
      ))}
    </Dialog>
  );
}

function PortsDialog({ instance, onClose }: { instance: Instance; onClose: () => void }) {
  const { run } = useChanges();
  const [ports, setPorts] = useState<Record<string, string>>(Object.fromEntries(Object.entries(instance.ports).map(([r, p]) => [r, String(p)])));
  const valid = Object.values(ports).every((p) => /^\d{1,5}$/.test(p) && Number(p) > 0 && Number(p) < 65536);
  const changed = Object.entries(ports).filter(([role, p]) => String(instance.ports[role]) !== p);
  return (
    <JobDialog
      title={`${instance.name}'s ports`}
      action="Change"
      ready={valid && changed.length > 0}
      onClose={onClose}
      work={() => run(["service", "port", instance.name, ...changed.map(([role, p]) => `${role}=${p}`)], `${instance.name} uses its new ports; .env files wired to it need wiring again.`)}
    >
      <p className="mt-0">Kiln restarts it on the new ports. Sites wired to it keep the old ones until they are wired again.</p>
      <div className="grid grid-cols-2 gap-4">
        {Object.keys(ports).map((role) => (
          <TextField
            key={role}
            label={role}
            mono
            value={ports[role]}
            onChange={(e) => setPorts({ ...ports, [role]: e.target.value })}
            error={/^\d{1,5}$/.test(ports[role]) ? undefined : "A port number."}
          />
        ))}
      </div>
    </JobDialog>
  );
}

function WireDialog({ instance, sites, onClose }: { instance: Instance; sites?: SitesList; onClose: () => void }) {
  const { confirm } = useChanges();
  const [project, setProject] = useState("");
  const [database, setDatabase] = useState("");
  const [redis, setRedis] = useState({ cache: true, session: false, queue: true });
  const list = (sites?.sites ?? []).filter((s) => s.source !== "alias");
  const site = list.find((s) => s.project === project);
  const db = instance.databases && database ? databaseName(database) : null;
  const ready = Boolean(site) && (!database || db !== null);
  return (
    <JobDialog
      title={`Wire a site to ${instance.name}`}
      action="Show the changes"
      ready={ready}
      onClose={onClose}
      work={async () =>
        confirm(
          `Point ${site!.host} at ${instance.name}?`,
          [
            "env",
            "use",
            instance.name,
            "--project",
            site!.project,
            ...(db ? ["--database", db] : []),
            ...(instance.kind === "redis" ? (["cache", "session", "queue"] as const).filter((k) => redis[k]).map((k) => `--${k}`) : []),
          ],
          `${site!.host} uses ${instance.name} now.`,
        )
      }
    >
      <p className="mt-0">Kiln sets the keys in the site's .env, and shows them before changing anything. History can undo it.</p>
      <Select label="Site" value={project} onChange={(e) => setProject(e.target.value)} options={[{ value: "", label: "Choose a site" }, ...list.map((s) => ({ value: s.project, label: s.host }))]} />
      {instance.databases && (
        <TextField
          className="mt-4"
          label="Database"
          placeholder={site ? site.project.split(/[\\/]/).pop() : "the folder's name"}
          value={database}
          onChange={(e) => setDatabase(e.target.value)}
          hint="Made if it doesn't exist. Empty: the project folder's name."
          error={database && !db ? "Letters, digits and underscores." : undefined}
        />
      )}
      {instance.kind === "redis" && (
        <div className="mt-4 flex flex-col gap-3">
          <Switch checked={redis.cache} onChange={(v) => setRedis({ ...redis, cache: v })} label="Laravel's cache" />
          <Switch checked={redis.session} onChange={(v) => setRedis({ ...redis, session: v })} label="Sessions" />
          <Switch checked={redis.queue} onChange={(v) => setRedis({ ...redis, queue: v })} label="Queues" />
        </div>
      )}
    </JobDialog>
  );
}

function DumpDialog({ instance, onClose }: { instance: Instance; onClose: () => void }) {
  const { run } = useChanges();
  const [database, setDatabase] = useState("");
  const db = database ? databaseName(database) : null;
  return (
    <JobDialog
      title={`Dump from ${instance.name}`}
      action="Choose where to save…"
      ready={!database || db !== null}
      onClose={onClose}
      work={async () => {
        const stamp = new Date().toISOString().slice(0, 10);
        const file = await pickSave("Save the dump", `${instance.name}-${db ?? "all"}-${stamp}.sql`);
        if (file) await run(["db", "dump", instance.name, ...(db ? [db] : []), "--output", file], `Wrote ${file}.`);
      }}
    >
      <p className="mt-0">SQL from {instance.name}'s own dump tool. It never overwrites a file.</p>
      <TextField label="Database" placeholder="every database" value={database} onChange={(e) => setDatabase(e.target.value)} error={database && !db ? "Letters, digits and underscores." : undefined} />
    </JobDialog>
  );
}

function RestoreDialog({ instance, onClose }: { instance: Instance; onClose: () => void }) {
  const { run } = useChanges();
  const [database, setDatabase] = useState("");
  const db = database ? databaseName(database) : null;
  return (
    <JobDialog
      title={`Restore into ${instance.name}`}
      action="Choose the .sql file…"
      ready={!database || db !== null}
      onClose={onClose}
      work={async () => {
        const file = await pickSql("Choose a dump to restore");
        if (file) await run(["db", "restore", instance.name, file, ...(db ? [db] : [])], `Restored ${file} into ${instance.name}.`);
      }}
    >
      <p className="mt-0">Loads a .sql dump. A database named here is made if it doesn't exist.</p>
      <TextField label="Into the database" placeholder="as the dump says" value={database} onChange={(e) => setDatabase(e.target.value)} error={database && !db ? "Letters, digits and underscores." : undefined} />
    </JobDialog>
  );
}

function RemoveDialog({ instance, onClose }: { instance: Instance; onClose: () => void }) {
  const { run } = useChanges();
  const [deleteData, setDeleteData] = useState(false);
  return (
    <JobDialog
      title={`Remove ${instance.name}?`}
      action={deleteData ? "Remove it and its data" : "Remove it"}
      ready
      onClose={onClose}
      work={() => run(["service", "remove", instance.name, ...(deleteData ? ["--delete-data"] : [])], `Removed ${instance.name}${deleteData ? " and its data" : "; its data is kept"}.`)}
    >
      <p className="mt-0">
        Kiln stops it and forgets it.{instance.used_by.length ? ` ${instance.used_by.join(", ")} use it.` : ""} Its data stays on disk unless you delete it too.
      </p>
      <Switch checked={deleteData} onChange={setDeleteData} label="Delete its data too" description="This can't be undone: dump a database first if you need it." />
    </JobDialog>
  );
}
