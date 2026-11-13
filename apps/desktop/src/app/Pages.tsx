import { useEffect, useState } from "react";
import { Page } from "../components/Page";
import { Section } from "../components/Rows";
import { Select } from "../components/Select";
import { SettingRow } from "../components/SettingRow";
import { setStartIn, settingsGet, type AppSettings } from "./api";

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
  useEffect(() => {
    settingsGet().then(setSettings);
  }, []);
  return (
    <Page title="Settings">
      <Section title="Starting and closing">
        <SettingRow
          title="When Kiln starts"
          control={
            settings && (
              <Select
                label="When Kiln starts"
                hideLabel
                value={settings.start_in}
                onChange={(e) => {
                  const value = e.target.value as AppSettings["start_in"];
                  setSettings({ ...settings, start_in: value });
                  setStartIn(value);
                }}
                options={[
                  { value: "window", label: "Open the window" },
                  { value: "tray", label: "Stay in the tray" },
                ]}
              />
            )
          }
        >
          Starting at sign-in always stays in the tray. Closing the window leaves Kiln in the tray; Quit is in the tray's menu.
        </SettingRow>
      </Section>
    </Page>
  );
}
