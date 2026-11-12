export interface LogLine {
  time: string;
  level: "debug" | "info" | "warn" | "error";
  text: string;
}

// Log lines in monospace: the time quiet, warnings and errors named in words as well as colour.
export function LogView({ lines, label }: { lines: LogLine[]; label: string }) {
  const level = { debug: "text-text-3", info: "text-text-3", warn: "text-warn font-medium", error: "text-bad font-semibold" };
  return (
    <div role="log" aria-label={label} tabIndex={0} className="max-h-[420px] overflow-auto rounded-xl border border-line py-1.5 font-mono text-xs leading-[19px]">
      {lines.map((l, i) => (
        <div key={i} className="grid grid-cols-[64px_52px_1fr] px-3 hover:bg-hover">
          <span className="figures text-text-3">{l.time}</span>
          <span className={level[l.level]}>{l.level.toUpperCase()}</span>
          <span className="whitespace-pre-wrap break-words">{l.text}</span>
        </div>
      ))}
    </div>
  );
}
