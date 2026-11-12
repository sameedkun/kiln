import { Add16Regular, ArrowSync16Regular, Delete16Regular } from "@fluentui/react-icons";
import { useState } from "react";
import { Button } from "../../components/Button";
import { Page } from "../../components/Page";
import { Detail, Row, RowName, Section } from "../../components/Rows";
import { Status } from "../../components/Status";
import { Switch } from "../../components/Switch";
import { Tag } from "../../components/Tag";
import { extensions, nodeVersions, phpVersions } from "../sample";
import { More } from "./common";

const cols = "minmax(150px,1fr) minmax(170px,1.3fr) 220px 30px";

// PHP: the versions Kiln keeps, which sites use each, updates, and extensions per version.
export function Php() {
  const [chosen, setChosen] = useState("8.4");
  const [on, setOn] = useState(Object.fromEntries(extensions.map((e) => [e.name, e.on])));
  return (
    <Page
      title="PHP"
      actions={
        <Button icon={<Add16Regular />} >
          Install a version
        </Button>
      }
    >
      <Section title="Versions" meta="Kiln's own builds; each site uses the default unless it picks one">
        {phpVersions.map((v) => (
          <Row
            key={v.version}
            columns={cols}
            actions={
              <More
                name={`PHP ${v.line}`}
                items={[
                  { label: "Make it the default", onSelect: () => {} },
                  { label: "Extensions", onSelect: () => setChosen(v.line) },
                  { label: "Remove…", icon: <Delete16Regular />, danger: true, groupStart: true, onSelect: () => {} },
                ]}
              />
            }
          >
            <RowName sub={`${v.extensions} extensions on`}>
              <span className="figures">PHP {v.version}</span>
              {v.isDefault && (
                <span className="ml-2">
                  <Tag>default</Tag>
                </span>
              )}
            </RowName>
            <Detail>{v.sites.length ? `used by ${v.sites.join(", ")}` : "no sites use it"}</Detail>
            <span className="flex items-center justify-end gap-2">
              {v.update ? (
                <>
                  <Status state="attention" label={`${v.update} is out`} />
                  <Button size="sm" icon={<ArrowSync16Regular />}>
                    Update
                  </Button>
                </>
              ) : (
                <Status state="running" label="Up to date" />
              )}
            </span>
          </Row>
        ))}
      </Section>

      <Section title={`Extensions in PHP ${chosen}`} meta="a change restarts that version's workers; History can undo it">
        <li className="grid grid-cols-3 gap-x-10 gap-y-3 px-1 py-4">
          {extensions.map((e) => (
            <Switch key={e.name} label={e.name} checked={on[e.name]} onChange={(v) => setOn((o) => ({ ...o, [e.name]: v }))} />
          ))}
        </li>
      </Section>
    </Page>
  );
}

// Node: the versions Kiln keeps, and which sites pin each one (.nvmrc, .node-version, kiln.toml).
export function Node() {
  return (
    <Page title="Node" actions={<Button icon={<Add16Regular />}>Install a version</Button>}>
      <Section title="Versions" meta="from nodejs.org, signatures checked">
        {nodeVersions.map((v) => (
          <Row
            key={v.version}
            columns={cols}
            actions={
              <More
                name={`Node ${v.line}`}
                items={[
                  { label: "Make it the default", onSelect: () => {} },
                  { label: "Remove…", icon: <Delete16Regular />, danger: true, groupStart: true, onSelect: () => {} },
                ]}
              />
            }
          >
            <RowName>
              <span className="figures">Node {v.version}</span>
              {v.isDefault && (
                <span className="ml-2">
                  <Tag>default</Tag>
                </span>
              )}
            </RowName>
            <Detail>{v.sites.length ? `pinned by ${v.sites.join(", ")}` : "no site pins it"}</Detail>
            <span className="flex justify-end">
              <Status state="running" label="Up to date" />
            </span>
          </Row>
        ))}
      </Section>
    </Page>
  );
}
