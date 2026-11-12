import { useState } from "react";
import { Chips } from "../../components/Chips";
import { LogView } from "../../components/LogView";
import { Page } from "../../components/Page";
import { Switch } from "../../components/Switch";
import { logSources, redisLog } from "../sample";

// Logs: every source on the left, the chosen one followed live on the right.
export function Logs() {
  const [source, setSource] = useState("redis");
  const [level, setLevel] = useState<"all" | "warn" | "error">("all");
  const [follow, setFollow] = useState(true);
  const lines = redisLog.filter((l) => level === "all" || (level === "warn" ? l.level !== "info" : l.level === "error"));
  const label = logSources.flatMap((g) => g.items).find((i) => i.id === source)?.label ?? source;
  return (
    <Page title="Logs">
      <div className="grid grid-cols-[200px_1fr] gap-6">
        <nav aria-label="Log sources">
          {logSources.map((g) => (
            <div key={g.group} className="mb-3">
              <div className="px-2 pb-1 text-xs font-medium text-text-3">{g.group}</div>
              {g.items.map((i) => (
                <button
                  key={i.id}
                  type="button"
                  aria-current={i.id === source ? "true" : undefined}
                  onClick={() => setSource(i.id)}
                  className={`flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px] ${
                    i.id === source ? "bg-hover font-medium text-text forced-colors:outline forced-colors:outline-1" : "text-text-2 hover:bg-hover hover:text-text"
                  }`}
                >
                  <span className="flex-1">{i.label}</span>
                  {"attention" in i && i.attention && <span className="size-[7px] rounded-full bg-warn" role="img" aria-label="needs attention" />}
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="min-w-0">
          <div className="mb-3 flex items-center justify-between gap-4">
            <Chips
              label="Show"
              value={level}
              onChange={setLevel}
              options={[
                { id: "all", label: "Everything" },
                { id: "warn", label: "Warnings and errors" },
                { id: "error", label: "Errors" },
              ]}
            />
            <div className="w-[150px]">
              <Switch label="Follow" checked={follow} onChange={setFollow} />
            </div>
          </div>
          <LogView label={`${label} log`} lines={lines} />
        </div>
      </div>
    </Page>
  );
}
