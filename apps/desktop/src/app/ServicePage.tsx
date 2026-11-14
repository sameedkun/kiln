import { ArrowDownload16Regular, ArrowLeft16Regular, Delete16Regular, Open16Regular, Play16Filled, Stop16Filled } from "@fluentui/react-icons";
import { useState } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Page } from "../components/Page";
import { Detail, Row, RowName, Section } from "../components/Rows";
import { Select } from "../components/Select";
import { Status } from "../components/Status";
import { Tabs } from "../components/Tabs";
import { Tag } from "../components/Tag";
import { TextField } from "../components/TextField";
import { type Instance, type LogSources, type ServiceDatabases, type ServicesList, type SitesList } from "./api";
import { useChanges } from "./Change";
import { mainPort, serviceState } from "./Home";
import { fewNames, useRead } from "./hooks";
import { LogPanel } from "./LogsPage";
import { databaseName, DumpDialog, JobDialog, PortsDialog, RemoveDialog, RestoreDialog, WireDialog } from "./ServicesPage";

/** Bytes as people read them: "0 B", "46.0 MB". */
export function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let n = bytes / 1024;
  let unit = 0;
  while (n >= 1024 && unit < units.length - 1) {
    n /= 1024;
    unit += 1;
  }
  return `${n.toFixed(1)} ${units[unit]}`;
}

type Open = "ports" | "wire" | "dump" | "restore" | "remove" | "pgadmin" | { drop: string } | { dumpOne: string };

// One service (ADR 0068): what it is, its databases, logs, .env values and tools.
export function ServicePage({ name, go }: { name: string; go: (page: string) => void }) {
  const services = useRead<ServicesList>("services.list");
  const sites = useRead<SitesList>("sites.list");
  const { run, open: visit } = useChanges();
  const [tab, setTab] = useState("overview");
  const [open, setOpen] = useState<Open | null>(null);
  // Bumped after a database is deleted, so the list is read again.
  const [dropped, setDropped] = useState(0);
  const i = services.data?.instances.find((x) => x.name === name);

  const back = <Button variant="quiet" aria-label="Back to Services" icon={<ArrowLeft16Regular />} onClick={() => go("services")} />;
  if (services.data && !i) {
    return (
      <Page title={name} leading={back}>
        <p className="m-0 text-[13px] text-text-2">Kiln has no service called {name} any more.</p>
      </Page>
    );
  }
  if (!i) return <Page title={name} leading={back}>{null}</Page>;

  const state = serviceState(i);
  const running = state === "running";
  const hasDatabases = i.databases && i.kind !== "mongodb";
  const port = mainPort(i);
  const tabs = [
    { id: "overview", label: "Overview" },
    ...(hasDatabases ? [{ id: "databases", label: "Databases" }] : []),
    { id: "logs", label: "Logs" },
    ...(i.env.length ? [{ id: "env", label: ".env" }] : []),
    { id: "tools", label: "Tools" },
  ];

  return (
    <Page
      title={i.name}
      leading={back}
      after={
        <>
          <span className="figures text-[13px] text-text-3">{i.version}</span>
          {port && <Tag>{`:${port}`}</Tag>}
          <Status state={state} label={state === "attention" ? "Not running" : undefined} />
        </>
      }
      actions={
        <>
          {i.web && (
            <Button icon={<Open16Regular />} onClick={() => visit(i.web!)}>
              Open {i.web}
            </Button>
          )}
          <Button
            icon={running ? <Stop16Filled /> : <Play16Filled />}
            onClick={() => run(["service", running ? "stop" : "start", i.name], `${running ? "Stopped" : "Started"} ${i.name}.`)}
          >
            {running ? "Stop" : "Start"}
          </Button>
        </>
      }
    >
      <Tabs tabs={tabs} current={tabs.some((t) => t.id === tab) ? tab : "overview"} onSelect={setTab}>
        {tab === "databases" && hasDatabases ? (
          <ServiceDatabasesTab key={dropped} instance={i} running={running} go={go} onDrop={(db) => setOpen({ drop: db })} onDump={(db) => setOpen({ dumpOne: db })} />
        ) : tab === "logs" ? (
          <ServiceLog name={i.name} />
        ) : tab === "env" ? (
          <ServiceEnv instance={i} />
        ) : tab === "tools" ? (
          <ServiceTools instance={i} hosts={(sites.data?.sites ?? []).map((s) => s.host)} pgadmin={services.data?.instances.find((x) => x.kind === "pgadmin")} onOpen={setOpen} />
        ) : (
          <ServiceOverview instance={i} go={go} />
        )}
      </Tabs>

      {open === "ports" && <PortsDialog instance={i} onClose={() => setOpen(null)} />}
      {open === "wire" && <WireDialog instance={i} sites={sites.data} onClose={() => setOpen(null)} />}
      {open === "dump" && <DumpDialog instance={i} onClose={() => setOpen(null)} />}
      {open === "restore" && <RestoreDialog instance={i} onClose={() => setOpen(null)} />}
      {open === "remove" && <RemoveDialog instance={i} onClose={() => setOpen(null)} />}
      {open === "pgadmin" && (
        <JobDialog
          title="Add pgAdmin?"
          action="Add pgAdmin"
          ready
          onClose={() => setOpen(null)}
          work={() => run(["service", "add", "pgadmin"], "pgAdmin is running at pgadmin.test.")}
        >
          <p className="mt-0">
            pgAdmin 4 comes in the same PostgreSQL download Kiln already checks, so a PC with PostgreSQL 18 has it downloaded. It unpacks to about 340 MB, without its desktop shell.
          </p>
          <p className="mb-0">
            It runs on this PC only (127.0.0.1) and is served at pgadmin.test, with Kiln's PostgreSQL instances listed. Its settings stay in Kiln's folder, apart from any pgAdmin you have, and it doesn't check for updates.
          </p>
        </JobDialog>
      )}
      {typeof open === "object" && open && "dumpOne" in open && <DumpDialog instance={i} database={open.dumpOne} onClose={() => setOpen(null)} />}
      {typeof open === "object" && open && "drop" in open && <DropDialog instance={i} database={open.drop} onDump={() => setOpen({ dumpOne: open.drop })} onClose={() => { setOpen(null); setDropped((n) => n + 1); }} />}
    </Page>
  );
}

function ServiceOverview({ instance: i, go }: { instance: Instance; go: (page: string) => void }) {
  const facts: [string, string][] = [
    ["Kind", i.kind],
    ["Version", i.version],
    ...Object.entries(i.ports).map(([role, port]): [string, string] => [`Port (${role})`, `127.0.0.1:${port}`]),
    ...(i.project ? [["Runs in", i.project] as [string, string]] : []),
    ["Starts", i.enabled ? "with Kiln's services" : "only when started"],
  ];
  return (
    <>
      <Section title="About it">
        {facts.map(([label, value]) => (
          <Row key={label} columns="180px minmax(0,1fr)">
            <span className="text-[13px] text-text-2">{label}</span>
            <span className="truncate font-mono text-xs" title={value}>
              {value}
            </span>
          </Row>
        ))}
      </Section>
      <Section title="Sites that use it" meta={i.used_by.length ? fewNames(i.used_by) : undefined}>
        {i.used_by.length === 0 && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">{i.project ? "It runs for its project." : "No site's .env points at it yet. Tools has \"Wire a site to it\"."}</li>}
        {i.used_by.map((host) => (
          <Row key={host} columns="minmax(0,1fr)">
            <RowName>
              <button type="button" className="hover:underline" onClick={() => go(`site:${host}`)}>
                {host}
              </button>
            </RowName>
          </Row>
        ))}
      </Section>
    </>
  );
}

function ServiceDatabasesTab({
  instance: i,
  running,
  go,
  onDrop,
  onDump,
}: {
  instance: Instance;
  running: boolean;
  go: (page: string) => void;
  onDrop: (db: string) => void;
  onDump: (db: string) => void;
}) {
  const list = useRead<ServiceDatabases>(running ? "services.databases" : null, { name: i.name });
  const { run } = useChanges();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const valid = name ? databaseName(name) : null;
  if (!running) return <p className="m-0 px-1 text-[13px] text-text-2">Start {i.name} to see its databases.</p>;
  const total = (list.data?.databases ?? []).reduce((n, d) => n + d.bytes, 0);
  return (
    <>
      <form
        className="mb-5 flex items-end gap-3"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!valid) return;
          setBusy(true);
          if (await run(["db", "create", i.name, valid], `Created the database ${valid}.`)) setName("");
          setBusy(false);
          list.reload();
        }}
      >
        <TextField label="New database" className="w-[260px]" placeholder="shop" value={name} onChange={(e) => setName(e.target.value)} error={name && !valid ? "Letters, digits, _ $ and -." : undefined} />
        <Button type="submit" variant="ink" disabled={!valid || busy}>
          {busy ? "Creating…" : "Create"}
        </Button>
      </form>
      {list.error && <p className="m-0 text-[13px] text-bad">{list.error}</p>}
      <Section title="Databases" meta={list.data ? `${list.data.databases.length} · ${size(total)}` : undefined}>
        {list.data && list.data.databases.length === 0 && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">None yet besides the server's own.</li>}
        {list.data?.databases.map((d) => (
          <Row
            key={d.name}
            columns="minmax(160px,1fr) 100px minmax(160px,1.4fr) 80px"
            actions={
              <>
                <Button variant="quiet" aria-label={`Dump ${d.name}`} title="Dump it to a .sql file" icon={<ArrowDownload16Regular />} onClick={() => onDump(d.name)} />
                <Button variant="quiet" aria-label={`Delete ${d.name}`} title="Delete it" icon={<Delete16Regular />} onClick={() => onDrop(d.name)} />
              </>
            }
          >
            <RowName>{d.name}</RowName>
            <Detail mono>{size(d.bytes)}</Detail>
            <span className="flex min-w-0 flex-wrap gap-x-2 text-[12.5px] text-text-2">
              {d.used_by.length === 0
                ? "no site"
                : d.used_by.map((host) => (
                    <button key={host} type="button" className="hover:underline" onClick={() => go(`site:${host}`)}>
                      {host}
                    </button>
                  ))}
            </span>
          </Row>
        ))}
      </Section>
    </>
  );
}

/** The service's log, followed; a service with several files (MySQL's error and general log) can switch. */
function ServiceLog({ name }: { name: string }) {
  const sources = useRead<LogSources>("logs.sources");
  const mine = (sources.data?.sources ?? []).filter((s) => s.id === `service:${name}` || s.id.startsWith(`service:${name}/`));
  const [chosen, setChosen] = useState<string | null>(null);
  const log = mine.find((s) => s.id === chosen) ?? mine[0];
  if (!sources.data) return null;
  if (!log) return <p className="m-0 px-1 text-[13px] text-text-2">{name} has no log Kiln knows of.</p>;
  return (
    <div>
      {mine.length > 1 && (
        <div className="mb-3 max-w-[320px]">
          <Select label="File" value={log.id} onChange={(e) => setChosen(e.target.value)} options={mine.map((s) => ({ value: s.id, label: s.label }))} />
        </div>
      )}
      <LogPanel key={log.id} source={log.id} label={log.label} />
    </div>
  );
}

function ServiceEnv({ instance: i }: { instance: Instance }) {
  return (
    <Section title="What a site's .env gets" meta={`kiln env use ${i.name}`}>
      {i.env.map(([key, value]) => (
        <Row key={key} columns="minmax(160px,220px) minmax(0,1fr)">
          <span className="font-mono text-xs font-medium">{key}</span>
          <span className="truncate font-mono text-xs text-text-2" title={value}>
            {value}
          </span>
        </Row>
      ))}
      {i.databases && <li className="border-b border-line px-1 py-3 text-xs text-text-3">DB_DATABASE is the site's own: Kiln names it after the project and makes it if it's missing.</li>}
    </Section>
  );
}

function ServiceTools({ instance: i, hosts, pgadmin, onOpen }: { instance: Instance; hosts: string[]; pgadmin?: Instance; onOpen: (o: Open) => void }) {
  const { run, open: visit } = useChanges();
  const tool = (name: string) => hosts.find((h) => h.startsWith(`${name}.`));
  const browsers = i.databases && i.kind !== "mongodb" ? (i.kind === "postgres" ? [["adminer", "Adminer"]] : [["pma", "phpMyAdmin"], ["adminer", "Adminer"]]) : [];
  const item = (title: string, detail: string, action: string, onClick: () => void, danger = false) => (
    <Row key={title} columns="minmax(0,1fr) auto">
      <RowName sub={detail}>{title}</RowName>
      <Button variant={danger ? "quiet" : undefined} onClick={onClick}>
        {action}
      </Button>
    </Row>
  );
  return (
    <Section title="Tools">
      {!i.project && item("Wire a site to it", "Points a site's .env at it; shows the old and new values first.", "Choose a site…", () => onOpen("wire"))}
      {Object.keys(i.ports).length > 0 && item("Change a port", "Moves it to another port; sites wired to it are updated.", "Change…", () => onOpen("ports"))}
      {i.databases && i.kind !== "mongodb" && item("Dump", "Every database, or one, to a .sql file with its own tool.", "Dump…", () => onOpen("dump"))}
      {i.databases && i.kind !== "mongodb" && item("Restore", "Loads a .sql file into it.", "Restore…", () => onOpen("restore"))}
      {browsers.map(([site, label]) => {
        const host = tool(site);
        return item(
          `Browse in ${label}`,
          host ? `Served at ${host}, already connected.` : `Not added yet: Kiln downloads it and serves it at ${site}.test.`,
          host ? `Open ${label}` : `Add ${label}`,
          () => (host ? visit(host) : run(["tool", "add", site === "pma" ? "phpmyadmin" : "adminer"], `${label} is set up at ${site}.test.`)),
        );
      })}
      {i.kind === "postgres" &&
        item(
          "Browse in pgAdmin",
          pgadmin
            ? `Served at ${pgadmin.web ?? "pgadmin.test"}, with Kiln's PostgreSQL instances listed.`
            : "Not added: pgAdmin 4 from the same PostgreSQL download, about 340 MB unpacked. Only if you want it.",
          pgadmin ? "Open pgAdmin" : "Add pgAdmin…",
          () => (pgadmin ? visit(pgadmin.web ?? "pgadmin.test") : onOpen("pgadmin")),
        )}
      {item("Restart", "Stops it and starts it again.", "Restart", () => run(["service", "restart", i.name], `Restarted ${i.name}.`))}
      {item("Remove", "Removes it from Kiln; you choose whether its data stays.", "Remove…", () => onOpen("remove"), true)}
    </Section>
  );
}

/** Deleting a database: named, sized, typed back, and a dump offered first (ADR 0068 §2). */
function DropDialog({ instance, database, onDump, onClose }: { instance: Instance; database: string; onDump: () => void; onClose: () => void }) {
  const { run } = useChanges();
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      open
      onClose={() => !busy && onClose()}
      title={`Delete ${database}?`}
      footer={
        <>
          <Button onClick={onDump} disabled={busy}>
            Dump it first…
          </Button>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="ink"
            disabled={typed !== database || busy}
            onClick={async () => {
              setBusy(true);
              await run(["db", "drop", instance.name, database], `Deleted the database ${database}.`);
              onClose();
            }}
          >
            {busy ? "Deleting…" : "Delete"}
          </Button>
        </>
      }
    >
      <p className="mt-0">
        This deletes {database} on {instance.name} and everything in it. It can't be undone; a dump keeps a copy.
      </p>
      <TextField label={`Type ${database} to confirm`} value={typed} onChange={(e) => setTyped(e.target.value)} />
    </Dialog>
  );
}

