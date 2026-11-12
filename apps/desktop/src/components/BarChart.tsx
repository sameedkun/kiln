// A small bar chart, such as response times by bucket. One neutral colour; the slow end
// in amber and the failing end in red, and every bar labelled with its count for screen
// readers and on hover.
export function BarChart({ label, bars }: { label: string; bars: { label: string; value: number; tone?: "slow" | "bad" }[] }) {
  const max = Math.max(1, ...bars.map((b) => b.value));
  return (
    <figure className="m-0">
      <figcaption className="mb-2 text-xs font-medium text-text-3">{label}</figcaption>
      <ul className="m-0 flex h-28 list-none items-end gap-1.5 p-0">
        {bars.map((b) => (
          <li key={b.label} className="flex h-full flex-1 flex-col items-center justify-end gap-1" title={`${b.label}: ${b.value}`}>
            <span className="sr-only">
              {b.label}: {b.value}
            </span>
            <span
              aria-hidden="true"
              className={`w-full rounded-t-[4px] forced-colors:bg-[CanvasText] ${b.tone === "bad" ? "bg-bad" : b.tone === "slow" ? "bg-warn" : "bg-text-3"}`}
              style={{ height: `${Math.max(2, (b.value / max) * 100)}%` }}
            />
            <span aria-hidden="true" className="figures text-[11px] text-text-3">
              {b.label}
            </span>
          </li>
        ))}
      </ul>
    </figure>
  );
}
