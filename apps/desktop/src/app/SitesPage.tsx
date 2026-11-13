import { Add16Regular, Delete16Regular, Folder16Regular, Globe20Regular, Link16Regular, LockClosed12Regular, MoreHorizontal16Regular, Open16Regular } from "@fluentui/react-icons";
import { useState, type ReactNode } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Menu } from "../components/Menu";
import { Page } from "../components/Page";
import { Detail, Row, RowName, Section } from "../components/Rows";
import { Switch } from "../components/Switch";
import { Tag } from "../components/Tag";
import { TextField } from "../components/TextField";
import { openFolder, pickFolder, proxyTarget, siteName, type Proxy, type Site, type SitesList } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";

const cols = "minmax(200px,1.4fr) 150px 80px 80px minmax(140px,1fr) 112px";

/** A framework as a word, with its version when Kiln could read it (ADR 0051 §4). */
export function framework(site: Site): string {
  const names: Record<string, string> = {
    laravel: "Laravel",
    symfony: "Symfony",
    drupal: "Drupal",
    craft: "Craft",
    "wordpress-bedrock": "Bedrock",
    wordpress: "WordPress",
    static: "Static",
  };
  const name = names[site.driver] ?? "PHP";
  return site.framework_version ? `${name} ${site.framework_version.split(".").slice(0, 2).join(".")}` : name;
}

const lockIcon = (secure: boolean) => (
  <LockClosed12Regular aria-label={secure ? "HTTPS" : undefined} aria-hidden={secure ? undefined : true} className={`shrink-0 text-text-3 ${secure ? "" : "invisible"}`} />
);

function folderName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? "";
}

// Every site, grouped by how Kiln found it (ADR 0055): a parked folder, a link, a proxy.
export function SitesPage({ go }: { go: (page: string) => void }) {
  const sites = useRead<SitesList>("sites.list");
  const { run, ask, open } = useChanges();
  const [filter, setFilter] = useState("");
  const [adding, setAdding] = useState<null | { kind: "link"; folder: string } | { kind: "proxy" }>(null);

  const match = (text: string) => text.toLowerCase().includes(filter.trim().toLowerCase());
  const shown = (sites.data?.sites ?? []).filter((s) => match(s.host) || match(framework(s)));
  const proxies = (sites.data?.proxies ?? []).filter((p) => match(p.host));
  const total = (sites.data?.sites.length ?? 0) + (sites.data?.proxies.length ?? 0);

  const link = async () => {
    const folder = await pickFolder("Choose a project folder to serve");
    if (folder) setAdding({ kind: "link", folder });
  };
  const park = async () => {
    const folder = await pickFolder("Choose a folder of projects: each folder in it becomes a site");
    if (folder) run(["park", folder], `Parked ${folder}: each project folder in it is a site.`);
  };

  const siteRow = (s: Site) => (
    <Row
      key={s.host}
      columns={cols}
      actions={
        <>
          <Button variant="quiet" aria-label={`Open ${s.host}`} icon={<Open16Regular />} onClick={() => open(s.host)} />
          <Button variant="quiet" aria-label={`Open ${s.host}'s folder`} icon={<Folder16Regular />} onClick={() => openFolder(s.host)} />
          <Menu
            label={`More for ${s.host}`}
            trigger={(props) => <Button variant="quiet" aria-label={`More for ${s.host}`} icon={<MoreHorizontal16Regular />} {...props} />}
            items={[
              { label: "Details", onSelect: () => go(`site:${s.host}`) },
              {
                label: s.secure ? "Serve over plain HTTP" : "Serve over HTTPS",
                onSelect: () => run([s.secure ? "unsecure" : "secure", s.host], `${s.host} is served over ${s.secure ? "HTTP" : "HTTPS"} now.`),
              },
              ...(s.source === "link"
                ? [
                    {
                      label: "Unlink…",
                      icon: <Delete16Regular />,
                      danger: true,
                      groupStart: true,
                      onSelect: () =>
                        ask(`Unlink ${s.host}?`, `Kiln stops serving it. Its folder (${s.project}) stays as it is, and History can undo this.`, "Unlink", ["unlink", s.host], `Unlinked ${s.host}.`),
                    },
                  ]
                : []),
            ]}
          />
        </>
      }
    >
      <RowName before={lockIcon(s.secure)} sub={s.project} mono>
        <button type="button" className="hover:underline" onClick={() => go(`site:${s.host}`)}>
          {s.host}
        </button>
        {s.source === "alias" && (
          <span className="ml-2">
            <Tag>alias</Tag>
          </span>
        )}
      </RowName>
      <Detail>
        <Tag>{framework(s)}</Tag>
      </Detail>
      <Detail>{s.php?.version ? `PHP ${s.php.version.split(".").slice(0, 2).join(".")}` : s.php ? "no PHP" : "—"}</Detail>
      <Detail>{s.node.version ? `Node ${s.node.version.split(".")[0]}` : "—"}</Detail>
      <Detail>{s.in_hosts ? "" : "not in the hosts file"}</Detail>
    </Row>
  );

  const proxyRow = (p: Proxy) => (
    <Row
      key={p.host}
      columns={cols}
      actions={
        <>
          <Button variant="quiet" aria-label={`Open ${p.host}`} icon={<Open16Regular />} onClick={() => open(p.host)} />
          <Button
            variant="quiet"
            aria-label={`Remove ${p.host}`}
            icon={<Delete16Regular />}
            onClick={() => ask(`Remove ${p.host}?`, `Kiln stops sending ${p.host} to ${p.target ?? "its target"}. History can undo this.`, "Remove", ["unproxy", p.host], `Removed ${p.host}.`)}
          />
        </>
      }
    >
      <RowName before={lockIcon(p.url.startsWith("https"))} sub={p.target ? `→ ${p.target}` : undefined} mono>
        {p.host}
      </RowName>
      <Detail>
        <Tag>proxy</Tag>
      </Detail>
      <Detail>—</Detail>
      <Detail>—</Detail>
      <Detail>{p.in_hosts ? "" : "not in the hosts file"}</Detail>
    </Row>
  );

  const parked = sites.data?.parked ?? [];
  return (
    <Page
      title="Sites"
      after={sites.data && <span className="figures text-[13px] text-text-3">{total}</span>}
      actions={
        <Menu
          label="Add a site"
          trigger={(props) => (
            <Button icon={<Add16Regular />} {...props}>
              Add a site
            </Button>
          )}
          items={[
            { label: "Link a folder…", icon: <Link16Regular />, onSelect: link },
            { label: "Park a folder of projects…", icon: <Folder16Regular />, onSelect: park },
            { label: "Proxy to a port…", icon: <Globe20Regular className="size-4" />, groupStart: true, onSelect: () => setAdding({ kind: "proxy" }) },
          ]}
        />
      }
    >
      <TextField label="Find a site" placeholder="Name or framework" value={filter} onChange={(e) => setFilter(e.target.value)} className="mb-6 max-w-[320px]" />
      {sites.error && <p className="text-[13px] text-bad">{sites.error}</p>}
      {sites.data && total === 0 && (
        <p className="m-0 max-w-[60ch] text-[13px] text-text-2">No sites yet. Link a project's folder, park a folder of projects, or proxy a name to a dev server: "Add a site" above.</p>
      )}
      {parked.map((folder) => {
        const inside = shown.filter((s) => s.parked_in === folder);
        return (
          <Section
            key={folder}
            title={`Parked in ${folder}`}
            meta={`${inside.length} · every folder inside is a site`}
            action={
              <Button
                variant="quiet"
                size="sm"
                onClick={() =>
                  ask(`Unpark ${folder}?`, `Kiln stops serving the projects in it. The folder and its projects stay as they are, and History can undo this.`, "Unpark", ["unpark", folder], `Unparked ${folder}.`)
                }
              >
                Unpark
              </Button>
            }
          >
            {inside.length === 0 ? <Empty>{filter ? "None match." : "No project folders in it yet."}</Empty> : inside.map(siteRow)}
          </Section>
        );
      })}
      {shown.some((s) => s.source !== "parked") && (
        <Section title="Linked" meta="folders added one by one">
          {shown.filter((s) => s.source !== "parked").map(siteRow)}
        </Section>
      )}
      {proxies.length > 0 && (
        <Section title="Proxies and tools" meta="served by Kiln, run by something else">
          {proxies.map(proxyRow)}
        </Section>
      )}

      {adding?.kind === "link" && <LinkDialog folder={adding.folder} onClose={() => setAdding(null)} />}
      {adding?.kind === "proxy" && <ProxyDialog onClose={() => setAdding(null)} />}
    </Page>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <li className="border-b border-line px-1 py-4 text-[13px] text-text-2">{children}</li>;
}

function LinkDialog({ folder, onClose }: { folder: string; onClose: () => void }) {
  const { run } = useChanges();
  const [name, setName] = useState(siteName(folderName(folder)) ?? "");
  const [secure, setSecure] = useState(false);
  const [busy, setBusy] = useState(false);
  const valid = siteName(name);
  return (
    <Dialog
      open
      onClose={() => !busy && onClose()}
      title="Link a folder"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="ink"
            disabled={!valid || busy}
            onClick={async () => {
              setBusy(true);
              await run(["link", valid!, "--path", folder, ...(secure ? ["--secure"] : [])], `Linked ${valid}.test.`);
              onClose();
            }}
          >
            {busy ? "Linking…" : "Link"}
          </Button>
        </>
      }
    >
      <p className="mt-0 font-mono text-xs break-all">{folder}</p>
      <TextField label="Site name" value={name} onChange={(e) => setName(e.target.value)} hint={valid ? `Opens as ${valid}.test` : undefined} error={name && !valid ? "Letters, digits and hyphens only." : undefined} />
      <div className="mt-4">
        <Switch checked={secure} onChange={setSecure} label="HTTPS" description="With Kiln's own certificate" />
      </div>
    </Dialog>
  );
}

function ProxyDialog({ onClose }: { onClose: () => void }) {
  const { run } = useChanges();
  const [name, setName] = useState("");
  const [target, setTarget] = useState("");
  const [secure, setSecure] = useState(false);
  const [busy, setBusy] = useState(false);
  const validName = siteName(name);
  const validTarget = proxyTarget(target);
  return (
    <Dialog
      open
      onClose={() => !busy && onClose()}
      title="Proxy to a port"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="ink"
            disabled={!validName || !validTarget || busy}
            onClick={async () => {
              setBusy(true);
              await run(["proxy", validName!, validTarget!, ...(secure ? ["--secure"] : [])], `${validName}.test now goes to ${validTarget}.`);
              onClose();
            }}
          >
            {busy ? "Adding…" : "Add"}
          </Button>
        </>
      }
    >
      <p className="mt-0">A name for a dev server running on this PC, such as Vite or a Node app.</p>
      <div className="grid grid-cols-2 gap-4">
        <TextField label="Name" value={name} onChange={(e) => setName(e.target.value)} hint={validName ? `${validName}.test` : undefined} error={name && !validName ? "Letters, digits and hyphens only." : undefined} />
        <TextField label="Port" mono placeholder="5173" value={target} onChange={(e) => setTarget(e.target.value)} error={target && !validTarget ? "A port, or 127.0.0.1:port." : undefined} />
      </div>
      <div className="mt-4">
        <Switch checked={secure} onChange={setSecure} label="HTTPS" description="With Kiln's own certificate" />
      </div>
    </Dialog>
  );
}
