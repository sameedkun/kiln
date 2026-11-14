import { useState } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Select } from "../components/Select";
import { Switch } from "../components/Switch";
import { TextField } from "../components/TextField";
import { pickFolder, siteName, type PhpList, type ServicesList } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";

/** Kiln's templates, as `kiln new --template` takes them (ADR 0070); the default first. */
export const templates: { id: string; what: string }[] = [
  { id: "laravel", what: "The Laravel skeleton: Blade, Vite; SQLite unless you choose a database." },
  { id: "react", what: "Laravel's starter kit: Inertia, React, TypeScript and sign-in. Its front end is built with Kiln's Node." },
  { id: "vue", what: "Laravel's starter kit: Inertia, Vue, TypeScript and sign-in. Its front end is built with Kiln's Node." },
  { id: "livewire", what: "Laravel's starter kit: Livewire, Flux and sign-in. Its front end is built with Kiln's Node." },
  { id: "symfony", what: "Symfony's skeleton: the framework and nothing more." },
  { id: "wordpress", what: "WordPress from wordpress.org, with its database made and wp-config.php written: open the site to finish its installation." },
  { id: "empty", what: "A folder with public/index.php, for anything else." },
];

/** The frameworks the dialog offers, and which databases each takes. */
export const frameworks: { id: string; label: string; databases: "any" | "mysql" | "none" }[] = [
  { id: "laravel", label: "Laravel", databases: "any" },
  { id: "symfony", label: "Symfony", databases: "none" },
  { id: "wordpress", label: "WordPress", databases: "mysql" },
  { id: "empty", label: "Empty PHP site", databases: "none" },
];

/** Laravel's starter kits; "" is the plain skeleton. */
export const kits: { id: string; label: string }[] = [
  { id: "", label: "None" },
  { id: "react", label: "React" },
  { id: "vue", label: "Vue" },
  { id: "livewire", label: "Livewire" },
];

/** The template a framework and starter kit make. */
export const templateFor = (framework: string, kit: string) => (framework === "laravel" ? kit || "laravel" : framework);

/** The command the dialog runs: `kiln new` with only what was chosen. */
export function newArgs(o: { name: string; template: string; folder: string; php: string; secure: boolean; db: string }): string[] {
  return [
    "new",
    o.name,
    "--template",
    o.template,
    "--path",
    o.folder,
    ...(o.php ? ["--php", o.php] : []),
    ...(o.secure ? ["--secure"] : []),
    ...(o.db ? ["--db", o.db] : []),
  ];
}

// A new project from one of Kiln's templates, made with Kiln's PHP and Composer and served at
// once (ADR 0070). It can take minutes; the working toast follows its steps.
export function NewSiteDialog({ folders, onClose }: { folders: string[]; onClose: () => void }) {
  const php = useRead<PhpList>("php.list");
  const services = useRead<ServicesList>("services.list");
  const { run } = useChanges();
  const [name, setName] = useState("");
  const [framework, setFramework] = useState("laravel");
  const [kit, setKit] = useState("");
  const [folder, setFolder] = useState(folders[0] ?? "");
  const [version, setVersion] = useState("");
  const [secure, setSecure] = useState(false);
  const [db, setDb] = useState("");
  const [busy, setBusy] = useState(false);
  const valid = name ? siteName(name) : null;
  const chosen = frameworks.find((f) => f.id === framework)!;
  const template = templateFor(framework, kit);
  const what = templates.find((t) => t.id === template)?.what ?? "";
  const lines = [...new Set((php.data?.versions ?? []).filter((v) => v.installed).map((v) => v.version.split(".").slice(0, 2).join(".")))];
  // WordPress runs on MySQL or MariaDB only; Laravel on any of Kiln's databases, or SQLite.
  const databases = (services.data?.instances ?? []).filter((i) => (chosen.databases === "mysql" ? ["mysql", "mariadb"] : ["mysql", "mariadb", "postgres"]).includes(i.kind) && i.enabled);
  const needsDb = chosen.databases === "mysql";
  const chosenDb = needsDb ? db || databases[0]?.name || "" : db;
  const ready = valid !== null && !valid.includes(".") && folder !== "" && !busy && (!needsDb || chosenDb !== "");
  return (
    <Dialog
      open
      wide
      onClose={() => !busy && onClose()}
      title="New site"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            variant="ink"
            disabled={!ready}
            onClick={async () => {
              setBusy(true);
              const ok = await run(newArgs({ name: valid!, template, folder, php: version, secure, db: chosen.databases === "none" ? "" : chosenDb }), `${valid}.test is ready.`);
              setBusy(false);
              if (ok) onClose();
            }}
          >
            {busy ? "Creating…" : "Create"}
          </Button>
        </>
      }
    >
      <p className="mb-4 mt-0">Kiln makes the project with its own PHP and Composer, sets it up to run, and serves it at once. The download can take a few minutes.</p>
      <div className="flex flex-col gap-4">
        <TextField
          label="Name"
          placeholder="shop"
          value={name}
          onChange={(e) => setName(e.target.value)}
          hint={valid && !valid.includes(".") ? `${valid}.test, in ${folder.replace(/[\\/]$/, "") || "…"}\\${valid}` : undefined}
          error={name && (!valid || valid.includes(".")) ? "One word of letters, digits and hyphens: it's the folder and the site's name." : undefined}
        />
        <div className="grid grid-cols-2 gap-4">
          <Select
            label="Framework"
            className="w-full"
            value={framework}
            onChange={(e) => {
              setFramework(e.target.value);
              setKit("");
              setDb("");
            }}
            options={frameworks.map((f) => ({ value: f.id, label: f.label }))}
          />
          {framework === "laravel" && (
            <Select label="Starter kit" className="w-full" value={kit} onChange={(e) => setKit(e.target.value)} options={kits.map((k) => ({ value: k.id, label: k.label }))} />
          )}
        </div>
        <p className="-mt-2 mb-0 text-xs text-text-3">{what}</p>
        <div className="flex items-end gap-3">
          <TextField label="In the folder" className="min-w-0 flex-1" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="D:\\Code" hint="A parked folder serves it as soon as it's made." />
          <Button
            className="mb-[22px]"
            onClick={async () => {
              const picked = await pickFolder("Choose where the project's folder goes");
              if (picked) setFolder(picked);
            }}
          >
            Choose…
          </Button>
        </div>
        <div className="grid grid-cols-2 gap-4">
          <Select label="PHP" className="w-full" value={version} onChange={(e) => setVersion(e.target.value)} options={[{ value: "", label: "The default" }, ...lines.map((l) => ({ value: l, label: `PHP ${l}` }))]} />
          {chosen.databases !== "none" &&
            (needsDb && databases.length === 0 ? (
              <p className="m-0 self-end pb-1 text-xs text-text-2">WordPress needs MySQL or MariaDB: add one on the Services page first.</p>
            ) : (
              <Select
                label="Database"
                className="w-full"
                value={chosenDb}
                onChange={(e) => setDb(e.target.value)}
                options={[...(needsDb ? [] : [{ value: "", label: "SQLite (in the project)" }]), ...databases.map((i) => ({ value: i.name, label: `${i.name} (${i.kind})` }))]}
              />
            ))}
        </div>
        <div className="max-w-[260px]">
          <Switch label="Serve over HTTPS" checked={secure} onChange={setSecure} />
        </div>
      </div>
    </Dialog>
  );
}
