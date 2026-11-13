import { useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Select } from "../components/Select";
import { TextField } from "../components/TextField";
import { Page } from "../components/Page";
import { Section } from "../components/Rows";
import { SettingRow } from "../components/SettingRow";
import { Switch } from "../components/Switch";
import { loginItem, onEvent, setStartIn, settingsGet, type AppSettings, type KilnSettings, type SetupStatus } from "./api";
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
      {s && editing && (
        <PortsDialog
          settings={s}
          running={setup.data?.web.running ?? false}
          onClose={() => setEditing(false)}
          apply={async (args) => {
            setEditing(false);
            if (setup.data?.web.running && !(await run(["web", "stop"], "The web server stopped to change its ports."))) return;
            await run(["web", "start", ...args], "The web server is running on its new ports.");
          }}
        />
      )}
    </Section>
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
