import {
  ArrowLeft16Regular,
  ArrowRight12Regular,
  Code16Regular,
  Folder16Regular,
  Open16Regular,
  WindowConsole20Regular,
} from "@fluentui/react-icons";
import { useState } from "react";
import { BarChart } from "../../components/BarChart";
import { Button } from "../../components/Button";
import { Chips } from "../../components/Chips";
import { LogView } from "../../components/LogView";
import { Page } from "../../components/Page";
import { Detail, Row, RowName, Section } from "../../components/Rows";
import { Select } from "../../components/Select";
import { Status } from "../../components/Status";
import { Switch } from "../../components/Switch";
import { Tabs } from "../../components/Tabs";
import { Tag } from "../../components/Tag";
import { env, laravelLog, requests, services, siteGroups, workers } from "../sample";
import { Figure, Label, More, StartStop, type Go } from "./common";

// One site: how it runs (PHP, Node, HTTPS), its workers and services, its requests, its logs
// and its .env.
export function Site({ name, go }: { name: string; go: Go }) {
  const site = siteGroups.flatMap((g) => g.sites).find((s) => s.name === name)!;
  const [tab, setTab] = useState("overview");
  return (
    <Page
      title={site.name}
      leading={<Button variant="quiet" aria-label="Back to Sites" icon={<ArrowLeft16Regular />} onClick={() => go("sites")} />}
      after={site.framework && <Tag>{site.framework}</Tag>}
      actions={
        <>
          <Button variant="quiet" aria-label="Open the folder" title="Open the folder" icon={<Folder16Regular />} />
          <Button variant="quiet" aria-label="Open a terminal here" title="Open a terminal here" icon={<WindowConsole20Regular className="size-4" />} />
          <Button variant="quiet" aria-label="Open in the editor" title="Open in the editor" icon={<Code16Regular />} />
          <Button icon={<Open16Regular />}>Open {site.name}</Button>
        </>
      }
    >
      <p className="-mt-2 mb-4 font-mono text-xs text-text-3">
        {site.secure ? "https" : "http"}://{site.name} · {site.folder}
      </p>
      <Tabs
        tabs={[
          { id: "overview", label: "Overview" },
          { id: "requests", label: "Requests", detail: requests.count },
          { id: "logs", label: "Logs" },
          { id: "env", label: ".env" },
        ]}
        current={tab}
        onSelect={setTab}
      >
        {tab === "overview" && <Overview go={go} setTab={setTab} />}
        {tab === "requests" && <Requests />}
        {tab === "logs" && <LogView label="storage/logs/laravel.log" lines={laravelLog} />}
        {tab === "env" && <Env />}
      </Tabs>
    </Page>
  );
}

function Overview({ go, setTab }: { go: Go; setTab: (t: string) => void }) {
  const [secure, setSecure] = useState(true);
  const used = services.filter((s) => s.usedBy.includes("shop"));
  return (
    <>
      <div className="mb-7 flex flex-wrap items-end gap-6">
        <Select label="PHP" defaultValue="8.4" options={[{ value: "8.5", label: "8.5" }, { value: "8.4", label: "8.4 (default)" }, { value: "8.3", label: "8.3" }]} />
        <Select label="Node" defaultValue="22" options={[{ value: "24", label: "24" }, { value: "22", label: "22 (default)" }]} />
        <div className="w-[230px]">
          <Switch checked={secure} onChange={setSecure} label="HTTPS" description="With Kiln's own certificate" />
        </div>
      </div>

      <Section title="Workers" meta="run by Kiln while the site is served">
        {workers.map((w) => (
          <Row key={w.id} columns="minmax(150px,1fr) minmax(200px,1.4fr) 210px 30px" actions={<More name={w.name} items={[{ label: "View its log", onSelect: () => setTab("logs") }, { label: "Restart", onSelect: () => {} }, { label: "Don't run it for this site", danger: true, groupStart: true, onSelect: () => {} }]} />}>
            <RowName>{w.name}</RowName>
            <Detail mono>{w.command}</Detail>
            <span className="flex items-center justify-end gap-2">
              <Status state={w.state} label={w.state === "running" && w.detail ? `Running · ${w.detail}` : undefined} />
              <StartStop name={w.name} state={w.state} />
            </span>
          </Row>
        ))}
      </Section>

      <Section
        title="Services"
        meta="wired into its .env"
        action={
          <Button variant="quiet" size="sm">
            Wire another…
          </Button>
        }
      >
        {used.map((s) => (
          <Row key={s.id} columns="minmax(150px,1fr) minmax(200px,1.4fr) 180px">
            <RowName>
              <button type="button" className="hover:underline" onClick={() => go("services")}>
                {s.name}
              </button>
            </RowName>
            <Detail>{s.category === "databases" ? "database shop" : s.category === "cache" ? "cache and queue" : s.category === "mail" ? "outgoing mail" : "broadcasting"}</Detail>
            <span className="flex items-center justify-end gap-2">
              <Status state={s.state} label={s.state === "attention" ? "Stopped" : undefined} />
              <StartStop name={s.name} state={s.state} />
            </span>
          </Row>
        ))}
      </Section>

      <div className="flex items-center gap-3 px-1 text-[13px] text-text-2">
        Last hour: {requests.count} requests, typically {requests.typical}, {requests.errors} failed.
        <Button variant="quiet" size="sm" onClick={() => setTab("requests")}>
          Requests <ArrowRight12Regular />
        </Button>
      </div>
    </>
  );
}

function Requests() {
  const [range, setRange] = useState<"15m" | "1h" | "24h">("1h");
  return (
    <>
      <div className="mb-6 flex items-center justify-between">
        <Chips
          label="Time range"
          value={range}
          onChange={setRange}
          options={[
            { id: "15m", label: "15 minutes" },
            { id: "1h", label: "1 hour" },
            { id: "24h", label: "24 hours" },
          ]}
        />
        <span className="text-xs text-text-3">From Caddy's access log for this site; nothing leaves this PC.</span>
      </div>
      <div className="mb-7 grid grid-cols-4 gap-6">
        <Figure value={requests.typical} label="typical (median)" />
        <Figure value={requests.slowest} label="slowest 5% take at least" />
        <Figure value={requests.count} label="requests" />
        <Figure value={requests.errors} label={`failed (${requests.errorCount} with 5xx)`} tone="warn" />
      </div>
      <div className="mb-7 max-w-[560px]">
        <BarChart label="Response time, in milliseconds" bars={requests.buckets} />
      </div>
      <Section title="Slowest requests">
        {requests.slow.map((r) => (
          <Row key={r.time} columns="72px 52px minmax(200px,1fr) 52px 80px">
            <span className="figures text-xs text-text-3">{r.time}</span>
            <Detail mono>{r.method}</Detail>
            <Detail mono>{r.path}</Detail>
            <span className={`figures text-[12.5px] ${r.status >= 500 ? "font-semibold text-bad" : "text-text-2"}`}>{r.status}</span>
            <span className={`figures text-right text-[12.5px] ${r.ms >= 1000 ? "font-semibold text-bad" : "text-warn"}`}>{r.ms.toLocaleString("en")} ms</span>
          </Row>
        ))}
      </Section>
    </>
  );
}

function Env() {
  const [shown, setShown] = useState<string[]>([]);
  return (
    <>
      <div className="mb-4 flex items-center justify-between gap-4">
        <Label>D:\Code\shop\.env · Kiln shows each change before making it, and History can undo it.</Label>
        <Button size="sm">Edit .env</Button>
      </div>
      <ul className="m-0 list-none p-0">
        {env.map((e) => (
          <Row key={e.key} columns="200px minmax(200px,1fr) 120px">
            <span className="font-mono text-xs font-medium">{e.key}</span>
            <span className="truncate font-mono text-xs text-text-2">
              {e.secret && !shown.includes(e.key) ? "••••••••" : e.value}
              {e.secret && (
                <button type="button" className="ml-2 font-sans text-xs text-text hover:underline" onClick={() => setShown((s) => (s.includes(e.key) ? s.filter((k) => k !== e.key) : [...s, e.key]))}>
                  {shown.includes(e.key) ? "Hide" : "Show"}
                </button>
              )}
            </span>
            <span className="text-right">{e.kiln && <Tag>set by Kiln</Tag>}</span>
          </Row>
        ))}
      </ul>
    </>
  );
}
