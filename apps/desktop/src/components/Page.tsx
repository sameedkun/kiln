import { useState, type ReactNode } from "react";

// A page: its header floats as glass over the content once the content scrolls under it.
// The header sits beside the scroller, not inside it, so the scroller's fade (which keeps
// content from peeking round the header's corners) never fades the header itself.
export function Page({
  title,
  leading,
  after,
  actions,
  children,
}: {
  title: string;
  /** Before the title: a way back. */
  leading?: ReactNode;
  /** Right after the title: a tag or a status. */
  after?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const [under, setUnder] = useState(false);
  return (
    <main className="relative min-h-0 overflow-hidden rounded-panel">
      <div
        className={`absolute left-0 right-3 top-1.5 z-[2] flex h-14 items-center gap-2 rounded-panel border pr-4 ${leading ? "pl-3" : "pl-7"} ${
          under ? "glass" : "border-transparent"
        }`}
      >
        {leading}
        <h1 className="m-0 text-xl leading-7 font-[650] tracking-[-.02em]">{title}</h1>
        <span className="mr-auto flex items-center gap-2 pl-1">{after}</span>
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
