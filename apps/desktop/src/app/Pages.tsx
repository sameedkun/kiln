import { useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Select } from "../components/Select";
import { TextField } from "../components/TextField";
import { Page } from "../components/Page";
import { Row, Section } from "../components/Rows";
import { SettingRow } from "../components/SettingRow";
import { Switch } from "../components/Switch";
import { loginItem, onEvent, setStartIn, settingsGet, type AppSettings, type Footprint, type KilnSettings, type SetupStatus, type Updates, openReleases } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";

// Settings: the app's own, then Kiln's (ADR 0058 §5).
export function Settings({ setUp }: { setUp: () => void }) {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [item, setItem] = useState<Awaited<ReturnType<typeof loginItem>> | null>(null);
  const { confirm } = useChanges();
  useEffect(() => {
    settingsGet().then(setSettings);
    loginItem().then(setItem);
    let stop: (() => void) | undefined;
    onEvent((note) => note.method === "state.changed" && loginItem().then(setItem)).then((u) => (stop = u));
    return () => stop?.();
  }, []);
  const itemWords = {
    app: "Starts Kiln's app in the tray, and the app starts Kiln's daemon.",
    daemon: "Now only Kiln's daemon starts at sign-in. Turning this on starts the app instead, which starts the daemon.",
    other: "Kiln's login item starts something else. Turning this on replaces it, and History can put it back.",
    none: "Starts Kiln's app in the tray, and the app starts Kiln's daemon.",
  };
  return (
    <Page title="Settings">
      <Section title="Set-up">
        <SettingRow title="Set up Kiln" control={<Button size="sm" onClick={setUp}>Open…</Button>}>
          The first-run steps: the web server, PHP, HTTPS, your projects, the terminal and starting at sign-in. Steps already done say so.
        </SettingRow>
      </Section>
      <Section title="Starting and closing">
        <SettingRow
          title="Start in the tray"
          control={
            settings && (
              <Switch
                hideLabel
                label="Start in the tray"
                checked={settings.start_in === "tray"}
                onChange={(on) => {
                  const value: AppSettings["start_in"] = on ? "tray" : "window";
                  setSettings({ ...settings, start_in: value });
                  setStartIn(value);
                }}
              />
            )
          }
        >
          When you open Kiln, it starts in the background, in the tray, without its window. Starting at sign-in always does. Closing the window leaves Kiln in the tray; Quit
          is in the tray's menu.
        </SettingRow>
        <SettingRow
          title="Start Kiln when I sign in"
          control={
            item && (
              <Switch
                hideLabel
                label="Start Kiln when I sign in"
                checked={item === "app"}
                onChange={(on) =>
                  confirm(
                    on ? "Start Kiln when you sign in?" : "Stop starting Kiln at sign-in?",
                    ["app", on ? "enable" : "disable"],
                    on ? "Kiln now starts in the tray when you sign in." : "Nothing of Kiln's starts at sign-in now.",
                  )
                }
              />
            )
          }
        >
          {item ? itemWords[item] : ""}
        </SettingRow>
      </Section>
      <KilnSection />
      <UpdatesSection />
      <FootprintSection />
    </Page>
  );
}

/** A port the user typed: a number from 1 to 65535, or nothing. */
export function portError(text: string): string | undefined {
  if (!/^[0-9]+$/.test(text)) return "A number, such as 8080";
  const n = Number(text);
  return n >= 1 && n <= 65535 ? undefined : "From 1 to 65535";
}

const MAX_WORKERS = 32;

// Kiln's own settings, from `kiln settings`; each change is a job like any other.
function KilnSection() {
  const settings = useRead<KilnSettings>("settings.get");
  const setup = useRead<SetupStatus>("setup.status");
  const { run } = useChanges();
  const [editing, setEditing] = useState(false);
  const s = settings.data;
  const portsWords = s
    ? `HTTP on ${s.http.port}, HTTPS on ${s.https.port}. ${s.http.auto && s.https.auto ? "Kiln picks them: 80 and 443 when they're free, 8080 and 8443 when another program (such as Herd) has them, and back to 80 and 443 once they're free again." : "You chose them; Kiln keeps them."}`
    : "";
  return (
    <>
    <Section title="Kiln">
      <SettingRow title="Web server ports" control={s && <Button size="sm" onClick={() => setEditing(true)}>Change…</Button>}>
        {portsWords}
      </SettingRow>
      <SettingRow
        title="PHP workers"
        control={
          s && (
            <Select
              hideLabel
              label="PHP workers"
              value={String(s.php_workers)}
              onChange={(e) => run(["php", "workers", e.target.value], `Each PHP version runs ${e.target.value} workers now.`)}
              options={[1, 2, 4, 6, 8, 12, 16, 24, MAX_WORKERS].concat(s.php_workers).filter((n, i, all) => all.indexOf(n) === i).sort((a, b) => a - b).map((n) => ({ value: String(n), label: String(n) }))}
            />
          )
        }
      >
        How many requests each PHP version handles at once. More helps a site that calls itself (an API on the same PC); each uses memory.
      </SettingRow>
      <SettingRow title="Development TLD" control={s && <code className="font-mono text-[13px]">.{s.tld}</code>}>
        Sites are named after their folder with this ending.
      </SettingRow>
    </Section>
      {s && editing && (
        <PortsDialog
          settings={s}
          running={setup.data?.web.running ?? false}
          onClose={() => setEditing(false)}
          apply={async (args) => {
            setEditing(false);
            if (setup.data?.web.running && !(await run(["web", "stop"], "The web server stopped to change its ports."))) return;
            if (await run(["web", "start", ...args], "The web server is running on its new ports.")) return;
            // Refused (a port is taken): nothing was saved, so it starts again where it was.
            if (setup.data?.web.running) await run(["web", "start"], "Those ports couldn't be used: the web server is running on its earlier ones.");
          }}
        />
      )}
    </>
  );
}

function PortsDialog({ settings, running, onClose, apply }: { settings: KilnSettings; running: boolean; onClose: () => void; apply: (args: string[]) => void }) {
  const [auto, setAuto] = useState(settings.http.auto && settings.https.auto);
  const [http, setHttp] = useState(String(settings.http.port));
  const [https, setHttps] = useState(String(settings.https.port));
  const errors = auto ? {} : { http: portError(http), https: portError(https) ?? (http === https ? "Not the same as HTTP" : undefined) };
  const ok = !errors.http && !errors.https;
  return (
    <Dialog
      open
      onClose={onClose}
      title="Web server ports"
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="ink" disabled={!ok} onClick={() => apply(auto ? ["--auto-ports"] : ["--http-port", http, "--https-port", https])}>
            {running ? "Restart on these ports" : "Start on these ports"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Switch checked={auto} onChange={setAuto} label="Let Kiln pick" description="80 and 443 when they're free; 8080 and 8443 when not" />
        {!auto && (
          <div className="flex gap-4">
            <TextField label="HTTP" mono inputMode="numeric" value={http} onChange={(e) => setHttp(e.target.value.trim())} error={errors.http} />
            <TextField label="HTTPS" mono inputMode="numeric" value={https} onChange={(e) => setHttps(e.target.value.trim())} error={errors.https} />
          </div>
        )}
        <p className="m-0 text-[13px] text-text-2">
          {running ? "The web server stops and starts again on these ports: a second or two in which sites don't answer." : "The web server starts on these ports."} Sites' addresses
          carry the port when it isn't 80 or 443.
        </p>
      </div>
    </Dialog>
  );
}

/** Bytes in words: 85.9 MB, 1.2 GB. */
export function size(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

// What Kiln uses on this PC (ADR 0063): its processes' memory and CPU, its folders on disk,
// and the one part that is safe to clear.
function FootprintSection() {
  const footprint = useRead<Footprint>("footprint.get");
  const { ask } = useChanges();
  const f = footprint.data;
  return (
    <>
      <Section title="Memory and CPU" meta={f ? `${size(f.memory_bytes)} together` : "measuring…"} action={<Button variant="quiet" size="sm" onClick={footprint.reload}>Measure again</Button>}>
        {f && f.processes.length === 0 && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">No Kiln process is running.</li>}
        {f?.processes.map((p) => (
          <Row key={p.label} columns="minmax(200px,1fr) 110px 90px">
            <span className="text-[13px]">
              {p.label}
              {p.count > 1 && <span className="figures ml-1.5 text-text-3">×{p.count}</span>}
            </span>
            <span className="figures text-right text-[12.5px]">{size(p.memory_bytes)}</span>
            <span className="figures text-right text-[12.5px] text-text-2">{p.cpu_percent.toFixed(1)}% CPU</span>
          </Row>
        ))}
      </Section>
      <Section
        title="On disk"
        meta={f ? `${size(f.disk_bytes)} together` : undefined}
        action={
          f && f.cache_bytes > 0 ? (
            <Button
              variant="quiet"
              size="sm"
              onClick={() =>
                ask(
                  "Clear the download cache?",
                  `It holds verified copies of what Kiln has already installed (${size(f.cache_bytes)}). Nothing installed changes; Kiln downloads again what it needs later.`,
                  "Clear it",
                  ["cache", "clear"],
                  "The download cache is cleared.",
                )
              }
            >
              Clear the download cache
            </Button>
          ) : undefined
        }
      >
        {f?.folders.map((d) => (
          <Row key={d.label} columns="minmax(200px,1fr) 110px">
            <span className="min-w-0">
              <span className="block text-[13px]">{d.label}</span>
              <span className="block truncate font-mono text-[11.5px] text-text-3" title={d.path}>
                {d.path}
              </span>
            </span>
            <span className="figures text-right text-[12.5px]">{size(d.bytes)}</span>
          </Row>
        ))}
      </Section>
    </>
  );
}

// Updates (ADR 0064): PHP's patches are on the PHP page; here, whether Kiln may ask GitHub for a
// newer Kiln, off until the user turns it on.
function UpdatesSection() {
  const updates = useRead<Updates>("updates.check");
  const { run } = useChanges();
  const k = updates.data?.kiln;
  const status = !k
    ? ""
    : !k.checking
      ? `This is Kiln ${k.version}.`
      : k.error
        ? `This is Kiln ${k.version}. GitHub couldn't be reached: ${k.error}`
        : k.update
          ? `Kiln ${k.latest} is out; this is ${k.version}.`
          : `Kiln ${k.version} is the newest.`;
  return (
    <Section title="Updates">
      <SettingRow
        title="Check GitHub for a new Kiln"
        control={
          k && (
            <>
              {k.update && (
                <Button size="sm" onClick={() => openReleases()}>
                  See Kiln {k.latest}
                </Button>
              )}
              <Switch
                hideLabel
                label="Check GitHub for a new Kiln"
                checked={k.checking}
                onChange={(on) => run(["updates", "check-kiln", on ? "on" : "off"], on ? "Kiln checks GitHub for new releases now." : "Kiln doesn't check GitHub any more.")}
              />
            </>
          )
        }
      >
        Kiln asks GitHub for its newest release, sending nothing about you or this PC. {status}
      </SettingRow>
    </Section>
  );
}
