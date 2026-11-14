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
export const templates: { id: string; label: string; what: string; laravel: boolean }[] = [
  { id: "laravel", label: "Laravel", what: "The Laravel skeleton: Blade, Vite, SQLite.", laravel: true },
  { id: "react", label: "Laravel + React", what: "Laravel's starter kit: Inertia, React, TypeScript and sign-in. Its front end is built with Kiln's Node.", laravel: true },
  { id: "vue", label: "Laravel + Vue", what: "Laravel's starter kit: Inertia, Vue, TypeScript and sign-in. Its front end is built with Kiln's Node.", laravel: true },
  { id: "livewire", label: "Laravel + Livewire", what: "Laravel's starter kit: Livewire, Flux and sign-in. Its front end is built with Kiln's Node.", laravel: true },
  { id: "symfony", label: "Symfony", what: "Symfony's skeleton: the framework and nothing more.", laravel: false },
  { id: "bedrock", label: "WordPress (Bedrock)", what: "WordPress through Composer, by Roots. Set its database in .env.", laravel: false },
  { id: "empty", label: "Empty PHP site", what: "A folder with public/index.php, for anything else.", laravel: false },
];

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
  const [template, setTemplate] = useState("laravel");
  const [folder, setFolder] = useState(folders[0] ?? "");
  const [version, setVersion] = useState("");
  const [secure, setSecure] = useState(false);
  const [db, setDb] = useState("");
  const [busy, setBusy] = useState(false);
  const valid = name ? siteName(name) : null;
  const chosen = templates.find((t) => t.id === template)!;
  const lines = [...new Set((php.data?.versions ?? []).filter((v) => v.installed).map((v) => v.version.split(".").slice(0, 2).join(".")))];
  const databases = (services.data?.instances ?? []).filter((i) => ["mysql", "mariadb", "postgres"].includes(i.kind) && i.enabled);
  const ready = valid !== null && !valid.includes(".") && folder !== "" && !busy;
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
              const ok = await run(newArgs({ name: valid!, template, folder, php: version, secure, db: chosen.laravel ? db : "" }), `${valid}.test is ready.`);
              setBusy(false);
              if (ok) onClose();
            }}
          >
            {busy ? "Creating…" : "Create"}
          </Button>
        </>
      }
    >
      <p className="mt-0">Kiln makes the project with its own PHP and Composer and serves it at once. The download can take a few minutes.</p>
      <div className="grid grid-cols-2 gap-4">
        <TextField
          label="Name"
          placeholder="shop"
          value={name}
          onChange={(e) => setName(e.target.value)}
          error={name && (!valid || valid.includes(".")) ? "One word of letters, digits and hyphens: it's the folder and the site's name." : undefined}
        />
        <Select label="Start from" value={template} onChange={(e) => setTemplate(e.target.value)} options={templates.map((t) => ({ value: t.id, label: t.label }))} />
      </div>
      <p className="mb-4 mt-2 text-xs text-text-3">{chosen.what}</p>
      <div className="flex items-end gap-3">
        <TextField label="In the folder" className="min-w-0 flex-1" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="D:\\Code" />
        <Button
          onClick={async () => {
            const picked = await pickFolder("Choose where the project's folder goes");
            if (picked) setFolder(picked);
          }}
        >
          Choose…
        </Button>
      </div>
      <p className="mb-4 mt-1.5 text-xs text-text-3">{folder && valid && !valid.includes(".") ? `It goes in ${folder.replace(/[\\/]$/, "")}\\${valid}.` : "A parked folder serves it as soon as it's made."}</p>
      <div className="grid grid-cols-2 items-end gap-4">
        <Select label="PHP" value={version} onChange={(e) => setVersion(e.target.value)} options={[{ value: "", label: "The default" }, ...lines.map((l) => ({ value: l, label: `PHP ${l}` }))]} />
        {chosen.laravel && (
          <Select
            label="Database"
            value={db}
            onChange={(e) => setDb(e.target.value)}
            options={[{ value: "", label: "SQLite (in the project)" }, ...databases.map((i) => ({ value: i.name, label: `${i.name} (${i.kind})` }))]}
          />
        )}
      </div>
      <div className="mt-4 max-w-[260px]">
        <Switch label="Serve over HTTPS" checked={secure} onChange={setSecure} />
      </div>
    </Dialog>
  );
}
