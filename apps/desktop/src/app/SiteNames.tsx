import { useState } from "react";
import { Button } from "../components/Button";
import { Section } from "../components/Rows";
import { Tag } from "../components/Tag";
import { TextField } from "../components/TextField";
import type { Site } from "./api";
import { useChanges } from "./Change";

/** A name as Kiln will serve it: `admin` is `admin.test`, `admin.test` stays (ADR 0083). */
export function fullName(name: string, tld: string): string {
  const n = name.trim().toLowerCase();
  return n.endsWith(`.${tld}`) ? n : `${n}.${tld}`;
}

/** Labels of letters, digits and inner hyphens, separated by dots. */
export function validName(name: string): boolean {
  return /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)*$/i.test(name.trim());
}

// A site's other names (ADR 0083 §5): Kiln's, removable here, and its kiln.toml's, shown.
export function SiteNames({ site }: { site: Site }) {
  const { run } = useChanges();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  if (site.source === "alias") return null;
  const tld = site.host.split(".").pop() ?? "test";
  const names = site.aliases ?? [];
  const typed = name.trim();
  const ok = typed !== "" && validName(typed) && fullName(typed, tld) !== site.host;

  const add = async () => {
    if (!ok) return;
    const host = fullName(typed, tld);
    setBusy(true);
    if (await run(["domain", "add", site.host, typed], `${host} opens ${site.host} now.`)) setName("");
    setBusy(false);
  };

  return (
    <Section title="Other names" meta={names.length ? `${names.length} more` : `only ${site.host}`}>
      {names.map((n) => (
        <li key={n.host} className="flex flex-wrap items-center gap-3 border-b border-line px-1 py-3">
          <span className="min-w-0 flex-1 truncate font-mono text-xs">{n.host}</span>
          {n.from === "kiln" ? (
            <Button size="sm" onClick={() => run(["domain", "remove", site.host, n.host], `${n.host} isn't a name of ${site.host} any more.`)}>
              Remove
            </Button>
          ) : (
            <span title="Kept in the project's kiln.toml: change it there">
              <Tag>kiln.toml</Tag>
            </span>
          )}
        </li>
      ))}
      <li className="flex flex-wrap items-end gap-3 px-1 py-4">
        <TextField
          className="min-w-[220px] flex-1"
          label="Add a name"
          placeholder={`admin, or admin.${tld}`}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          hint={typed && ok ? `Serves ${site.host} at ${fullName(typed, tld)} too. Kept by Kiln, not in the project.` : "Kept by Kiln on this PC, not in the project."}
          error={typed && !validName(typed) ? "Letters, digits, hyphens and dots." : undefined}
        />
        <Button disabled={!ok || busy} onClick={add}>
          {busy ? "Adding…" : "Add"}
        </Button>
      </li>
    </Section>
  );
}
