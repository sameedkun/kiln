import { useId, type InputHTMLAttributes } from "react";

export function TextField({
  label,
  hint,
  error,
  mono,
  className = "",
  ...input
}: InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: string; error?: string; mono?: boolean }) {
  const id = useId();
  const described = [hint && `${id}-h`, error && `${id}-e`].filter(Boolean).join(" ") || undefined;
  return (
    <div className={`flex flex-col gap-1.5 ${className}`}>
      <label htmlFor={id} className="text-[13px] font-medium">
        {label}
      </label>
      <input
        id={id}
        aria-invalid={error ? true : undefined}
        aria-describedby={described}
        className={`h-8 rounded-control border bg-[color-mix(in_srgb,var(--bg)_60%,white)] px-2.5 text-sm text-text outline-none placeholder:text-text-3 focus:border-ember focus:shadow-[0_0_0_3px_var(--ember-soft)] dark:bg-hover ${
          error ? "border-bad" : "border-line-strong"
        } ${mono ? "font-mono text-[13px]" : ""}`}
        {...input}
      />
      {hint && !error && (
        <p id={`${id}-h`} className="m-0 text-xs text-text-2">
          {hint}
        </p>
      )}
      {error && (
        <p id={`${id}-e`} className="m-0 text-xs text-bad">
          {error}
        </p>
      )}
    </div>
  );
}
