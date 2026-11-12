import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";

export interface ToastOptions {
  message: string;
  action?: { label: string; onClick: () => void };
}
interface Shown extends ToastOptions {
  id: number;
}

const Context = createContext<(toast: ToastOptions) => void>(() => {});

/** Shows a short message at the bottom of the window, such as "Linked api.test · Undo". */
export function useToast() {
  return useContext(Context);
}

const SHOWN_FOR_MS = 6000;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Shown[]>([]);
  const next = useRef(0);
  const dismiss = useCallback((id: number) => setToasts((all) => all.filter((t) => t.id !== id)), []);
  const show = useCallback(
    (toast: ToastOptions) => {
      const id = next.current++;
      setToasts((all) => [...all.slice(-2), { ...toast, id }]);
      setTimeout(() => dismiss(id), SHOWN_FOR_MS);
    },
    [dismiss],
  );
  return (
    <Context.Provider value={show}>
      {children}
      <div role="status" aria-live="polite" className="pointer-events-none fixed bottom-[18px] left-1/2 z-[60] flex w-[min(520px,calc(100%-32px))] -translate-x-1/2 flex-col items-center gap-2">
        {toasts.map((t) => (
          <div key={t.id} className="glass-pop pointer-events-auto flex min-h-11 animate-[rise_.16s_var(--ease-out)] items-center gap-3 rounded-full py-1.5 pl-[18px] pr-1.5 text-[13px]">
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
