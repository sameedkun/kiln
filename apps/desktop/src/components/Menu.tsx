import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  /** A change that removes something: shown in red. */
  danger?: boolean;
  /** A divider above this item. */
  groupStart?: boolean;
}

// A menu opened from a button: arrow keys move, Enter chooses, Esc or a click elsewhere
// closes it and returns focus to the button.
export function Menu({ trigger, label, items }: { trigger: (props: MenuTriggerProps) => ReactNode; label: string; items: MenuItem[] }) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement | null>(null);
  const id = useId();

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    wrap.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const away = (e: MouseEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [open]);

  const move = (e: KeyboardEvent) => {
    const list = [...(wrap.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const at = list.indexOf(document.activeElement as HTMLElement);
    const go = (i: number) => list[(i + list.length) % list.length]?.focus();
    if (e.key === "ArrowDown") go(at + 1);
    else if (e.key === "ArrowUp") go(at - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(list.length - 1);
    else if (e.key === "Escape" || e.key === "Tab") return close(e.key === "Escape");
    else return;
    e.preventDefault();
  };

  return (
    <div ref={wrap} className="relative inline-block">
      {trigger({
        ref: (el) => {
          button.current = el;
        },
        "aria-haspopup": "menu",
        "aria-expanded": open,
        "aria-controls": open ? id : undefined,
        onClick: () => setOpen((o) => !o),
      })}
      {open && (
        <div
          id={id}
          role="menu"
          aria-label={label}
          onKeyDown={move}
          className="glass-pop absolute right-0 top-full z-50 mt-1 min-w-[200px] animate-[pop_.1s_var(--ease-out)] rounded-pop p-[5px]"
        >
          {items.map((it) => (
            <div key={it.label}>
              {it.groupStart && <hr className="mx-1 my-1 border-0 border-t border-line" />}
              <button
                type="button"
                role="menuitem"
                tabIndex={-1}
                onClick={() => {
                  close(true);
                  it.onSelect();
                }}
                className={`flex h-8 w-full items-center gap-2.5 rounded-md px-3 text-left text-[13px] outline-none hover:bg-hover focus-visible:bg-hover focus:bg-hover ${
                  it.danger ? "text-bad" : ""
                }`}
              >
                {it.icon}
                {it.label}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export interface MenuTriggerProps {
  ref: (el: HTMLButtonElement | null) => void;
  "aria-haspopup": "menu";
  "aria-expanded": boolean;
  "aria-controls"?: string;
  onClick: () => void;
}
