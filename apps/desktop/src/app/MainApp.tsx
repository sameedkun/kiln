import {
  Code20Regular,
  Cube20Regular,
  Database20Regular,
  Globe20Regular,
  History20Regular,
  Home20Regular,
  Settings20Regular,
  TextBulletListLtr20Regular,
} from "@fluentui/react-icons";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Sidebar, type NavItem } from "../components/Sidebar";
import { Dot } from "../components/Status";
import { TitleBar } from "../components/TitleBar";
import { closeNoticeRead, setTray, windowAction, type DaemonState, type ServicesList, type SitesList } from "./api";
import { Home } from "./Home";
import { useDaemon, useRead } from "./hooks";
import { NotYet, Settings } from "./Pages";

/** The tray icon's look, from what the app knows (ADR 0051 §6). */
export function trayState(daemon: DaemonState, services?: ServicesList): { state: "running" | "attention" | "stopped"; tooltip: string } {
  if (daemon !== "running") {
    return { state: "stopped", tooltip: daemon === "too-old" ? "Kiln: update Kiln to use this app" : "Kiln: not running" };
  }
  const down = services?.instances.filter((i) => i.enabled && !i.running) ?? [];
  if (down.length > 0) {
    return { state: "attention", tooltip: `Kiln: ${down.map((d) => d.name).join(", ")} not running` };
  }
  const running = services?.instances.filter((i) => i.running).length ?? 0;
  return { state: "running", tooltip: running ? `Kiln: ${running} service${running === 1 ? "" : "s"} running` : "Kiln" };
}

const daemonWords: Record<Exclude<DaemonState, "running">, string> = {
  starting: "Starting Kiln's daemon…",
  "too-old": "This Kiln is older than the app: its API is version 1. Update Kiln, then reopen the app.",
  unreachable: "Kiln's daemon isn't answering. In a terminal, `kiln daemon start` starts it; the app keeps trying.",
};

export function MainApp() {
  const [page, setPage] = useState("home");
  const [firstClose, setFirstClose] = useState(false);
  const daemon = useDaemon();
  const sites = useRead<SitesList>("sites.list");
  const services = useRead<ServicesList>("services.list");

  useEffect(() => {
    const { state, tooltip } = trayState(daemon, services.data);
    setTray(state, tooltip);
  }, [daemon, services.data]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    listen("kiln-first-close", () => setFirstClose(true)).then((u) => (stop = u));
    return () => stop?.();
  }, []);

  const down = services.data?.instances.some((i) => i.enabled && !i.running) ?? false;
  const pages: NavItem[] = [
    { id: "home", label: "Home", icon: <Home20Regular /> },
    { id: "sites", label: "Sites", icon: <Globe20Regular />, detail: sites.data ? String(sites.data.sites.length + sites.data.proxies.length) : undefined },
    { id: "services", label: "Services", icon: <Database20Regular />, attention: down },
    { id: "php", label: "PHP", icon: <Code20Regular /> },
    { id: "node", label: "Node", icon: <Cube20Regular /> },
    { id: "logs", label: "Logs", icon: <TextBulletListLtr20Regular />, groupStart: true },
    { id: "history", label: "History", icon: <History20Regular /> },
  ];

  return (
    <div className="backdrop grid h-screen w-screen grid-rows-[44px_1fr] overflow-hidden">
      <TitleBar onMinimise={() => windowAction("minimise")} onMaximise={() => windowAction("maximise")} onClose={() => windowAction("close")} />
      <div className="grid min-h-0 grid-cols-[220px_1fr] gap-2 px-2 pb-2">
        <Sidebar
          items={pages}
          footer={[{ id: "settings", label: "Settings", icon: <Settings20Regular /> }]}
          current={page}
          onSelect={setPage}
          status={
            <>
              <Dot state={daemon === "running" ? "running" : daemon === "starting" ? "busy" : "failed"} />
              {daemon === "running" ? "Kiln is running" : daemon === "starting" ? "Starting…" : "Kiln isn't answering"}
            </>
          }
        />
        <div className="grid min-h-0 grid-rows-[auto_1fr]">
          {daemon !== "running" ? (
            <div role="status" className="mx-1 mt-1.5 rounded-xl border border-line bg-warn-soft px-4 py-2.5 text-[13px]">
              {daemonWords[daemon]}
            </div>
          ) : (
            <span />
          )}
          {page === "home" ? <Home key="home" /> : page === "settings" ? <Settings key="settings" /> : <NotYet key={page} page={page} />}
        </div>
      </div>

      <Dialog
        open={firstClose}
        onClose={() => {
          setFirstClose(false);
          closeNoticeRead();
        }}
        title="Kiln keeps running"
        footer={
          <Button
            variant="ink"
            onClick={() => {
              setFirstClose(false);
              closeNoticeRead();
            }}
          >
            Got it
          </Button>
        }
      >
        Your sites and services keep running, and Kiln stays in the tray at the right of the taskbar. Click its icon to open this window again; right-click it for the
        quick menu. Quit is in that menu.
      </Dialog>
    </div>
  );
}
