import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";

export interface Tab {
  id: string;
  label: string;
  /** Quiet detail beside the label, such as a count. */
  detail?: string;
}

// Tabs under a page's title: arrow keys move between them, as the ARIA tabs pattern asks.
export function Tabs({ tabs, current, onSelect, children }: { tabs: Tab[]; current: string; onSelect: (id: string) => void; children: ReactNode }) {
  const id = useId();
  const list = useRef<HTMLDivElement>(null);
  const keys = (e: KeyboardEvent) => {
    const at = tabs.findIndex((t) => t.id === current);
    const to = e.key === "ArrowRight" ? at + 1 : e.key === "ArrowLeft" ? at - 1 : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : null;
    if (to === null) return;
    e.preventDefault();
    const next = tabs[(to + tabs.length) % tabs.length];
    onSelect(next.id);
    list.current?.querySelector<HTMLElement>(`#${CSS.escape(`${id}-${next.id}`)}`)?.focus();
  };
  return (
    <>
      <div ref={list} role="tablist" onKeyDown={keys} className="mb-5 flex gap-1 border-b border-line">
        {tabs.map((t) => {
          const selected = t.id === current;
          return (
            <button
              key={t.id}
              id={`${id}-${t.id}`}
              type="button"
              role="tab"
              aria-selected={selected}
              aria-controls={`${id}-panel`}
              tabIndex={selected ? 0 : -1}
              onClick={() => onSelect(t.id)}
              className={`relative -mb-px h-9 rounded-t-md px-3 text-[13px] font-medium ${
                selected ? "text-text after:absolute after:inset-x-3 after:bottom-0 after:h-0.5 after:rounded-full after:bg-ember" : "text-text-2 hover:text-text"
              }`}
            >
              {t.label}
              {t.detail && <span className="ml-1.5 text-xs font-normal text-text-3">{t.detail}</span>}
            </button>
          );
        })}
      </div>
      <div id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-${current}`}>
        {children}
      </div>
    </>
  );
}
