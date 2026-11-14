import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";

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
  return (
    <Context.Provider value={value}>
      {children}
      <div role="status" aria-live="polite" className="pointer-events-none fixed bottom-[18px] left-1/2 z-[60] flex w-[min(520px,calc(100%-32px))] -translate-x-1/2 flex-col items-center gap-2">
        {toasts.map((t) => (
          <div key={t.id} className="backdrop pop-frame pointer-events-auto flex min-h-11 animate-[rise_.16s_var(--ease-out)] items-center gap-3 rounded-full py-1.5 pl-[18px] pr-1.5 text-[13px]">
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
