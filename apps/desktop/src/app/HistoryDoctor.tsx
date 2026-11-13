import { useState } from "react";
import { Button } from "../components/Button";
import { Page } from "../components/Page";
import { Row, Section } from "../components/Rows";
import { Status, type StatusState } from "../components/Status";
import { Switch } from "../components/Switch";
import { Tag } from "../components/Tag";
import type { DoctorReport, Finding, HistoryList, Operation } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";

/** "Today", "Yesterday" or the date, for grouping operations by day. */
export function dayOf(ms: number | null, now = new Date()): string {
  if (ms === null) return "Earlier";
  const d = new Date(ms);
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short", year: d.getFullYear() === now.getFullYear() ? undefined : "numeric" });
}

/** Operations in order, grouped by day; each group keeps the order it was given. */
export function byDay(ops: Operation[], now = new Date()): [string, Operation[]][] {
  const groups: [string, Operation[]][] = [];
  for (const op of ops) {
    const day = dayOf(op.at_ms, now);
    const last = groups.at(-1);
    if (last && last[0] === day) last[1].push(op);
    else groups.push([day, [op]]);
  }
  return groups;
}

// History (ADR 0058 §3): everything Kiln changed, newest first, each undoable with its preview.
export function HistoryPage() {
  const [all, setAll] = useState(false);
  const history = useRead<HistoryList>("history.list", { all });
  const { confirm } = useChanges();
  const ops = history.data?.operations ?? [];
  return (
    <Page
      title="History"
      actions={<Switch label="Show undone" checked={all} onChange={setAll} />}
    >
      <p className="m-0 max-w-[68ch] px-1 text-[13px] text-text-2">
        Everything Kiln changed on this PC, newest first. Undo shows what it will put back before it does; undoing an older change can need the newer ones undone first.
      </p>
      {history.error && <p className="m-0 px-1 text-[13px] text-text-2">{history.error}</p>}
      {history.data && ops.length === 0 && (
        <p className="m-0 px-1 text-[13px] text-text-2">
          {history.data.hidden > 0 ? `Nothing Kiln changed is in effect now. ${history.data.hidden} earlier change${history.data.hidden === 1 ? " was" : "s were"} undone: Show undone lists them.` : "Kiln hasn't changed anything on this PC yet."}
        </p>
      )}
      {byDay(ops).map(([day, group]) => (
        <Section key={day} title={day}>
          {group.map((op) => {
            const inEffect = op.state === "in effect";
            return (
              <Row
                key={op.id}
                columns="56px 64px 1fr auto"
                actions={
                  inEffect || op.state.startsWith("stuck") || op.state.startsWith("half-done") ? (
                    <Button variant="quiet" size="sm" onClick={() => confirm(`Undo #${op.id}?`, ["undo", String(op.id)], `Undid #${op.id}.`)}>
                      Undo
                    </Button>
                  ) : undefined
                }
              >
                <span className="figures text-xs text-text-3">#{op.id}</span>
                <span className="figures text-xs text-text-3">{op.at_ms ? new Date(op.at_ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : ""}</span>
                <span className={inEffect ? "" : "text-text-2"}>{op.description}</span>
                <span className="text-right">{!inEffect && <Tag>{op.state}</Tag>}</span>
              </Row>
            );
          })}
        </Section>
      ))}
    </Page>
  );
}

const statusOf: Record<Finding["status"], StatusState> = { ok: "running", info: "stopped", warn: "attention", fail: "failed" };
const statusWord: Record<Finding["status"], string> = { ok: "OK", info: "Note", warn: "Check", fail: "Problem" };

/** Findings grouped by area, in the doctor's order. */
export function byArea(findings: Finding[]): [string, Finding[]][] {
  const groups = new Map<string, Finding[]>();
  for (const f of findings) groups.set(f.area, [...(groups.get(f.area) ?? []), f]);
  return [...groups];
}

// Doctor (ADR 0058 §4): `kiln doctor`'s findings, problems first, each with its fix.
export function DoctorPage() {
  const report = useRead<DoctorReport>("doctor.report");
  const [everything, setEverything] = useState(false);
  const findings = report.data?.findings ?? [];
  const problems = findings.filter((f) => f.status === "fail" || f.status === "warn");
  const shown = everything ? findings : findings.filter((f) => f.status !== "ok");
  return (
    <Page
      title="Doctor"
      actions={
        <>
          <Switch label="Show what's fine" checked={everything} onChange={setEverything} />
          <Button size="sm" onClick={report.reload}>
            Check again
          </Button>
        </>
      }
    >
      <p className="m-0 max-w-[68ch] px-1 text-[13px] text-text-2">
        {report.data
          ? problems.length === 0
            ? `Nothing needs you: ${findings.filter((f) => f.status === "ok").length} checks passed.`
            : `${problems.length} thing${problems.length === 1 ? " needs" : "s need"} a look. Each says what to run in a terminal.`
          : "Checking Kiln, its ports, the web server, PHP, sites, the hosts file and services…"}
      </p>
      {report.error && <p className="m-0 px-1 text-[13px] text-text-2">{report.error}</p>}
      {byArea(shown).map(([area, group]) => (
        <Section key={area} title={area}>
          {group.map((f, i) => (
            <Row key={`${area}-${i}`} columns="110px 1fr">
              <Status state={statusOf[f.status]} label={statusWord[f.status]} />
              <span>
                <span className="text-[13px]">{f.message}</span>
                {f.fix && (
                  <span className="mt-1 block text-xs text-text-2">
                    Fix: <code className="font-mono text-xs text-text">{f.fix}</code>
                  </span>
                )}
              </span>
            </Row>
          ))}
        </Section>
      ))}
    </Page>
  );
}
