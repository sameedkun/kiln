import {
  Add16Regular,
  ArrowRight12Regular,
  Code20Regular,
  Cube20Regular,
  Database20Regular,
  Delete16Regular,
  Folder16Regular,
  Globe20Regular,
  History20Regular,
  Home20Regular,
  LockClosed12Regular,
  MoreHorizontal16Regular,
  Open16Regular,
  Play16Filled,
  Settings20Regular,
  Stop16Filled,
  TextBulletListLtr20Regular,
} from "@fluentui/react-icons";
import { useState, type ReactNode } from "react";
import { appearance, setAppearance } from "../appearance";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Menu } from "../components/Menu";
import { Notice } from "../components/Notice";
import { Page } from "../components/Page";
import { Detail, Row, RowName, Section } from "../components/Rows";
import { Sidebar, type NavItem } from "../components/Sidebar";
import { Dot, Status } from "../components/Status";
import { Switch } from "../components/Switch";
import { Tag } from "../components/Tag";
import { TextField } from "../components/TextField";
import { TitleBar } from "../components/TitleBar";
import { useToast } from "../components/Toast";
import { TrayPopup } from "../components/TrayPopup";

// The design gallery (P4-02): Kiln's base components with sample data, for review and for
// the screenshot and accessibility checks (e2e/gallery.spec.ts). Not part of the app.

const pages: NavItem[] = [
  { id: "home", label: "Home", icon: <Home20Regular /> },
  { id: "sites", label: "Sites", icon: <Globe20Regular />, detail: "4" },
  { id: "php", label: "PHP", icon: <Code20Regular />, detail: "8.4 · 8.3" },
  { id: "node", label: "Node", icon: <Cube20Regular />, detail: "22" },
  { id: "services", label: "Services", icon: <Database20Regular />, attention: true },
  { id: "logs", label: "Logs", icon: <TextBulletListLtr20Regular />, groupStart: true },
  { id: "history", label: "History", icon: <History20Regular /> },
];
const footer: NavItem[] = [{ id: "settings", label: "Settings", icon: <Settings20Regular /> }];

const siteCols = "minmax(180px,1.3fr) 90px minmax(160px,1.4fr) 112px";
const serviceCols = "minmax(160px,1.2fr) 110px minmax(120px,1.3fr) 170px";
const lock = <LockClosed12Regular aria-label="HTTPS" className="text-text-3" />;
const noLock = <LockClosed12Regular aria-hidden="true" className="invisible" />;

function More({ name }: { name: string }) {
  return (
    <Menu
      label={`More for ${name}`}
      trigger={(props) => <Button variant="quiet" aria-label={`More for ${name}`} icon={<MoreHorizontal16Regular />} {...props} />}
      items={[
        { label: "Open in the browser", icon: <Open16Regular />, onSelect: () => {} },
        { label: "Open the folder", icon: <Folder16Regular />, onSelect: () => {} },
        { label: "Unlink…", icon: <Delete16Regular />, danger: true, groupStart: true, onSelect: () => {} },
      ]}
    />
  );
}

function SiteActions({ name, folder }: { name: string; folder?: boolean }) {
  return (
    <>
      {folder && <Button variant="quiet" aria-label={`Open ${name}'s folder`} icon={<Folder16Regular />} />}
      <Button variant="quiet" aria-label={`Open ${name}`} icon={<Open16Regular />} />
      <More name={name} />
    </>
  );
}

function ServiceRow({ name, version, port, used, state }: { name: string; version: string; port: number; used: string; state: "running" | "attention" | "stopped" }) {
  const running = state === "running";
  return (
    <Row columns={serviceCols}>
      <RowName>
        {name} <span className="figures text-[12.5px] font-normal text-text-2">{version}</span>
      </RowName>
      <Detail mono>:{port}</Detail>
      <Detail>{used}</Detail>
      <span className="flex items-center justify-end gap-2">
        <Status state={state} label={running ? undefined : "Stopped"} />
        <Button variant="quiet" aria-label={`${running ? "Stop" : "Start"} ${name}`} icon={running ? <Stop16Filled /> : <Play16Filled />} />
      </span>
    </Row>
  );
}

function HomeWindow() {
  const [page, setPage] = useState("home");
  return (
    <div className="backdrop relative grid h-[720px] w-[1120px] shrink-0 grid-rows-[44px_1fr] overflow-hidden rounded-lg shadow-[0_2px_8px_rgba(0,0,0,.18),0_30px_80px_rgba(0,0,0,.35)] outline outline-1 outline-black/25">
      <TitleBar />
      <div className="grid min-h-0 grid-cols-[220px_1fr] gap-2 px-2 pb-2">
        <Sidebar
          items={pages}
          footer={footer}
          current={page}
          onSelect={setPage}
          status={
            <>
              <Dot state="running" />
              Web server running on :443
            </>
          }
        />
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
                <Button variant="quiet">View log</Button>
                <Button variant="ink" icon={<Play16Filled />}>
                  Start Redis
                </Button>
              </>
            }
          >
            It ran out of memory (maxmemory 256 MB). shop.test uses it for its cache and queue.
          </Notice>

          <Section title="Sites" meta="parked in D:\Code · 2 linked" action={<Button variant="quiet" size="sm">All sites <ArrowRight12Regular /></Button>}>
            <Row columns={siteCols} actions={<SiteActions name="shop.test" folder />}>
              <RowName before={lock} sub="D:\Code\shop" mono>shop.test</RowName>
              <Detail>PHP 8.4</Detail>
              <Detail>MySQL 8.4 · Redis · Mailpit</Detail>
            </Row>
            <Row columns={siteCols} actions={<SiteActions name="blog.test" folder />}>
              <RowName before={lock} sub="D:\Code\blog" mono>blog.test</RowName>
              <Detail>PHP 8.3</Detail>
              <Detail>PostgreSQL 17</Detail>
            </Row>
            <Row columns={siteCols} actions={<SiteActions name="api.test" />}>
              <RowName before={noLock} sub="→ 127.0.0.1:5173" mono>
                api.test <Tag>proxy</Tag>
              </RowName>
              <Detail>—</Detail>
              <Detail>Vite dev server</Detail>
            </Row>
            <Row columns={siteCols} actions={<SiteActions name="mailpit.test" />}>
              <RowName before={lock} sub="Mailpit's inbox">mailpit.test</RowName>
              <Detail>—</Detail>
              <Detail>3 new messages</Detail>
            </Row>
          </Section>

          <Section title="Services" meta="3 of 4 running" action={<Button variant="quiet" size="sm">All services <ArrowRight12Regular /></Button>}>
            <ServiceRow name="MySQL" version="8.4.6" port={3306} used="shop" state="running" />
            <ServiceRow name="Redis" version="7.4.5" port={6379} used="shop" state="attention" />
            <ServiceRow name="PostgreSQL" version="17.6" port={5432} used="blog" state="running" />
            <ServiceRow name="Mailpit" version="1.27" port={1025} used="shop" state="running" />
          </Section>

          <Section title="Recent changes" action={<Button variant="quiet" size="sm">History <ArrowRight12Regular /></Button>}>
            {[
              ["14:10", <>Linked <b>api.test</b> as a proxy to 127.0.0.1:5173</>],
              ["11:32", <>Wired shop's <span className="font-mono text-xs">.env</span> to Redis (5 keys)</>],
              ["Yesterday", <>Installed PHP 8.4.13</>],
            ].map(([when, what], i) => (
              <Row key={i} columns="72px 1fr auto" actions={<Button variant="quiet" size="sm">Undo</Button>}>
                <span className="figures text-xs text-text-3">{when}</span>
                <span>{what}</span>
              </Row>
            ))}
          </Section>
        </Page>
      </div>
    </div>
  );
}

function Tray() {
  return (
    <div className="relative h-[560px] w-[520px] shrink-0 overflow-hidden rounded-lg bg-[linear-gradient(160deg,#5d6b7a,#3b4552_60%,#2c333d)]">
      <div className="absolute bottom-[58px] right-3">
        <TrayPopup
          summary="3 of 4 services running"
          attention={{ text: "Redis stopped at 14:02", actionLabel: "Start", onAction: () => {} }}
          sites={[
            { name: "shop.test", secure: true, detail: "PHP 8.4" },
            { name: "blog.test", secure: true, detail: "PHP 8.3" },
            { name: "api.test", secure: false, detail: "proxy" },
            { name: "mailpit.test", secure: true, detail: "3 new" },
          ]}
          services={[
            { id: "mysql", name: "MySQL", state: "running", port: 3306 },
            { id: "redis", name: "Redis", state: "attention" },
            { id: "postgres", name: "PostgreSQL", state: "running", port: 5432 },
            { id: "mailpit", name: "Mailpit", state: "running", port: 1025 },
          ]}
          onOpenSite={() => {}}
          onToggleService={() => {}}
          onOpenKiln={() => {}}
          onStopAll={() => {}}
          onQuit={() => {}}
        />
      </div>
      <div className="absolute inset-x-0 bottom-0 flex h-12 items-center justify-end gap-1 border-t border-black/10 bg-[rgba(32,32,32,.86)] pr-3 text-xs text-white">
        <span className="relative grid h-10 w-8 place-items-center rounded-md bg-white/10" aria-hidden="true">
          <svg width="18" height="18" viewBox="0 0 24 24">
            <path d="M4 21V11.5a8 8 0 0 1 16 0V21" fill="none" stroke="currentColor" strokeWidth="1.7" />
          </svg>
          <span className="absolute bottom-2.5 right-1 size-[7px] rounded-full bg-[#facc15]" />
        </span>
        <span className="figures px-2 text-right leading-4">
          14:12
          <br />
          12/11/2026
        </span>
      </div>
    </div>
  );
}

function Sheet() {
  const [startInTray, setStartInTray] = useState(true);
  const [atLogin, setAtLogin] = useState(false);
  const [dialog, setDialog] = useState(false);
  const toast = useToast();
  return (
    <div className="backdrop w-[1120px] rounded-lg p-7 shadow-[0_30px_80px_rgba(0,0,0,.35)]">
      <div className="grid grid-cols-2 gap-x-12 gap-y-8">
        <Group title="Buttons">
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="ink" icon={<Play16Filled />}>
              Start Redis
            </Button>
            <Button icon={<Add16Regular />}>New project</Button>
            <Button variant="quiet">View log</Button>
            <Button variant="quiet" aria-label="Open shop.test" icon={<Open16Regular />} />
            <Button disabled>Disabled</Button>
            <Button size="sm">Small</Button>
          </div>
        </Group>
        <Group title="Statuses: always a dot and a word">
          <div className="flex flex-wrap gap-5">
            <Status state="running" />
            <Status state="attention" label="Stopped after a crash" />
            <Status state="failed" label="Failed to start" />
            <Status state="stopped" />
            <Status state="busy" label="Starting…" />
          </div>
        </Group>
        <Group title="Switches">
          <div className="flex flex-col gap-4">
            <Switch checked={startInTray} onChange={setStartInTray} label="When Kiln starts, stay in the tray" description="Open the window from the tray icon." />
            <Switch checked={atLogin} onChange={setAtLogin} label="Start Kiln when I sign in" />
          </div>
        </Group>
        <Group title="Text fields">
          <div className="grid grid-cols-2 gap-4">
            <TextField label="Site name" defaultValue="shop" hint="Opens as https://shop.test" />
            <TextField label="Port" defaultValue="3306" mono error="Port 3306 is used by Laravel Herd's MySQL." />
          </div>
        </Group>
        <Group title="Tags">
          <div className="flex gap-2">
            <Tag>proxy</Tag>
            <Tag>PHP 8.4</Tag>
            <Tag>from herd.yml</Tag>
          </div>
        </Group>
        <Group title="Menu, dialog and toast">
          <div className="flex flex-wrap gap-2">
            <More name="shop.test" />
            <Button onClick={() => setDialog(true)}>Unlink shop.test…</Button>
            <Button onClick={() => toast({ message: "Linked api.test as a proxy to 127.0.0.1:5173", action: { label: "Undo", onClick: () => {} } })}>
              Show a toast
            </Button>
          </div>
        </Group>
      </div>
      <Dialog
        open={dialog}
        onClose={() => setDialog(false)}
        title="Unlink shop.test?"
        footer={
          <>
            <Button onClick={() => setDialog(false)}>Cancel</Button>
            <Button variant="ink" onClick={() => setDialog(false)}>
              Unlink
            </Button>
          </>
        }
      >
        Kiln stops serving <b className="text-text">https://shop.test</b>. The folder D:\Code\shop and its database stay as they are, and History can undo this.
      </Dialog>
    </div>
  );
}

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title}>
      <h3 className="m-0 mb-3 text-xs font-medium tracking-[.02em] text-text-3">{title}</h3>
      {children}
    </section>
  );
}

function ReviewBar() {
  const [now, setNow] = useState(appearance());
  const set = (next: Parameters<typeof setAppearance>[0]) => {
    setAppearance(next);
    setNow(appearance());
  };
  const seg = (on: boolean) => `rounded-md px-2.5 py-0.5 text-xs ${on ? "bg-white text-[#111]" : "bg-white/10 text-white"}`;
  return (
    <div role="group" aria-label="Gallery controls (not part of the design)" className="sticky top-0 z-40 flex items-center gap-1.5 bg-black/55 px-4 py-2 text-xs text-white backdrop-blur">
      <b className="mr-2">Kiln design gallery</b>
      {(["system", "light", "dark"] as const).map((t) => (
        <button key={t} type="button" aria-pressed={now.theme === t} className={seg(now.theme === t)} onClick={() => set({ theme: t })}>
          {t[0].toUpperCase() + t.slice(1)}
        </button>
      ))}
      <span className="w-3" />
      <button type="button" aria-pressed={now.solid === true} className={seg(now.solid === true)} onClick={() => set({ solid: now.solid === true ? "system" : true })}>
        Transparency off
      </button>
    </div>
  );
}

export function Gallery() {
  return (
    <div className="min-h-full bg-[linear-gradient(160deg,#5d6b7a,#3b4552_60%,#2c333d)] text-text">
      <ReviewBar />
      <div className="flex flex-col gap-10 overflow-x-auto p-8">
        <Shown title="Home">
          <HomeWindow />
        </Shown>
        <Shown title="Tray popup (right-click on the tray icon)">
          <Tray />
        </Shown>
        <Shown title="Components">
          <Sheet />
        </Shown>
      </div>
    </div>
  );
}

function Shown({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section aria-label={title} data-shown={title}>
      <h2 className="m-0 mb-3 text-sm font-medium text-white/80">{title}</h2>
      {children}
    </section>
  );
}
