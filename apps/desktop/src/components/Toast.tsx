import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

export interface ToastOptions {
  message: string;
  action?: { label: string; onClick: () => void };
  /** Stays until dismissed or replaced, for work still going on ("Unpacking MySQL…"). */
  sticky?: boolean;
  /** Replaces the toast with this id instead of adding one. */
  replaces?: number;
}
interface Shown extends ToastOptions {
  id: number;
}

interface Toasts {
  show: (toast: ToastOptions) => number;
  dismiss: (id: number) => void;
}
const Context = createContext<Toasts>({ show: () => -1, dismiss: () => {} });

/** Shows a short message at the bottom of the window, such as "Linked api.test · Undo". */
export function useToast() {
  return useContext(Context).show;
}

/** Removes a toast, such as a sticky one whose work has ended. */
export function useDismissToast() {
  return useContext(Context).dismiss;
}

const SHOWN_FOR_MS = 6000;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Shown[]>([]);
  const next = useRef(0);
  const dismiss = useCallback((id: number) => setToasts((all) => all.filter((t) => t.id !== id)), []);
  const show = useCallback(
    (toast: ToastOptions) => {
      const replaced = toast.replaces;
      const id = replaced ?? next.current++;
      setToasts((all) =>
        replaced !== undefined && all.some((t) => t.id === replaced)
          ? all.map((t) => (t.id === replaced ? { ...toast, id } : t))
          : [...all.slice(-2), { ...toast, id }],
      );
      if (!toast.sticky) setTimeout(() => dismiss(id), SHOWN_FOR_MS);
      return id;
    },
    [dismiss],
  );
  const value = useMemo(() => ({ show, dismiss }), [show, dismiss]);
  // Dialogs are the browser's modal <dialog>, in its top layer, where no z-index reaches: the
  // toasts go there too (a manual popover), raised again on every change, so a job's progress
  // shows above an open dialog rather than under its backdrop (P4D-03).
  const layer = useRef<HTMLDivElement>(null);
  // Only while a modal dialog is open: otherwise the stack stays an ordinary live region in
  // the page, which screen readers follow best.
  useEffect(() => {
    const el = layer.current as (HTMLDivElement & { showPopover?: () => void; hidePopover?: () => void }) | null;
    if (!el || typeof el.showPopover !== "function" || typeof el.hidePopover !== "function") return;
    try {
      const open = el.hasAttribute("popover") && el.matches(":popover-open");
      if (toasts.length && document.querySelector("dialog[open]")) {
        if (open) el.hidePopover();
        if (!el.hasAttribute("popover")) el.setAttribute("popover", "manual");
        el.showPopover();
      } else if (el.hasAttribute("popover")) {
        if (open) el.hidePopover();
        el.removeAttribute("popover");
      }
    } catch {
      // A browser without the Popover API keeps them in the page.
    }
  }, [toasts]);
  return (
    <Context.Provider value={value}>
      {children}
      <div
        ref={layer}
        role="status"
        aria-live="polite"
        className="pointer-events-none fixed inset-auto bottom-[18px] left-1/2 z-[60] m-0 flex w-[min(520px,calc(100%-32px))] -translate-x-1/2 flex-col items-center gap-2 overflow-visible border-0 bg-transparent p-0"
      >
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`backdrop pop-frame pointer-events-auto flex min-h-11 animate-[rise_.16s_var(--ease-out)] items-center gap-3 rounded-full py-1.5 pl-[18px] text-[13px] ${t.action ? "pr-1.5" : "pr-[18px]"}`}
          >
            {t.sticky && <span aria-hidden className="size-3 shrink-0 animate-spin rounded-full border-2 border-text-3 border-t-ember" />}
            <span>{t.message}</span>
            {t.action && (
              <button
                type="button"
                onClick={() => {
                  t.action?.onClick();
                  dismiss(t.id);
                }}
                className="h-[30px] rounded-full px-3 font-medium text-text hover:bg-hover"
              >
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>
    </Context.Provider>
  );
}
