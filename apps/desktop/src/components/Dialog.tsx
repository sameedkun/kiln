import { useEffect, useId, useRef, type ReactNode } from "react";

// A modal dialog on the browser's own <dialog>: it keeps focus inside, Esc closes it, and
// focus returns to what opened it.
export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
  wide,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
    } else if (!open && dialog.open) {
      if (typeof dialog.close === "function") dialog.close();
      else dialog.removeAttribute("open");
    }
  }, [open]);

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          onClose();
        }
      }}
      className={`glass-pop m-auto max-h-[calc(100%-48px)] rounded-[18px] p-0 text-text backdrop:bg-[var(--smoke)] open:animate-[pop_.14s_var(--ease-out)] ${
        wide ? "w-[min(640px,calc(100%-48px))]" : "w-[min(460px,calc(100%-48px))]"
      }`}
    >
      <div className="px-6 pb-5 pt-[22px]">
        <h2 id={titleId} className="m-0 mb-2.5 text-lg leading-7 font-semibold tracking-[-.015em]">
          {title}
        </h2>
        <div className="text-sm text-text-2">{children}</div>
      </div>
      {footer && <div className="flex justify-end gap-2 px-6 pb-[22px]">{footer}</div>}
    </dialog>
  );
}
