import { ArrowLeft16Regular, Folder16Regular, Open16Regular } from "@fluentui/react-icons";
import { Button } from "../components/Button";
import { Notice } from "../components/Notice";
import { Page } from "../components/Page";
import { Row, Section } from "../components/Rows";
import { Select } from "../components/Select";
import { Switch } from "../components/Switch";
import { Tag } from "../components/Tag";
import { openFolder, openSite, type PhpList, type Site, type SitesList } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";
import { framework } from "./SitesPage";

const minor = (version: string) => version.split(".").slice(0, 2).join(".");

/** Where a site's PHP comes from, in words. */
function phpWhy(site: Site, defaultVersion: string | null): string {
  const php = site.php;
  if (!php) return "A static site: no PHP.";
  if (!php.version) return "No PHP is installed yet: install one on the PHP page.";
  if (php.pinned && php.fell_back) return `It asks for ${php.pinned}, which isn't installed, so it uses the default.`;
  if (php.pinned && php.from_project) return `Its kiln.toml asks for ${php.pinned}.`;
  if (php.pinned) return `Pinned to ${php.pinned} on this PC.`;
  return `The default${defaultVersion ? ` (${minor(defaultVersion)})` : ""}.`;
}

/** Where a site's Node comes from, in words. */
function nodeWhy(site: Site): string {
  const node = site.node;
  if (!node.version) return node.asks_for ? `${node.asked_by} asks for ${node.asks_for}; Kiln has no Node installed.` : "Kiln has no Node installed.";
  if (node.asked_by && node.fell_back) return `${node.asked_by} asks for ${node.asks_for}, which isn't installed, so the default.`;
  if (node.asked_by) return `${node.asked_by} asks for ${node.asks_for}.`;
  return "The default: no kiln.toml, .nvmrc or .node-version asks for another.";
}

// One site (ADR 0055): how it runs, where it is, and what can change.
export function SitePage({ host, go }: { host: string; go: (page: string) => void }) {
  const sites = useRead<SitesList>("sites.list");
  const php = useRead<PhpList>("php.list");
  const { run, ask, confirm } = useChanges();
  const site = sites.data?.sites.find((s) => s.host === host);

  const back = <Button variant="quiet" aria-label="Back to Sites" icon={<ArrowLeft16Regular />} onClick={() => go("sites")} />;
  if (sites.data && !site) {
    return (
      <Page title={host} leading={back}>
        <p className="m-0 text-[13px] text-text-2">Kiln doesn't serve {host} any more.</p>
      </Page>
    );
  }
  if (!site) return <Page title={host} leading={back}>{null}</Page>;

  const lines = [...new Set((php.data?.versions ?? []).filter((v) => v.installed).map((v) => minor(v.version)))];
  const current = site.php?.pinned && !site.php.fell_back ? minor(site.php.pinned) : "default";
  const facts: [string, string][] = [
    ["Address", site.url],
    ["Folder", site.project],
    ["Served from", site.document_root],
    ["Found", site.source === "parked" ? `in the parked folder ${site.parked_in}` : site.source === "link" ? "linked on its own" : "an alias in its kiln.toml"],
  ];

  return (
    <Page
      title={site.host}
      leading={back}
      after={<Tag>{framework(site)}</Tag>}
      actions={
        <>
          <Button variant="quiet" aria-label="Open the folder" title="Open the folder" icon={<Folder16Regular />} onClick={() => openFolder(site.host)} />
          <Button icon={<Open16Regular />} onClick={() => openSite(site.host)}>
            Open {site.host}
          </Button>
        </>
      }
    >
      {!site.in_hosts && (
        <Notice tone="attention" title={`${site.host} isn't in the hosts file`} actions={<Button variant="ink" onClick={() => confirm("Update the hosts file?", ["hosts", "sync"], "The hosts file has every site now.")}>Add it</Button>}>
          Browsers can't find it by name until it is. Windows asks for administrator approval.
        </Notice>
      )}

      <Section title="How it runs">
        <li className="flex flex-wrap items-start gap-x-10 gap-y-4 border-b border-line px-1 py-4">
          {site.php ? (
            <div>
              <Select
                label="PHP"
                value={current}
                disabled={site.source === "alias" || lines.length === 0}
                onChange={(e) => {
                  const v = e.target.value;
                  if (v === "default") run(["unisolate", site.host], `${site.host} uses the default PHP now.`);
                  else run(["isolate", v, site.host], `${site.host} uses PHP ${v} now.`);
                }}
                options={[{ value: "default", label: "Default" }, ...lines.map((l) => ({ value: l, label: `PHP ${l}` }))]}
              />
              <p className="m-0 mt-1.5 max-w-[34ch] text-xs text-text-3">
                {site.php.version ? `Runs ${site.php.version}. ` : ""}
                {phpWhy(site, php.data?.default ?? null)}
                {site.source === "alias" ? " An alias follows its main site." : ""}
              </p>
            </div>
          ) : (
            <p className="m-0 text-[13px] text-text-2">A static site: no PHP.</p>
          )}
          <div>
            <div className="text-[13px] text-text-2">Node</div>
            <div className="figures mt-1 text-sm font-medium">{site.node.version ?? "none"}</div>
            <p className="m-0 mt-1 max-w-[40ch] text-xs text-text-3">{nodeWhy(site)}</p>
          </div>
          <div className="w-[240px]">
            <Switch
              checked={site.secure}
              onChange={(on) => run([on ? "secure" : "unsecure", site.host], `${site.host} is served over ${on ? "HTTPS" : "HTTP"} now.`)}
              label="HTTPS"
              description="With Kiln's own certificate"
            />
          </div>
        </li>
      </Section>

      <Section title="Where it is">
        {facts.map(([label, value]) => (
          <Row key={label} columns="140px 1fr">
            <span className="text-[13px] text-text-2">{label}</span>
            <span className="font-mono text-xs break-all">{value}</span>
          </Row>
        ))}
      </Section>

      <p className="m-0 mb-6 max-w-[64ch] px-1 text-[13px] text-text-3">
        Its services and <code className="font-mono">.env</code> come with the Services page (P4-05), its logs with the log viewer (P4-08), and its requests with P4-18.
      </p>

      {site.source === "link" && (
        <Button
          onClick={() =>
            ask(`Unlink ${site.host}?`, `Kiln stops serving it. Its folder (${site.project}) stays as it is, and History can undo this.`, "Unlink", ["unlink", site.host], `Unlinked ${site.host}.`)
          }
        >
          Unlink {site.host}
        </Button>
      )}
      {site.source === "parked" && <p className="m-0 px-1 text-[13px] text-text-2">It's served because its folder is in {site.parked_in}. Moving it out, or unparking that folder, stops it.</p>}
    </Page>
  );
}
