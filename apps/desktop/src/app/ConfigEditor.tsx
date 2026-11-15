import { useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Dialog } from "../components/Dialog";
import { Select } from "../components/Select";
import { call, message } from "./api";
import { useChanges } from "./Change";
import { useRead } from "./hooks";

/** `config.show` (ADR 0089 §2). */
export interface ConfigShow {
  label: string;
  path: string;
  exists: boolean;
  text: string;
  backups: string[];
}

/** A backup's name as a time: `20261115-093000.bak` → `2026-11-15 09:30:00 UTC`. */
export function backupTime(name: string): string {
  const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})/.exec(name);
  return m ? `${m[1]}-${m[2]}-${m[3]} ${m[4]}:${m[5]}:${m[6]} UTC` : name;
}

// A config file edited whole (ADR 0089 §5): php.ini, a site's .env, a service's settings.
// The text is staged with kilnd (never put on a command line), then `kiln config save`
// shows the changed lines, checks the file and keeps a backup.
export function ConfigEditor({ target, onClose }: { target: string; onClose: () => void }) {
  const file = useRead<ConfigShow>("config.show", { target });
  const { confirm } = useChanges();
  const [text, setText] = useState<string | null>(null);
  const [backup, setBackup] = useState("");
  const [error, setError] = useState<string>();
  useEffect(() => {
    if (file.data && text === null) setText(file.data.text);
  }, [file.data, text]);

  const label = file.data?.label ?? target;
  const changed = file.data !== undefined && text !== null && text !== file.data.text;

  const save = async () => {
    if (text === null) return;
    try {
      const { token } = await call<{ token: string }>("config.stage", { text });
      onClose();
      confirm(`Save ${label}?`, ["config", "save", target, "--staged", token], `Saved ${label}; the previous version is a backup.`);
    } catch (e) {
      setError(message(e));
    }
  };

  return (
    <Dialog
      open
      wide
      onClose={onClose}
      title={`Edit ${label}`}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="ink" disabled={!changed} onClick={save}>
            Show the changes
          </Button>
        </>
      }
    >
      {file.error && <p className="m-0 mb-3 text-[13px] text-bad">{file.error}</p>}
      {file.data && <p className="m-0 mb-2 break-all font-mono text-xs text-text-3">{file.data.path}</p>}
      <textarea
        aria-label={label}
        value={text ?? ""}
        onChange={(e) => setText(e.target.value)}
        spellCheck={false}
        rows={20}
        className="w-full rounded-lg border border-line bg-transparent p-2.5 font-mono text-xs leading-[18px] text-text outline-none focus:border-ember"
      />
      <p className="m-0 mt-2 text-xs text-text-2">Kiln checks it before saving{target.startsWith("service:") ? " (the service restarts, and goes back to the old file if it won't start)" : ""}, and keeps the old version as a backup.</p>
      {error && <p className="m-0 mt-2 text-[13px] text-bad">{error}</p>}
      {file.data && file.data.backups.length > 0 && (
        <div className="mt-4 flex flex-wrap items-end gap-3 border-t border-line pt-4">
          <Select
            label="Backups"
            value={backup}
            onChange={(e) => setBackup(e.target.value)}
            options={[{ value: "", label: "Choose a backup" }, ...file.data.backups.map((b) => ({ value: b, label: backupTime(b) }))]}
          />
          <Button
            disabled={!backup}
            onClick={() => {
              onClose();
              confirm(`Restore ${label}?`, ["config", "restore", target, backup], `${label} is back as of ${backupTime(backup)}.`);
            }}
          >
            Restore…
          </Button>
        </div>
      )}
    </Dialog>
  );
}
