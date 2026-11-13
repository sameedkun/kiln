import { LockClosed12Regular, Open16Regular, Play16Filled, Stop16Filled } from "@fluentui/react-icons";
import type { ReactNode } from "react";
import { Button } from "../components/Button";
import { Notice } from "../components/Notice";
import { Page } from "../components/Page";
import { Detail, Row, RowName, Section } from "../components/Rows";
import { Status, type StatusState } from "../components/Status";
import { Tag } from "../components/Tag";
import { type HistoryList, type Instance, type ServicesList, type SitesList } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";

const siteCols = "minmax(180px,1.3fr) 110px minmax(160px,1.4fr) 40px";
const serviceCols = "minmax(160px,1.2fr) 90px minmax(120px,1.3fr) 170px";

export function serviceState(i: Instance): StatusState {
  if (i.running) return "running";
  return i.enabled ? "attention" : "stopped";
}

export function mainPort(i: Instance): number | undefined {
  return i.main_port ?? Object.values(i.ports)[0];
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

// Home, from Kiln's own answers: what needs the user first, then sites, services and the
// latest changes (ADR 0051 §4, ADR 0054 §5).
export function Home({ go }: { go: (page: string) => void }) {
  const sites = useRead<SitesList>("sites.list");
  const services = useRead<ServicesList>("services.list");
  const history = useRead<HistoryList>("history.list");
  const { run, confirm, open } = useChanges();
  const instances = services.data?.instances ?? [];
  const down = instances.filter((i) => i.enabled && !i.running);
  const listed = sites.data?.sites.filter((s) => s.source !== "alias") ?? [];

  return (
    <Page title="Home">
      {down.length > 0 && (
        <Notice
          tone="attention"
          title={down.length === 1 ? `${down[0].name} isn't running` : `${down.length} services aren't running`}
          actions={
            <Button variant="ink" icon={<Play16Filled />} onClick={() => run(["service", "start", ...down.map((d) => d.name)], `Started ${down.map((d) => d.name).join(", ")}.`)}>
              {down.length === 1 ? `Start ${down[0].name}` : "Start them"}
            </Button>
          }
        >
          {down.map((d) => d.name).join(", ")} should be running but {down.length === 1 ? "doesn't" : "don't"} answer.
        </Notice>
      )}

      <Section title="Sites" meta={sites.data ? `${count(listed.length, "site", "sites")} · ${count(sites.data.proxies.length, "proxy", "proxies")}` : undefined}>
        {sites.error && <Empty>{sites.error}</Empty>}
        {sites.data && listed.length === 0 && sites.data.proxies.length === 0 && (
          <Empty>
            No sites yet. In a terminal, <code className="font-mono text-xs">kiln link</code> in a project folder, or{" "}
            <code className="font-mono text-xs">kiln park</code> in a folder of projects. Adding sites here comes with the Sites page.
          </Empty>
        )}
        {listed.map((s) => (
          <Row key={s.host} columns={siteCols} actions={<Button variant="quiet" aria-label={`Open ${s.host}`} icon={<Open16Regular />} onClick={() => open(s.host)} />}>
            <RowName before={<LockClosed12Regular aria-label={s.secure ? "HTTPS" : undefined} aria-hidden={s.secure ? undefined : true} className={`shrink-0 text-text-3 ${s.secure ? "" : "invisible"}`} />} sub={s.document_root} mono>
              <button type="button" className="hover:underline" onClick={() => go(`site:${s.host}`)}>
                {s.host}
              </button>
            </RowName>
            <Detail>
              <Tag>{s.driver}</Tag>
            </Detail>
            <Detail>{s.php?.version ? `PHP ${s.php.version}` : s.php ? "PHP: none installed" : "static"}</Detail>
          </Row>
        ))}
        {sites.data?.proxies.map((p) => (
          <Row key={p.host} columns={siteCols} actions={<Button variant="quiet" aria-label={`Open ${p.host}`} icon={<Open16Regular />} onClick={() => open(p.host)} />}>
            <RowName before={<LockClosed12Regular aria-label={p.url.startsWith("https") ? "HTTPS" : undefined} aria-hidden={p.url.startsWith("https") ? undefined : true} className={`shrink-0 text-text-3 ${p.url.startsWith("https") ? "" : "invisible"}`} />} sub={p.target ? `→ ${p.target}` : undefined} mono>
              {p.host}
            </RowName>
            <Detail>
              <Tag>proxy</Tag>
            </Detail>
            <Detail>{p.in_hosts ? "" : "not in the hosts file yet"}</Detail>
          </Row>
        ))}
      </Section>

      <Section title="Services" meta={services.data ? `${instances.filter((i) => i.running).length} of ${instances.length} running` : undefined}>
        {services.error && <Empty>{services.error}</Empty>}
        {services.data && instances.length === 0 && (
          <Empty>
            No services yet. In a terminal, <code className="font-mono text-xs">kiln service add mysql</code> (see{" "}
            <code className="font-mono text-xs">kiln service catalog</code>).
          </Empty>
        )}
        {instances.map((i) => {
          const state = serviceState(i);
          const running = state === "running";
          return (
            <Row key={i.name} columns={serviceCols}>
              <RowName>
                {i.name} <span className="figures text-[12.5px] font-normal text-text-2">{i.version}</span>
              </RowName>
              <Detail mono>{mainPort(i) ? `:${mainPort(i)}` : ""}</Detail>
              <Detail>{i.project ? `runs in ${i.project}` : i.kind}</Detail>
              <span className="flex items-center justify-end gap-2">
                <Status state={state} label={state === "attention" ? "Not running" : undefined} />
                <Button
                  variant="quiet"
                  aria-label={`${running ? "Stop" : "Start"} ${i.name}`}
                  icon={running ? <Stop16Filled /> : <Play16Filled />}
                  onClick={() => run(["service", running ? "stop" : "start", i.name], `${running ? "Stopped" : "Started"} ${i.name}.`)}
                />
              </span>
            </Row>
          );
        })}
      </Section>

      <Section title="Recent changes">
        {history.data && history.data.operations.length === 0 && <Empty>Kiln hasn't changed anything on this PC yet.</Empty>}
        {history.data?.operations.slice(0, 4).map((op) => (
          <Row
            key={op.id}
            columns="72px 1fr auto"
            actions={
              <Button variant="quiet" size="sm" onClick={() => confirm(`Undo #${op.id}?`, ["undo", String(op.id)], `Undid #${op.id}.`)}>
                Undo
              </Button>
            }
          >
            <span className="figures text-xs text-text-3">{op.at_ms ? new Date(op.at_ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : `#${op.id}`}</span>
            <span>{op.description}</span>
          </Row>
        ))}
      </Section>
    </Page>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">{children}</li>;
}
