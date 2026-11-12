import { Add16Regular, Folder16Regular, Globe20Regular, Link16Regular } from "@fluentui/react-icons";
import { useState } from "react";
import { Button } from "../../components/Button";
import { Menu } from "../../components/Menu";
import { Page } from "../../components/Page";
import { Detail, Row, RowName, Section } from "../../components/Rows";
import { Tag } from "../../components/Tag";
import { TextField } from "../../components/TextField";
import { siteGroups } from "../sample";
import { lock, noLock, SiteActions, type Go } from "./common";

const cols = "minmax(200px,1.4fr) 110px 70px minmax(160px,1.4fr) 112px";

// Every site, grouped by how Kiln found it: a parked folder, a link, or a proxy.
export function Sites({ go }: { go: Go }) {
  const [filter, setFilter] = useState("");
  const groups = siteGroups
    .map((g) => ({ ...g, sites: g.sites.filter((s) => s.name.includes(filter.toLowerCase()) || (s.framework ?? "").toLowerCase().includes(filter.toLowerCase())) }))
    .filter((g) => g.sites.length > 0);
  return (
    <Page
      title="Sites"
      after={<span className="figures text-[13px] text-text-3">8</span>}
      actions={
        <Menu
          label="Add a site"
          trigger={(props) => (
            <Button icon={<Add16Regular />} {...props}>
              Add a site
            </Button>
          )}
          items={[
            { label: "Link a folder…", icon: <Link16Regular />, onSelect: () => {} },
            { label: "Park a folder of projects…", icon: <Folder16Regular />, onSelect: () => {} },
            { label: "New Laravel project…", icon: <Add16Regular />, onSelect: () => {} },
            { label: "Proxy to a port…", icon: <Globe20Regular />, groupStart: true, onSelect: () => {} },
          ]}
        />
      }
    >
      <TextField label="Find a site" placeholder="Name or framework" value={filter} onChange={(e) => setFilter(e.target.value)} className="mb-6 max-w-[320px]" />
      {groups.map((g) => (
        <Section key={g.title} title={g.title} meta={`${g.sites.length} · ${g.meta}`}>
          {g.sites.map((s) => (
            <Row key={s.name} columns={cols} actions={<SiteActions name={s.name} folder={Boolean(s.folder)} />}>
              <RowName before={s.secure ? lock : noLock} sub={s.folder ?? (s.proxy ? `→ ${s.proxy}` : s.note)} mono={Boolean(s.folder ?? s.proxy)}>
                {s.folder ? (
                  <button type="button" className="hover:underline" onClick={() => go(`site:${s.name}`)}>
                    {s.name}
                  </button>
                ) : (
                  s.name
                )}
                {s.proxy && (
                  <span className="ml-2">
                    <Tag>proxy</Tag>
                  </span>
                )}
              </RowName>
              <Detail>{s.framework ? <Tag>{s.framework}</Tag> : "—"}</Detail>
              <Detail>{s.php ? `PHP ${s.php}` : "—"}</Detail>
              <Detail>{s.proxy ? s.note : s.services.join(" · ") || "no services"}</Detail>
            </Row>
          ))}
        </Section>
      ))}
      {groups.length === 0 && <p className="text-[13px] text-text-2">No site matches “{filter}”.</p>}
    </Page>
  );
}
