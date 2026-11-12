import { useId } from "react";

export function Switch({
  checked,
  onChange,
  label,
  description,
  disabled,
  hideLabel,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
  /** The label is already shown beside it (a setting's title): keep it for screen readers only. */
  hideLabel?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex items-center justify-between gap-6">
      <div className={hideLabel ? "sr-only" : "min-w-0"}>
        <label htmlFor={id} className="block text-sm">
          {label}
        </label>
        {description && (
          <p id={`${id}-d`} className="m-0 text-xs text-text-2">
            {description}
          </p>
        )}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-describedby={description ? `${id}-d` : undefined}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors duration-150 disabled:opacity-40 forced-colors:border forced-colors:border-[ButtonText] ${
          checked ? "bg-ink" : "bg-press shadow-[inset_0_0_0_1px_var(--line-strong)]"
        }`}
      >
        <span
          aria-hidden="true"
          className={`absolute left-0.5 top-0.5 size-[18px] rounded-full shadow-[0_1px_3px_rgba(0,0,0,.28)] transition-transform duration-150 ease-out forced-colors:bg-[ButtonText] ${
            checked ? "translate-x-4 bg-on-ink" : "bg-white"
          }`}
        />
      </button>
    </div>
  );
}
