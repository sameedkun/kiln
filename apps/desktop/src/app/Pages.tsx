import { useEffect, useState } from "react";
import { Page } from "../components/Page";
import { Section } from "../components/Rows";
import { SettingRow } from "../components/SettingRow";
import { Switch } from "../components/Switch";
import { loginItem, onEvent, setStartIn, settingsGet, type AppSettings } from "./api";
import { useChanges } from "./Change";

// Pages whose task hasn't connected them yet (ADR 0054 §5): what they will show, and the
// command that does it until then. Never sample data.
const notYet: Record<string, { title: string; task: string; what: string; command: string }> = {
  sites: { title: "Sites", task: "P4-04", what: "every site, grouped by parked folder, link or proxy, with its PHP, HTTPS and services", command: "kiln sites" },
  services: { title: "Services", task: "P4-05", what: "every service by kind, the catalog to add one, ports, logs and data", command: "kiln service list" },
  php: { title: "PHP", task: "P4-04", what: "PHP versions, updates and extensions", command: "kiln php list" },
  node: { title: "Node", task: "P4-04", what: "Node versions and which sites pin them", command: "kiln node list" },
  logs: { title: "Logs", task: "P4-08", what: "every log, followed live", command: "kiln service logs <name>" },
  history: { title: "History", task: "P4-07", what: "everything Kiln changed, with undo", command: "kiln history" },
};

export function NotYet({ page }: { page: string }) {
  const p = notYet[page];
  if (!p) return null;
  return (
    <Page title={p.title}>
      <p className="m-0 max-w-[60ch] text-[13px] text-text-2">
        This page will show {p.what}. It is connected in {p.task}. Until then, in a terminal: <code className="font-mono text-xs text-text">{p.command}</code>.
      </p>
    </Page>
  );
}

// Settings: the app's own for now; Kiln's settings come with P4-07.
export function Settings() {
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
    </Page>
  );
}
