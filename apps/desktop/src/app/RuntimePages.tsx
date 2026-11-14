import { Add16Regular, ArrowSync16Regular, Delete16Regular, MoreHorizontal16Regular } from "@fluentui/react-icons";
import { useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Menu } from "../components/Menu";
import { Meter } from "../components/Meter";
import { Page } from "../components/Page";
import { Detail, Row, RowName, Section } from "../components/Rows";
import { Select } from "../components/Select";
import { Status } from "../components/Status";
import { Switch } from "../components/Switch";
import { Tag } from "../components/Tag";
import { call, message, type JobEvent, type NodeAvailable, type NodeList, type PhpAvailable, type PhpExtensions, type PhpList, type SitesList, type Updates } from "./api";
import { useChanges } from "./Change";
import { fewNames, useRead } from "./hooks";

const minor = (version: string) => version.split(".").slice(0, 2).join(".");
const mb = (bytes: number) => `${(bytes / 1_048_576).toFixed(1)} MB`;
const cols = "minmax(170px,1fr) minmax(180px,1.4fr) 200px 40px";

/** A download's progress from a job's `progress` lines (ADR 0055 §4). */
export function useProgress() {
  const [progress, setProgress] = useState<{ label: string; done: number; total: number | null } | null>(null);
  const onLine = (e: JobEvent) => {
    if (e.type === "progress") setProgress({ label: e.label ?? "Downloading", done: e.done ?? 0, total: e.total ?? null });
  };
  return { progress, onLine, clear: () => setProgress(null) };
}

function ProgressLine({ progress }: { progress: ReturnType<typeof useProgress>["progress"] }) {
  if (!progress) return null;
  return (
    <div className="mt-4">
      <Meter
        label={progress.label}
        value={progress.done}
        max={progress.total ?? Math.max(progress.done, 1)}
        display={progress.total ? `${mb(progress.done)} of ${mb(progress.total)}` : mb(progress.done)}
      />
    </div>
  );
}

interface Offer {
  line: string;
  version: string;
  detail: string;
  installed: boolean;
}

/** What can be installed, read from the network only when the user asks (ADR 0053 §4). */
function InstallDialog({ what, load, args, onClose }: { what: string; load: () => Promise<Offer[]>; args: (o: Offer) => string[]; onClose: () => void }) {
  const { run } = useChanges();
  const { progress, onLine } = useProgress();
  const [offers, setOffers] = useState<Offer[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  // Once, when the dialog opens: it asks the network.
  const [first] = useState(() => load);
  useEffect(() => {
    first().then(setOffers, (e) => setError(message(e)));
  }, [first]);
  return (
    <Dialog
      open
      wide
      onClose={() => !busy && onClose()}
      title={`Install ${what}`}
      footer={
        <Button onClick={onClose} disabled={busy !== null}>
          Close
        </Button>
      }
    >
      {!offers && !error && <p className="m-0">Asking what's available…</p>}
      {error && <p className="m-0 text-text">{error}</p>}
      {offers && (
        <ul className="m-0 max-h-[360px] list-none overflow-auto p-0">
          {offers.map((o) => (
            <Row key={o.version} columns="minmax(120px,1fr) minmax(120px,1fr) 100px">
              <RowName sub={o.detail}>
                <span className="figures">
                  {what} {o.version}
                </span>
              </RowName>
              <Detail>{o.installed ? "installed" : ""}</Detail>
              <span className="flex justify-end">
                <Button
                  size="sm"
                  disabled={o.installed || busy !== null}
                  onClick={async () => {
                    setBusy(o.version);
                    await run(args(o), `Installed ${what} ${o.version}.`, onLine);
                    setBusy(null);
                    onClose();
                  }}
                >
                  {busy === o.version ? "Installing…" : "Install"}
                </Button>
              </span>
            </Row>
          ))}
        </ul>
      )}
      <ProgressLine progress={progress} />
    </Dialog>
  );
}

// PHP (ADR 0055): the versions Kiln keeps, which sites use each, and their extensions.
export function PhpPage() {
  const php = useRead<PhpList>("php.list");
  // Patch releases from windows.php.net (ADR 0064); none shown when it can't be reached.
  const updates = useRead<Updates>("updates.check");
  const newest = (version: string) => updates.data?.php.find((u) => u.version === version && u.update)?.newest;
  const sites = useRead<SitesList>("sites.list");
  const { run, ask } = useChanges();
  const { progress, onLine, clear } = useProgress();
  const [installing, setInstalling] = useState(false);
  const versions = php.data?.versions ?? [];
  const [chosen, setChosen] = useState<string | null>(null);
  const shownFor = chosen ?? (php.data?.default ? minor(php.data.default) : versions[0] ? minor(versions[0].version) : null);
  const usedBy = (version: string) => (sites.data?.sites ?? []).filter((s) => s.php?.version === version).map((s) => s.host);

  return (
    <Page title="PHP" actions={<Button icon={<Add16Regular />} onClick={() => setInstalling(true)}>Install a version</Button>}>
      {php.error && <p className="text-[13px] text-bad">{php.error}</p>}
      <Section title="Versions" meta="Kiln's own builds; a site uses the default unless it picks one">
        {php.data && versions.length === 0 && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">No PHP yet: "Install a version" above.</li>}
        {versions.map((v) => {
          const isDefault = php.data?.default === v.version;
          const users = usedBy(v.version);
          return (
            <Row
              key={v.version}
              columns={cols}
              actions={
                <Menu
                  label={`More for PHP ${v.version}`}
                  trigger={(props) => <Button variant="quiet" aria-label={`More for PHP ${v.version}`} icon={<MoreHorizontal16Regular />} {...props} />}
                  items={[
                    ...(isDefault ? [] : [{ label: "Make it the default", onSelect: () => run(["php", "default", v.version], `PHP ${v.version} serves sites by default now.`) }]),
                    { label: "Extensions", onSelect: () => setChosen(minor(v.version)) },
                    ...(v.xdebug_available
                      ? [
                          {
                            label: v.xdebug ? "Turn Xdebug off" : "Turn Xdebug on",
                            onSelect: () =>
                              run(
                                ["php", "xdebug", v.xdebug ? "off" : "on", "--php", v.version],
                                v.xdebug ? `Xdebug is off for PHP ${minor(v.version)}.` : `Xdebug is on for PHP ${minor(v.version)}: requests with its cookie are debugged.`,
                              ),
                          },
                        ]
                      : []),
                    {
                      label: "Update to its newest patch",
                      icon: <ArrowSync16Regular />,
                      onSelect: async () => {
                        await run(["php", "update", minor(v.version)], `PHP ${minor(v.version)} is up to date.`, onLine);
                        clear();
                      },
                    },
                    {
                      label: "Remove…",
                      icon: <Delete16Regular />,
                      danger: true,
                      groupStart: true,
                      onSelect: () =>
                        ask(
                          `Remove PHP ${v.version}?`,
                          users.length ? `${users.join(", ")} use it; they will use the default instead.` : "No site uses it.",
                          "Remove",
                          ["php", "remove", v.version],
                          `Removed PHP ${v.version}.`,
                        ),
                    },
                  ]}
                />
              }
            >
              <RowName sub={v.system ? "your distribution's" : undefined}>
                <span className="figures">PHP {v.version}</span>
                {isDefault && (
                  <span className="ml-2">
                    <Tag>default</Tag>
                  </span>
                )}
                {newest(v.version) && (
                  <span className="ml-2">
                    <Tag>{newest(v.version)} available</Tag>
                  </span>
                )}
                {v.xdebug && (
                  <span className="ml-2">
                    <Tag>Xdebug</Tag>
                  </span>
                )}
              </RowName>
              <Detail title={users.join(", ")}>{users.length ? `used by ${fewNames(users)}` : "no site uses it"}</Detail>
              <span className="flex justify-end">{v.installed ? <Status state="running" label="Installed" /> : <Status state="attention" label="Missing on disk" />}</span>
            </Row>
          );
        })}
      </Section>
      {versions.some((v) => v.xdebug) && (
        <Section title="Debugging" meta="Xdebug on demand">
          <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">
            A request with the <code className="font-mono text-xs text-text">XDEBUG_SESSION</code> cookie (set by your browser's Xdebug helper) or{" "}
            <code className="font-mono text-xs text-text">?XDEBUG_SESSION_START=1</code> goes to a debug worker, which connects to your editor on{" "}
            <code className="font-mono text-xs text-text">127.0.0.1:9003</code>. Every other request stays on the fast workers.
          </li>
        </Section>
      )}
      <ProgressLine progress={progress} />
      {shownFor && <Extensions php={shownFor} lines={[...new Set(versions.map((v) => minor(v.version)))]} onChoose={setChosen} />}
      {installing && (
        <InstallDialog
          what="PHP"
          onClose={() => setInstalling(false)}
          load={async () =>
            (await call<PhpAvailable>("php.available")).releases.map((r) => ({ line: r.line, version: r.version, detail: r.archive, installed: r.installed }))
          }
          args={(o) => ["php", "install", o.version]}
        />
      )}
    </Page>
  );
}

function Extensions({ php, lines, onChoose }: { php: string; lines: string[]; onChoose: (line: string) => void }) {
  const ext = useRead<PhpExtensions>("php.extensions", { php });
  const { run } = useChanges();
  return (
    <Section
      title={`Extensions in PHP ${php}`}
      meta="a change restarts that version's workers; History can undo it"
      action={lines.length > 1 ? <Select label="PHP version" hideLabel value={php} onChange={(e) => onChoose(e.target.value)} options={lines.map((l) => ({ value: l, label: `PHP ${l}` }))} /> : undefined}
    >
      {ext.error && <li className="px-1 py-4 text-[13px] text-bad">{ext.error}</li>}
      {ext.data?.static_build && <li className="px-1 py-4 text-[13px] text-text-2">This build has its extensions compiled in: they can't be turned on or off.</li>}
      {ext.data && ext.data.toggles.length > 0 && (
        <li className="grid grid-cols-3 gap-x-10 gap-y-3 border-b border-line px-1 py-4">
          {ext.data.toggles.map((t) => (
            <Switch key={t.name} label={t.name} checked={t.on} onChange={(on) => run(["php", "ext", on ? "on" : "off", t.name, "--php", php], `${t.name} is ${on ? "on" : "off"} in PHP ${php}.`)} />
          ))}
        </li>
      )}
      {ext.data && ext.data.built_in.length > 0 && (
        <li className="px-1 py-3 text-xs text-text-3">
          Built in, always on: <span className="font-mono">{ext.data.built_in.join(", ")}</span>
        </li>
      )}
      {/* PECL extensions Kiln pins (ADR 0067): added once, then a switch like the others. */}
      {ext.data && !ext.data.static_build && !ext.data.toggles.some((t) => t.name === "redis") && (
        <li className="flex items-center justify-between gap-4 border-t border-line px-1 py-3">
          <span className="text-[13px] text-text-2">
            <span className="font-medium text-text">phpredis</span>: a fast Redis client from PECL, checked against Kiln's pin. Laravel then uses{" "}
            <code className="font-mono text-xs">REDIS_CLIENT=phpredis</code>.
          </span>
          <Button size="sm" onClick={() => run(["php", "ext", "add", "redis", "--php", php], `phpredis is on in PHP ${php}.`)}>
            Add phpredis
          </Button>
        </li>
      )}
    </Section>
  );
}

// Node (ADR 0055): the versions Kiln keeps, and which sites run each.
export function NodePage() {
  const node = useRead<NodeList>("node.list");
  const sites = useRead<SitesList>("sites.list");
  const { run, ask } = useChanges();
  const [installing, setInstalling] = useState(false);
  const versions = node.data?.versions ?? [];
  const usedBy = (version: string) => (sites.data?.sites ?? []).filter((s) => s.node.version === version).map((s) => s.host);
  return (
    <Page title="Node" actions={<Button icon={<Add16Regular />} onClick={() => setInstalling(true)}>Install a version</Button>}>
      {node.error && <p className="text-[13px] text-bad">{node.error}</p>}
      <Section title="Versions" meta="from nodejs.org, signatures checked; a project picks one with .nvmrc, .node-version or kiln.toml">
        {node.data && versions.length === 0 && <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">No Node yet: "Install a version" above.</li>}
        {versions.map((v) => {
          const isDefault = node.data?.default === v.version;
          const users = usedBy(v.version);
          return (
            <Row
              key={v.version}
              columns={cols}
              actions={
                <Menu
                  label={`More for Node ${v.version}`}
                  trigger={(props) => <Button variant="quiet" aria-label={`More for Node ${v.version}`} icon={<MoreHorizontal16Regular />} {...props} />}
                  items={[
                    ...(isDefault ? [] : [{ label: "Make it the default", onSelect: () => run(["node", "default", v.version], `Node ${v.version} is the default now.`) }]),
                    {
                      label: "Remove…",
                      icon: <Delete16Regular />,
                      danger: true,
                      groupStart: true,
                      onSelect: () =>
                        ask(
                          `Remove Node ${v.version}?`,
                          `Its folder goes, with any packages installed globally in it.${users.length ? ` ${users.join(", ")} use it.` : ""}`,
                          "Remove",
                          ["node", "remove", v.version],
                          `Removed Node ${v.version}.`,
                        ),
                    },
                  ]}
                />
              }
            >
              <RowName sub={v.lts ? `LTS ${v.lts}` : undefined}>
                <span className="figures">Node {v.version}</span>
                {isDefault && (
                  <span className="ml-2">
                    <Tag>default</Tag>
                  </span>
                )}
              </RowName>
              <Detail>{users.length ? `runs ${users.join(", ")}` : "no site runs it"}</Detail>
              <span className="flex justify-end">
                <Status state="running" label="Installed" />
              </span>
            </Row>
          );
        })}
      </Section>
      {installing && (
        <InstallDialog
          what="Node"
          onClose={() => setInstalling(false)}
          load={async () =>
            (await call<NodeAvailable>("node.available")).releases.map((r) => ({
              line: String(r.line),
              version: r.version,
              detail: `${r.lts ? `LTS ${r.lts} · ` : ""}${r.date}`,
              installed: r.installed,
            }))
          }
          args={(o) => ["node", "install", o.version]}
        />
      )}
    </Page>
  );
}
