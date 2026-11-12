import { Search16Regular } from "@fluentui/react-icons";
import { Logo } from "./Logo";

// Kiln's own title bar (ADR 0051 §3): the mark, "Go to…" (Ctrl+K), and Windows' caption
// buttons on the right. `data-tauri-drag-region` lets the window be dragged by it (P4-03).
export function TitleBar({
  onGoTo,
  onMinimise,
  onMaximise,
  onClose,
}: {
  onGoTo?: () => void;
  onMinimise?: () => void;
  onMaximise?: () => void;
  onClose?: () => void;
}) {
  const cap = "grid w-[46px] place-items-center text-text hover:bg-hover";
  return (
    <header data-tauri-drag-region className="relative flex h-11 select-none items-center gap-2.5 pl-4">
      <div data-tauri-drag-region className="flex w-[204px] items-center gap-2 text-[13px] font-semibold tracking-[-.01em]">
        <Logo />
        Kiln
      </div>
      <button
        type="button"
        onClick={onGoTo}
        className="mx-auto flex h-[30px] w-[min(420px,40%)] min-w-0 items-center gap-2 overflow-hidden whitespace-nowrap rounded-control border border-line bg-hover pl-2.5 pr-2 text-left text-[13px] text-text-3 hover:border-line-strong"
      >
        <Search16Regular aria-hidden="true" className="shrink-0" />
        <span className="truncate">Go to a site, service or command</span>
        <kbd className="ml-auto rounded border border-line-strong px-[5px] font-sans text-[11px] text-text-3">Ctrl K</kbd>
      </button>
      <div className="flex self-stretch">
        <button type="button" className={cap} aria-label="Minimise" onClick={onMinimise}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M0 5.5h10" stroke="currentColor" />
          </svg>
        </button>
        <button type="button" className={cap} aria-label="Maximise" onClick={onMaximise}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x=".5" y=".5" width="9" height="9" rx="1" fill="none" stroke="currentColor" />
          </svg>
        </button>
        <button
          type="button"
          className={`${cap} hover:bg-[#c42b1c] hover:text-white`}
          aria-label="Close to the tray"
          title="Close (Kiln keeps running in the tray)"
          onClick={onClose}
        >
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M0 0l10 10M10 0 0 10" stroke="currentColor" />
          </svg>
        </button>
      </div>
    </header>
  );
}
