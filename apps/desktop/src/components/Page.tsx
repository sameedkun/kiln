import { useState, type ReactNode } from "react";

// A page: its header floats as glass over the content once the content scrolls under it.
// The header sits beside the scroller, not inside it, so the scroller's fade (which keeps
// content from peeking round the header's corners) never fades the header itself.
export function Page({ title, actions, children }: { title: string; actions?: ReactNode; children: ReactNode }) {
  const [under, setUnder] = useState(false);
  return (
    <main className="relative min-h-0 overflow-hidden rounded-panel">
      <div
        className={`absolute left-0 right-3 top-1.5 z-[2] flex h-14 items-center gap-2 rounded-panel border pl-7 pr-4 ${
          under ? "glass" : "border-transparent"
        }`}
      >
        <h1 className="m-0 mr-auto text-xl leading-7 font-[650] tracking-[-.02em]">{title}</h1>
        {actions}
      </div>
      <div
        tabIndex={-1}
        onScroll={(e) => setUnder(e.currentTarget.scrollTop > 4)}
        className="fade-top h-full overflow-auto pt-[66px] [scroll-padding-top:72px] [scrollbar-width:thin] focus:outline-none"
      >
        <div className="max-w-[980px] px-7 pb-10">{children}</div>
      </div>
    </main>
  );
}
