import { ArrowSync16Regular, Broom16Regular } from "@fluentui/react-icons";
import { useState } from "react";
import { Button } from "../../components/Button";
import { Meter } from "../../components/Meter";
import { Page } from "../../components/Page";
import { Row, Section } from "../../components/Rows";
import { Select } from "../../components/Select";
import { SettingRow } from "../../components/SettingRow";
import { Status } from "../../components/Status";
import { Switch } from "../../components/Switch";
import { health, usage } from "../sample";

// Settings, with the machine's health and Kiln's footprint on it.
export function Settings() {
  const [atSignIn, setAtSignIn] = useState(true);
  const [checkUpdates, setCheckUpdates] = useState(false);
  const [mcp, setMcp] = useState(false);
  return (
    <Page title="Settings">
      <Section title="Starting and closing">
        <SettingRow
          title="When Kiln starts"
          control={<Select label="When Kiln starts" hideLabel defaultValue="tray" options={[{ value: "tray", label: "Stay in the tray" }, { value: "window", label: "Open the window" }]} />}
        >
          Starting at sign-in always stays in the tray.
        </SettingRow>
        <SettingRow title="Start Kiln when I sign in" control={<Switch hideLabel label="Start Kiln when I sign in" checked={atSignIn} onChange={setAtSignIn} />}>
          Starts the app and keeps your sites and services running. Closing the window leaves Kiln in the tray.
        </SettingRow>
        <SettingRow
          title="Theme"
          control={<Select label="Theme" hideLabel defaultValue="system" options={[{ value: "system", label: "Same as Windows" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} />}
        />
      </Section>

      <Section
        title="This PC"
        meta="from kiln doctor"
        action={
          <Button variant="quiet" size="sm" icon={<ArrowSync16Regular />}>
            Check again
          </Button>
        }
      >
        {health.map((h) => (
          <Row key={h.text} columns="minmax(220px,1fr) auto" actions={h.state === "attention" ? <Button size="sm">Update</Button> : undefined}>
            <span className="flex items-start gap-3">
              <span className="w-[92px] shrink-0 pt-0.5">
                <Status state={h.state} label={h.state === "running" ? "OK" : "Out of date"} />
              </span>
              <span>
                {h.text}
                <span className="block text-xs text-text-3">{h.detail}</span>
              </span>
            </span>
          </Row>
        ))}
      </Section>

      <Section title="Kiln's footprint">
        <li className="grid grid-cols-2 gap-x-10 gap-y-5 border-b border-line px-1 py-4">
          <Meter label="Memory" {...usage.memory} />
          <div>
            <Meter label="Disk" {...usage.disk} />
            <Button size="sm" className="mt-2" icon={<Broom16Regular />}>
              Clear downloads (480 MB)
            </Button>
          </div>
          <div className="col-span-2">
            <div className="mb-1 text-xs font-medium text-text-3">Using the most memory</div>
            <ul className="m-0 grid list-none grid-cols-4 gap-4 p-0 text-[13px]">
              {usage.top.map((t) => (
                <li key={t.name}>
                  {t.name}
                  <span className="figures block text-xs text-text-3">{t.memory}</span>
                </li>
              ))}
            </ul>
          </div>
        </li>
      </Section>

      <Section title="Updates">
        <SettingRow title="Kiln 0.1.0" control={<Button size="sm" icon={<ArrowSync16Regular />}>Check now</Button>}>
          PHP and services are checked against Kiln's pinned list; Kiln itself is checked on GitHub.
        </SettingRow>
        <SettingRow title="Check for a new Kiln once a week" control={<Switch hideLabel label="Check for a new Kiln once a week" checked={checkUpdates} onChange={setCheckUpdates} />}>
          Off by default: the check asks GitHub, and sends nothing about you or your sites.
        </SettingRow>
      </Section>

      <Section title="AI assistants">
        <SettingRow title="Let AI assistants use Kiln (MCP)" control={<Switch hideLabel label="Let AI assistants use Kiln (MCP)" checked={mcp} onChange={setMcp} />}>
          Claude Code, Cursor and others can list your sites, read logs, start services and switch PHP. Every change they make shows in History and can be undone.
        </SettingRow>
      </Section>

      <Section title="Advanced">
        <SettingRow title="Domain for sites" control={<Select label="Domain for sites" hideLabel defaultValue="test" options={[{ value: "test", label: ".test" }, { value: "localhost", label: ".localhost" }]} />}>
          Changing it renames every site; History can undo it.
        </SettingRow>
        <SettingRow title="Kiln's folders" control={<Button size="sm">Open</Button>}>
          <span className="font-mono text-xs">%LOCALAPPDATA%\Kiln · %APPDATA%\Kiln</span>
        </SettingRow>
      </Section>
    </Page>
  );
}
