import { Add16Regular, ArrowRight12Regular, Play16Filled, Stop16Filled } from "@fluentui/react-icons";
import { Button } from "../../components/Button";
import { Notice } from "../../components/Notice";
import { Page } from "../../components/Page";
import { Detail, Row, RowName, Section } from "../../components/Rows";
import { Status } from "../../components/Status";
import { Tag } from "../../components/Tag";
import { history, services, siteGroups, usage } from "../sample";
import { lock, noLock, SiteActions, StartStop, type Go } from "./common";

const siteCols = "minmax(180px,1.3fr) 100px minmax(160px,1.4fr) 112px";
const serviceCols = "minmax(160px,1.2fr) 90px minmax(120px,1.3fr) 170px";

// Home: the one sentence that matters, then a short look at sites, services and changes.
export function Home({ go }: { go: Go }) {
  const sites = siteGroups.flatMap((g) => g.sites).slice(0, 4);
  return (
    <Page
      title="Home"
      actions={
        <>
          <Button variant="quiet" icon={<Stop16Filled />}>
            Stop all
          </Button>
          <Button icon={<Add16Regular />}>New project</Button>
        </>
      }
    >
      <Notice
        tone="attention"
        title="Redis stopped at 14:02"
        actions={
          <>
            <Button variant="quiet" onClick={() => go("logs")}>
              View log
            </Button>
            <Button variant="ink" icon={<Play16Filled />}>
              Start Redis
            </Button>
          </>
        }
      >
        It ran out of memory (maxmemory 256 MB). shop.test uses it for its cache and queue.
      </Notice>

      <Section
        title="Sites"
        meta="4 parked in D:\Code · 1 linked · 3 proxies and tools"
        action={
          <Button variant="quiet" size="sm" onClick={() => go("sites")}>
            All sites <ArrowRight12Regular />
          </Button>
        }
      >
        {sites.map((s) => (
          <Row key={s.name} columns={siteCols} actions={<SiteActions name={s.name} folder />}>
            <RowName before={s.secure ? lock : noLock} sub={s.folder} mono>
              <button type="button" className="hover:underline" onClick={() => go(`site:${s.name}`)}>
                {s.name}
              </button>
            </RowName>
            <Detail>{s.framework && <Tag>{s.framework}</Tag>}</Detail>
            <Detail>
              PHP {s.php}
              {s.services.length > 0 && ` · ${s.services.join(" · ")}`}
            </Detail>
          </Row>
        ))}
      </Section>

      <Section
        title="Services"
        meta={`${services.filter((s) => s.state === "running").length} of ${services.length} running`}
        action={
          <Button variant="quiet" size="sm" onClick={() => go("services")}>
            All services <ArrowRight12Regular />
          </Button>
        }
      >
        {services.slice(0, 4).map((s) => (
          <Row key={s.id} columns={serviceCols}>
            <RowName>
              {s.name} <span className="figures text-[12.5px] font-normal text-text-2">{s.version}</span>
            </RowName>
            <Detail mono>:{s.port}</Detail>
            <Detail>{s.usedBy.join(", ") || "no sites yet"}</Detail>
            <span className="flex items-center justify-end gap-2">
              <Status state={s.state} label={s.state === "attention" ? "Stopped" : undefined} />
              <StartStop name={s.name} state={s.state} />
            </span>
          </Row>
        ))}
      </Section>

      <Section
        title="Recent changes"
        action={
          <Button variant="quiet" size="sm" onClick={() => go("history")}>
            History <ArrowRight12Regular />
          </Button>
        }
      >
        {history[0].items.slice(0, 2).map((h) => (
          <Row
            key={h.time}
            columns="72px 1fr auto"
            actions={
              <Button variant="quiet" size="sm">
                Undo
              </Button>
            }
          >
            <span className="figures text-xs text-text-3">{h.time}</span>
            <span>{h.what}</span>
          </Row>
        ))}
      </Section>

      <p className="m-0 flex items-center gap-3 px-1 text-[13px] text-text-2">
        Kiln uses {usage.memory.display} of memory and {usage.disk.display} of disk on this PC.
        <Button variant="quiet" size="sm" onClick={() => go("settings")}>
          Details
        </Button>
      </p>
    </Page>
  );
}
