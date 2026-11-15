import { useEffect, useLayoutEffect, useRef } from "react";

export interface LogLine {
  time: string;
  level: "debug" | "info" | "warn" | "error";
  text: string;
}

/** A piece of a log line: text, or a PHP file at a line (as PHP and Laravel print them). */
export type Piece = { text: string } | { text: string; path: string; line: number };

/** A line's text with each C:\...\x.php:42 and C:\...\x.php(42) picked out (ADR 0085 §4). */
export function linkPaths(text: string): Piece[] {
  const re = /([A-Za-z]:[\\/][^\s:()"'<>|*?]+?\.php)(?::(\d+)|\((\d+)\))/g;
  const out: Piece[] = [];
  let at = 0;
  for (const m of text.matchAll(re)) {
    const start = m.index ?? 0;
    if (start > at) out.push({ text: text.slice(at, start) });
    out.push({ text: m[0], path: m[1], line: Number(m[2] ?? m[3]) });
    at = start + m[0].length;
  }
  if (at < text.length) out.push({ text: text.slice(at) });
  return out;
}

// Log lines in monospace: the time quiet, warnings and errors named in words as well as colour.
// While following, it stays at the newest line unless the reader has scrolled up to read.
export function LogView({ lines, label, follow = false, tall = false, onPath }: { lines: LogLine[]; label: string; follow?: boolean; tall?: boolean; onPath?: (path: string, line: number) => void }) {
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
          <span className="whitespace-pre-wrap break-words">
            {onPath
              ? linkPaths(l.text).map((piece, j) =>
                  "path" in piece ? (
                    <button key={j} type="button" className="text-left underline decoration-dotted underline-offset-2 hover:text-ember" title="Open in your editor" onClick={() => onPath(piece.path, piece.line)}>
                      {piece.text}
                    </button>
                  ) : (
                    <span key={j}>{piece.text}</span>
                  ),
                )
              : l.text}
          </span>
        </div>
      ))}
    </div>
  );
}
