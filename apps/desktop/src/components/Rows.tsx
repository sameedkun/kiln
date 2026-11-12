import type { ReactNode } from "react";

// Lists are rows with dividers on the backdrop, not a card per item (ADR 0051 §2).

export function Section({ title, meta, action, children }: { title: string; meta?: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="mt-2" aria-label={title}>
      <div className="flex items-baseline gap-2.5 border-b border-line px-1 pb-1.5">
        <h3 className="m-0 text-[13px] font-semibold tracking-[.01em]">{title}</h3>
        {meta && <span className="text-xs text-text-3">{meta}</span>}
        {action && <span className="ml-auto">{action}</span>}
      </div>
      <ul className="m-0 mb-7 list-none p-0">{children}</ul>
    </section>
  );
}

/**
 * One row. `columns` is the CSS grid template shared by a list's rows; `actions` show on
 * hover or keyboard focus.
 */
export function Row({ columns, children, actions }: { columns: string; children: ReactNode; actions?: ReactNode }) {
  return (
    <li
      className="group grid min-h-12 items-center gap-4 border-b border-line px-1 py-1.5 hover:bg-[linear-gradient(90deg,transparent,var(--hover)_8%,var(--hover)_92%,transparent)]"
      style={{ gridTemplateColumns: columns }}
    >
      {children}
      {actions && (
        <span className="flex justify-end gap-0.5 opacity-0 transition-opacity duration-100 group-hover:opacity-100 group-focus-within:opacity-100 [@media(hover:none)]:opacity-100">
          {actions}
        </span>
      )}
    </li>
  );
}

/** A row's name, with an optional quiet line under it (a path, a proxy target). */
export function RowName({ children, sub, before, mono }: { children: ReactNode; sub?: ReactNode; before?: ReactNode; mono?: boolean }) {
  return (
    <span className="flex min-w-0 items-center gap-2 text-sm font-semibold tracking-[-.005em]">
      {before}
      <span className="min-w-0">
        {children}
        {sub && <span className={`block truncate text-xs font-normal text-text-3 ${mono ? "font-mono" : ""}`}>{sub}</span>}
      </span>
    </span>
  );
}

/** A quiet detail cell. */
export function Detail({ children, mono }: { children: ReactNode; mono?: boolean }) {
  return <span className={`figures text-[12.5px] text-text-2 ${mono ? "font-mono text-xs" : ""}`}>{children}</span>;
}
