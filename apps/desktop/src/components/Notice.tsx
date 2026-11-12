import type { ReactNode } from "react";

// The one sentence about what matters, at the top of a page (ADR 0051 §4), with its action.
export function Notice({
  tone,
  title,
  children,
  actions,
}: {
  tone: "attention" | "failed";
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  const look =
    tone === "attention"
      ? "bg-warn-soft border-[color-mix(in_srgb,var(--warn)_22%,transparent)] [--mark:var(--warn)]"
      : "bg-bad-soft border-[color-mix(in_srgb,var(--bad)_22%,transparent)] [--mark:var(--bad)]";
  return (
    <div role="status" className={`my-1 mb-7 flex items-start gap-3.5 rounded-xl border px-5 py-[18px] ${look}`}>
      <span
        aria-hidden="true"
        className="mt-[7px] size-[9px] shrink-0 rounded-full bg-[var(--mark)] shadow-[0_0_0_4px_color-mix(in_srgb,var(--mark)_18%,transparent)]"
      />
      <div>
        <h2 className="m-0 text-base leading-6 font-semibold tracking-[-.01em]">{title}</h2>
        {children && <p className="m-0 mt-0.5 text-[13px] text-text-2">{children}</p>}
      </div>
      {actions && <div className="ml-auto flex gap-1.5 self-center">{actions}</div>}
    </div>
  );
}
