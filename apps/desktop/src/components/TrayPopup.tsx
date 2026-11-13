import { LockClosed12Regular, Open16Regular, Play16Filled, Stop16Filled } from "@fluentui/react-icons";
import { Button } from "./Button";
import { Logo } from "./Logo";
import { Dot, type StatusState } from "./Status";

export interface TraySite {
  name: string;
  secure: boolean;
  detail: string;
}
export interface TrayService {
  id: string;
  name: string;
  state: StatusState;
  /** Shown beside a running service: its port. */
  port?: number;
}

// The tray popup (ADR 0051 §6): what right-clicking the tray icon opens. In the app it is a
// small window of its own, placed against the tray icon; here it is just its content.
export function TrayPopup({
  summary,
  attention,
  sites,
  services,
  onOpenSite,
  onToggleService,
  onOpenKiln,
  onStopAll,
  onQuit,
  fill,
}: {
  summary: string;
  attention?: { text: string; actionLabel: string; onAction: () => void };
  sites: TraySite[];
  services: TrayService[];
  onOpenSite: (name: string) => void;
  onToggleService: (id: string) => void;
  onOpenKiln: () => void;
  onStopAll: () => void;
  onQuit: () => void;
  /** Fills its own window (the app); without it, a framed popup (the gallery). */
  fill?: boolean;
}) {
  const item = "flex h-[34px] w-full items-center gap-2.5 rounded-[7px] px-2 text-left text-[13px]";
  const label = "px-2 pb-1 pt-0.5 text-[11.5px] font-medium text-text-3";
  return (
    <div role="dialog" aria-label="Kiln" className={`backdrop overflow-hidden text-text ${fill ? "w-full" : "pop-frame w-[340px] rounded-pop"}`}>
      <div className="flex items-center gap-2.5 px-3.5 pb-2.5 pt-3.5">
        <Logo size={22} />
        <div>
          <div className="font-semibold">Kiln</div>
          <div className="text-xs text-text-2">{summary}</div>
        </div>
      </div>
      {attention && (
        <div className="mx-2 mb-1.5 flex items-center gap-2.5 rounded-[9px] bg-warn-soft py-2 pl-3 pr-2 text-[13px]">
          <Dot state="attention" />
          <span className="flex-1">{attention.text}</span>
          <Button variant="ink" size="sm" onClick={attention.onAction}>
            {attention.actionLabel}
          </Button>
        </div>
      )}
      {sites.length > 0 && (
      <div className="border-t border-line px-1.5 pb-1 pt-1.5">
        <div className={label}>Sites</div>
        {sites.map((s) => (
          <button key={s.name} type="button" onClick={() => onOpenSite(s.name)} className={`group ${item} hover:bg-hover`}>
            <LockClosed12Regular aria-label={s.secure ? "HTTPS" : undefined} aria-hidden={s.secure ? undefined : true} className={`text-text-3 ${s.secure ? "" : "invisible"}`} />
            {s.name}
            <span className="ml-auto text-xs text-text-3">{s.detail}</span>
            <Open16Regular aria-hidden="true" className="text-text-2 opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100" />
          </button>
        ))}
      </div>
      )}
      {services.length > 0 && (
      <div className="border-t border-line px-1.5 pb-1 pt-1.5">
        <div className={label}>Services</div>
        {services.map((s) => {
          const running = s.state === "running";
          return (
            <div key={s.id} className={item}>
              <Dot state={s.state} />
              {s.name}
              <span className={`figures ml-auto text-xs ${s.state === "attention" ? "font-medium text-warn" : "text-text-3"}`}>
                {running ? (
                  <>
                    Running <span className="font-mono">:{s.port}</span>
                  </>
                ) : (
                  "Stopped"
                )}
              </span>
              <Button
                variant="quiet"
                size="sm"
                aria-label={`${running ? "Stop" : "Start"} ${s.name}`}
                icon={running ? <Stop16Filled /> : <Play16Filled />}
                onClick={() => onToggleService(s.id)}
              />
            </div>
          );
        })}
      </div>
      )}
      <div className="flex gap-1.5 border-t border-line px-3 pb-3 pt-2.5">
        <Button variant="ink" className="flex-1 justify-center" onClick={onOpenKiln}>
          Open Kiln
        </Button>
        <Button onClick={onStopAll}>Stop all</Button>
        <Button variant="quiet" onClick={onQuit}>
          Quit
        </Button>
      </div>
    </div>
  );
}
