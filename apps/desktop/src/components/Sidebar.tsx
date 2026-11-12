import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

export interface NavItem {
  id: string;
  label: string;
  icon: ReactNode;
  /** Quiet detail on the right: a count or versions. */
  detail?: string;
  /** Something on this page needs the user: an amber dot. */
  attention?: boolean;
  /** Starts a new group with a divider above it. */
  groupStart?: boolean;
}

// The pages, on a floating glass panel. The selection is a pill that slides to the chosen
// page (ADR 0051 §9); each item also marks itself, so it reads right without the pill
// (high contrast, or before the pill is measured).
export function Sidebar({
  items,
  footer,
  current,
  onSelect,
  status,
}: {
  items: NavItem[];
  footer: NavItem[];
  current: string;
  onSelect: (id: string) => void;
  status?: ReactNode;
}) {
  const nav = useRef<HTMLElement>(null);
  const [pill, setPill] = useState<{ top: number; height: number } | null>(null);

  useLayoutEffect(() => {
    const active = nav.current?.querySelector<HTMLElement>('[aria-current="page"]');
    setPill(active ? { top: active.offsetTop, height: active.offsetHeight } : null);
  }, [current]);

  const item = (it: NavItem) => {
    const selected = it.id === current;
    return (
      <div key={it.id}>
        {it.groupStart && <hr className="mx-1 my-1.5 border-0 border-t border-line" />}
        <button
          type="button"
          aria-current={selected ? "page" : undefined}
          onClick={() => onSelect(it.id)}
          className={`relative z-[1] flex h-[34px] w-full items-center gap-2.5 rounded-item px-2.5 text-left text-[13.5px] font-medium ${
            selected
              ? "text-text [&>svg]:text-ember forced-colors:outline forced-colors:outline-1 forced-colors:outline-[Highlight]"
              : "text-text-2 hover:bg-hover hover:text-text active:bg-press"
          }`}
        >
          {it.icon}
          <span className="flex-1">{it.label}</span>
          {it.attention ? (
            <span className="size-[7px] rounded-full bg-warn" role="img" aria-label="needs attention" />
          ) : (
            it.detail && <span className="text-xs font-normal text-text-3">{it.detail}</span>
          )}
        </button>
      </div>
    );
  };

  return (
    <nav ref={nav} aria-label="Pages" className="glass relative flex min-h-0 flex-col gap-px rounded-panel p-2">
      {pill && (
        <span
          aria-hidden="true"
          className="absolute inset-x-2 rounded-item bg-glass-strong shadow-[inset_0_1px_0_var(--rim),0_1px_3px_rgba(41,37,36,.10)] transition-[transform,height] duration-200 ease-out"
          style={{ top: 0, height: pill.height, transform: `translateY(${pill.top}px)` }}
        />
      )}
      {items.map(item)}
      <div className="mt-auto flex flex-col gap-px">
        {footer.map(item)}
        {status && <div className="flex items-center gap-2 px-2.5 pb-1 pt-2 text-xs text-text-3">{status}</div>}
      </div>
    </nav>
  );
}
