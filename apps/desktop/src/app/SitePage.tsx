import { useState } from "react";
import { ArrowLeft16Regular, Folder16Regular, Open16Regular } from "@fluentui/react-icons";
import { Button } from "../components/Button";
import { Notice } from "../components/Notice";
import { Page } from "../components/Page";
import { Row, Section } from "../components/Rows";
import { Select } from "../components/Select";
import { Switch } from "../components/Switch";
import { Tabs } from "../components/Tabs";
import { Tag } from "../components/Tag";
import { openFolder, type EnvShow, type LogSources, type PhpList, type ServicesList, type Site, type SitesList } from "./api";
import { useChanges } from "./Change";
import { LogPanel } from "./LogsPage";
import { useRead } from "./hooks";
import { framework } from "./SitesPage";

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
      after={<Tag>{framework(site)}</Tag>}
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
          { id: "logs", label: "Logs" },
          { id: "env", label: ".env" },
        ]}
        current={tab}
        onSelect={setTab}
      >
      {tab === "env" ? (
        <SiteEnv site={site} />
      ) : tab === "logs" ? (
        <SiteLog host={site.host} />
      ) : (
      <>
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

/** The site's own log: the newest in its storage/logs (ADR 0059 §4). */
function SiteLog({ host }: { host: string }) {
  const sources = useRead<LogSources>("logs.sources");
  const log = sources.data?.sources.find((s) => s.id === `site:${host}`);
  if (!sources.data) return null;
  if (!log) {
    return (
      <p className="m-0 px-1 text-[13px] text-text-2">
        {host} has no log yet: Kiln shows the newest file in its storage/logs once the site writes one. Kiln's own logs (the web server, PHP's errors, services) are on the Logs page.
      </p>
    );
  }
  return <LogPanel source={log.id} label={log.label} />;
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
