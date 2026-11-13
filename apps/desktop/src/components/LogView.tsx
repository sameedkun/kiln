import { useEffect, useLayoutEffect, useRef } from "react";

export interface LogLine {
  time: string;
  level: "debug" | "info" | "warn" | "error";
  text: string;
}

// Log lines in monospace: the time quiet, warnings and errors named in words as well as colour.
// While following, it stays at the newest line unless the reader has scrolled up to read.
export function LogView({ lines, label, follow = false, tall = false }: { lines: LogLine[]; label: string; follow?: boolean; tall?: boolean }) {
  const level = { debug: "text-text-3", info: "text-text-3", warn: "text-warn font-medium", error: "text-bad font-semibold" };
  const box = useRef<HTMLDivElement>(null);
  const atEnd = useRef(true);
  useLayoutEffect(() => {
    const el = box.current;
    if (el && follow && atEnd.current) el.scrollTop = el.scrollHeight;
  }, [lines, follow]);
  useEffect(() => {
    if (follow) atEnd.current = true;
  }, [follow]);
  return (
    <div
      ref={box}
      role="log"
      aria-label={label}
      tabIndex={0}
      onScroll={(e) => {
        const el = e.currentTarget;
        atEnd.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
      className={`${tall ? "h-[calc(100vh-250px)] min-h-[240px]" : "max-h-[420px]"} overflow-auto rounded-xl border border-line py-1.5 font-mono text-xs leading-[19px]`}
    >
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
