// A row of filters where one is chosen: categories of services, levels of a log.
export function Chips<T extends string>({ label, options, value, onChange }: { label: string; options: { id: T; label: string; count?: number }[]; value: T; onChange: (id: T) => void }) {
  return (
    <div role="radiogroup" aria-label={label} className="flex flex-wrap gap-1.5">
      {options.map((o) => {
        const on = o.id === value;
        return (
          <button
            key={o.id}
            type="button"
            role="radio"
            aria-checked={on}
            onClick={() => onChange(o.id)}
            className={`h-7 rounded-full border px-3 text-[12.5px] font-medium ${
              on ? "border-transparent bg-ink text-on-ink forced-colors:border-[Highlight]" : "border-line-strong text-text-2 hover:bg-hover hover:text-text"
            }`}
          >
            {o.label}
            {o.count !== undefined && <span className={`figures ml-1.5 font-normal ${on ? "opacity-70" : "text-text-3"}`}>{o.count}</span>}
          </button>
        );
      })}
    </div>
  );
}
