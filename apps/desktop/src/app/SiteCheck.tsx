import { useState } from "react";
import { Button } from "../components/Button";
import { Row, Section } from "../components/Rows";
import { Status } from "../components/Status";
import { Switch } from "../components/Switch";
import type { DoctorReport, Finding, Site } from "./api";
import { useChanges } from "./Change";
import { byArea, statusOf, statusWord } from "./HistoryDoctor";
import { useRead } from "./hooks";

const areaTitles: Record<string, string> = {
  project: "Project",
  envfile: "Environment file",
  packages: "Packages",
  database: "Database",
  answer: "The site's answer",
};

/** How the app runs a finding's fix: through its preview (`init`, `env use`), or after asking in words. */
export function fixWay(run: string[]): "preview" | "ask" {
  return run[0] === "init" || (run[0] === "env" && run[1] === "use") ? "preview" : "ask";
}

/** Problems first, then notes, then what's fine; each group in the doctor's order. */
export function worstFirst(findings: Finding[]): Finding[] {
  const rank = { fail: 0, warn: 1, info: 2, ok: 3 } as const;
  return findings
    .map((f, i) => [f, i] as const)
    .sort(([a, i], [b, j]) => rank[a.status] - rank[b.status] || i - j)
    .map(([f]) => f);
}

// The site's own checks (ADR 0081 §8): `kiln doctor <site>`, each fix run through its preview
// or a question first.
export function SiteCheck({ site }: { site: Site }) {
  const report = useRead<DoctorReport>("site.doctor", { host: site.host });
  const { confirm, ask } = useChanges();
  const [everything, setEverything] = useState(false);
  const findings = report.data?.findings ?? [];
  const problems = findings.filter((f) => f.status === "fail" || f.status === "warn");
  const shown = everything ? findings : findings.filter((f) => f.status !== "ok");

  const fix = (f: Finding) => {
    if (!f.run) return;
    const command = `kiln ${f.run.join(" ")}`;
    if (fixWay(f.run) === "preview") {
      confirm(f.run[0] === "init" ? `Set up ${site.host}?` : "Point the .env at it?", f.run, "Done: check again to see.");
    } else {
      ask(f.message, `Kiln runs ${command}.`, "Run it", f.run, "Done: check again to see.");
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3 px-1">
        <p className="m-0 mr-auto max-w-[68ch] text-[13px] text-text-2">
          {report.data
            ? problems.length === 0
              ? `Nothing needs you: ${findings.filter((f) => f.status === "ok").length} checks passed.`
              : `${problems.length} thing${problems.length === 1 ? " needs" : "s need"} a look.`
            : report.error
              ? report.error
              : `Checking ${site.host}'s .env, packages, database, PHP and its answer…`}
        </p>
        <Switch label="Show what's fine" checked={everything} onChange={setEverything} />
        <Button size="sm" onClick={report.reload}>
          Check again
        </Button>
      </div>
      {byArea(worstFirst(shown).sort((a, b) => Object.keys(areaTitles).indexOf(a.area) - Object.keys(areaTitles).indexOf(b.area))).map(([area, group]) => (
        <Section key={area} title={areaTitles[area] ?? area}>
          {group.map((f, i) => (
            <Row key={`${area}-${i}`} columns="110px 1fr auto">
              <Status state={statusOf[f.status]} label={statusWord[f.status]} />
              <span>
                <span className="whitespace-pre-line text-[13px]">{f.message}</span>
                {f.fix && (
                  <span className="mt-1 block text-xs text-text-2">
                    Fix: <code className="font-mono text-xs text-text">{f.fix}</code>
                  </span>
                )}
              </span>
              {f.run && f.status !== "ok" ? (
                <Button size="sm" onClick={() => fix(f)}>
                  Fix
                </Button>
              ) : (
                <span />
              )}
            </Row>
          ))}
        </Section>
      ))}
    </div>
  );
}
