import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { useToast } from "../components/Toast";
import { TrayPopup, type TrayService, type TraySite } from "../components/TrayPopup";
import { fitPopup, hidePopup, openSite, quit, runJob, showMain, type ServicesList, type SitesList } from "./api";
import { useChanges } from "./Change";
import { mainPort, serviceState } from "./Home";
import { useDaemon, useRead } from "./hooks";

// The tray popup (ADR 0051 §6, ADR 0054 §4): Kiln's state, sites to open, services to start
// and stop, and the way into the window or out of the app.
export function TrayApp() {
  const daemon = useDaemon();
  const sites = useRead<SitesList>("sites.list");
  const services = useRead<ServicesList>("services.list");
  const { run } = useChanges();
  const toast = useToast();
  const box = useRef<HTMLDivElement>(null);

  // Fresh each time it opens; Esc puts it away.
  useEffect(() => {
    let stop: (() => void) | undefined;
    listen("kiln-popup-shown", () => {
      sites.reload();
      services.reload();
    }).then((u) => (stop = u));
    const esc = (e: KeyboardEvent) => e.key === "Escape" && hidePopup();
    window.addEventListener("keydown", esc);
    return () => {
      stop?.();
      window.removeEventListener("keydown", esc);
    };
  }, []);

  // The window fits its content.
  useEffect(() => {
    if (!box.current) return;
    const observer = new ResizeObserver(() => box.current && fitPopup(box.current.offsetHeight));
    observer.observe(box.current);
    return () => observer.disconnect();
  }, []);

  const instances = services.data?.instances ?? [];
  const down = instances.filter((i) => i.enabled && !i.running);
  const shownSites: TraySite[] = [
    ...(sites.data?.sites.filter((s) => s.source !== "alias") ?? []).slice(0, 6).map((s) => ({
      name: s.host,
      secure: s.secure,
      detail: s.php?.version ? `PHP ${s.php.version.split(".").slice(0, 2).join(".")}` : s.driver,
    })),
    ...(sites.data?.proxies ?? []).slice(0, 2).map((p) => ({ name: p.host, secure: p.url.startsWith("https"), detail: "proxy" })),
  ];
  const shownServices: TrayService[] = instances.map((i) => ({ id: i.name, name: i.name, state: serviceState(i), port: mainPort(i) }));
  const summary =
    daemon !== "running"
      ? daemon === "starting"
        ? "Starting…"
        : "Kiln's daemon isn't answering"
      : instances.length
        ? `${instances.filter((i) => i.running).length} of ${instances.length} services running`
        : "Running";

  const stopAll = async () => {
    const services = await runJob(["services", "down"], "apply");
    const web = await runJob(["web", "stop"], "apply");
    toast({ message: services.ok && web.ok ? "Stopped the services and the web server." : "Something didn't stop: open Kiln to see." });
  };

  return (
    <div ref={box}>
      <TrayPopup
        fill
        summary={summary}
        attention={
          down.length
            ? { text: `${down[0].name} isn't running`, actionLabel: "Start", onAction: () => run(["service", "start", down[0].name], `Started ${down[0].name}.`) }
            : undefined
        }
        sites={shownSites}
        services={shownServices}
        onOpenSite={(host) => openSite(host).then(hidePopup)}
        onToggleService={(name) => {
          const instance = instances.find((i) => i.name === name);
          const start = !instance?.running;
          run(["service", start ? "start" : "stop", name], `${start ? "Started" : "Stopped"} ${name}.`);
        }}
        onOpenKiln={() => showMain()}
        onStopAll={stopAll}
        onQuit={() => quit()}
      />
    </div>
  );
}
