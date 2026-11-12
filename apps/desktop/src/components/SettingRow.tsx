import type { ReactNode } from "react";

// A setting: what it is and what it does on the left, its control on the right.
export function SettingRow({ title, children, control }: { title: string; children?: ReactNode; control: ReactNode }) {
  return (
    <li className="flex items-center justify-between gap-6 border-b border-line px-1 py-3.5">
      <div className="min-w-0">
        <div className="text-sm font-medium">{title}</div>
        {children && <p className="m-0 mt-0.5 max-w-[60ch] text-[13px] text-text-2">{children}</p>}
      </div>
      <div className="flex shrink-0 items-center gap-2">{control}</div>
    </li>
  );
}
