import { ChevronDown12Regular } from "@fluentui/react-icons";
import { useId, type SelectHTMLAttributes } from "react";

// A choice from a short list (a PHP version, a theme), on the browser's own <select>: it
// brings the keyboard and screen-reader behaviour with it.
export function Select({
  label,
  hideLabel,
  options,
  className = "",
  ...rest
}: SelectHTMLAttributes<HTMLSelectElement> & { label: string; hideLabel?: boolean; options: { value: string; label: string }[] }) {
  const id = useId();
  return (
    <span className={`inline-flex items-center gap-2 ${className}`}>
      <label htmlFor={id} className={hideLabel ? "sr-only" : "text-[13px] text-text-2"}>
        {label}
      </label>
      <span className="relative inline-flex">
        <select
          id={id}
          className="h-[30px] appearance-none rounded-control border border-line-strong bg-transparent pl-2.5 pr-7 text-[13px] font-medium text-text hover:bg-hover focus:border-ember focus:outline-none focus:shadow-[0_0_0_3px_var(--ember-soft)] [&>option]:bg-[var(--bg)]"
          {...rest}
        >
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
        <ChevronDown12Regular aria-hidden="true" className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-text-2" />
      </span>
    </span>
  );
}
