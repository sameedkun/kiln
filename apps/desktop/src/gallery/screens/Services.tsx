import { Add16Regular, ArrowDownload16Regular, Delete16Regular, Open16Regular, WindowConsole20Regular } from "@fluentui/react-icons";
import { useState } from "react";
import { Button } from "../../components/Button";
import { Chips } from "../../components/Chips";
import { Dialog } from "../../components/Dialog";
import { Page } from "../../components/Page";
import { Detail, Row, RowName, Section } from "../../components/Rows";
import { Select } from "../../components/Select";
import { Status } from "../../components/Status";
import { catalog, categories, services } from "../sample";
import { More, StartStop } from "./common";

const cols = "minmax(170px,1.2fr) 80px minmax(130px,1.2fr) 200px";
type Category = (typeof categories)[number]["id"];

// Every service instance, by kind, and the catalog to add more from.
export function Services() {
  const [category, setCategory] = useState<Category>("all");
  const [adding, setAdding] = useState(false);
  const shown = categories.filter((c) => c.id !== "all" && (category === "all" || category === c.id));
  const running = services.filter((s) => s.state === "running").length;
  return (
    <Page
      title="Services"
      after={<span className="figures text-[13px] text-text-3">{running} of {services.length} running</span>}
      actions={
        <Button icon={<Add16Regular />} onClick={() => setAdding(true)}>
          Add a service
        </Button>
      }
    >
      <div className="mb-6">
        <Chips
          label="Kind of service"
          value={category}
          onChange={setCategory}
          options={categories.map((c) => ({ id: c.id, label: c.label, count: c.id === "all" ? services.length : services.filter((s) => s.category === c.id).length }))}
        />
      </div>
      {shown.map((c) => {
        const list = services.filter((s) => s.category === c.id);
        if (list.length === 0) return null;
        return (
          <Section key={c.id} title={c.label}>
            {list.map((s) => (
              <Row key={s.id} columns={cols}>
                <RowName sub={`${s.id} · version ${s.version}`}>{s.name}</RowName>
                <Detail mono>:{s.port}</Detail>
                <Detail>{s.usedBy.length ? `used by ${s.usedBy.join(", ")}` : "no sites yet"}</Detail>
                <span className="flex items-center justify-end gap-0.5">
                  <span className="mr-1.5">
                    <Status state={s.state} label={s.state === "attention" ? "Stopped" : undefined} />
                  </span>
                  {s.web && <Button variant="quiet" aria-label={`Open ${s.name}'s page`} title="Open its page" icon={<Open16Regular />} />}
                  {s.client && <Button variant="quiet" aria-label={`Open a ${s.name} client`} title="Open a client" icon={<WindowConsole20Regular className="size-4" />} />}
                  <StartStop name={s.name} state={s.state} />
                  <More
                    name={s.name}
                    items={[
                      { label: "Restart", onSelect: () => {} },
                      { label: "View its log", onSelect: () => {} },
                      { label: "Change the port…", onSelect: () => {} },
                      ...(s.category === "databases" ? [{ label: "Dump a database…", icon: <ArrowDownload16Regular />, onSelect: () => {} }] : []),
                      { label: "Remove…", icon: <Delete16Regular />, danger: true, groupStart: true, onSelect: () => {} },
                    ]}
                  />
                </span>
              </Row>
            ))}
          </Section>
        );
      })}

      <Dialog
        open={adding}
        onClose={() => setAdding(false)}
        title="Add a service"
        wide
        footer={<Button onClick={() => setAdding(false)}>Close</Button>}
      >
        <p className="mt-0">Kiln downloads the service once, checks it against a pinned hash, and runs it on this PC only (127.0.0.1).</p>
        <ul className="m-0 list-none p-0">
          {catalog.map((c) => (
            <Row key={c.name} columns="minmax(140px,1fr) 100px 120px 70px">
              <RowName sub={c.category}>{c.name}</RowName>
              <Select label={`${c.name} version`} hideLabel options={c.versions.map((v) => ({ value: v, label: v }))} />
              <Detail>{c.size} download</Detail>
              <Button size="sm">Add</Button>
            </Row>
          ))}
        </ul>
      </Dialog>
    </Page>
  );
}
