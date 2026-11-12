import type { ReactNode } from "react";

export function Tag({ children }: { children: ReactNode }) {
  return (
    <span className="inline-block rounded-[5px] border border-line-strong px-[5px] text-[11.5px] leading-[17px] font-medium text-text-2">
      {children}
    </span>
  );
}
