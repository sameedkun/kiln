// How much of something is used: memory, disk. The numbers are always written out; the bar
// only helps the eye.
export function Meter({ label, value, max, display, detail }: { label: string; value: number; max: number; display: string; detail?: string }) {
  const share = Math.min(1, Math.max(0, value / max));
  return (
    <div>
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-[13px] text-text-2">{label}</span>
        <span className="figures text-sm font-semibold">{display}</span>
      </div>
      <div
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={display}
        className="mt-1.5 h-1 overflow-hidden rounded-full bg-press forced-colors:border forced-colors:border-[CanvasText]"
      >
        <div className="h-full rounded-full bg-text-2 forced-colors:bg-[CanvasText]" style={{ width: `${share * 100}%` }} />
      </div>
      {detail && <p className="m-0 mt-1 text-xs text-text-3">{detail}</p>}
    </div>
  );
}
