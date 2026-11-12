import {
  Delete16Regular,
  Folder16Regular,
  LockClosed12Regular,
  MoreHorizontal16Regular,
  Open16Regular,
  Play16Filled,
  Stop16Filled,
} from "@fluentui/react-icons";
import type { ReactNode } from "react";
import { Button } from "../../components/Button";
import { Menu, type MenuItem } from "../../components/Menu";
import type { StatusState } from "../../components/Status";

export type Go = (page: string) => void;

export const lock = <LockClosed12Regular aria-label="HTTPS" className="shrink-0 text-text-3" />;
export const noLock = <LockClosed12Regular aria-hidden="true" className="invisible shrink-0" />;

export function More({ name, items }: { name: string; items?: MenuItem[] }) {
  return (
    <Menu
      label={`More for ${name}`}
      trigger={(props) => <Button variant="quiet" aria-label={`More for ${name}`} icon={<MoreHorizontal16Regular />} {...props} />}
      items={
        items ?? [
          { label: "Open in the browser", icon: <Open16Regular />, onSelect: () => {} },
          { label: "Open the folder", icon: <Folder16Regular />, onSelect: () => {} },
          { label: "Unlink…", icon: <Delete16Regular />, danger: true, groupStart: true, onSelect: () => {} },
        ]
      }
    />
  );
}

export function SiteActions({ name, folder }: { name: string; folder?: boolean }) {
  return (
    <>
      {folder && <Button variant="quiet" aria-label={`Open ${name}'s folder`} icon={<Folder16Regular />} />}
      <Button variant="quiet" aria-label={`Open ${name}`} icon={<Open16Regular />} />
      <More name={name} />
    </>
  );
}

export function StartStop({ name, state }: { name: string; state: StatusState }) {
  const running = state === "running";
  return <Button variant="quiet" aria-label={`${running ? "Stop" : "Start"} ${name}`} icon={running ? <Stop16Filled /> : <Play16Filled />} />;
}

/** A label above a group of figures or controls. */
export function Label({ children }: { children: ReactNode }) {
  return <div className="mb-1 text-xs font-medium text-text-3">{children}</div>;
}

/** A figure with what it means under it: used sparingly, on detail pages only. */
export function Figure({ value, label, tone }: { value: string; label: string; tone?: "warn" }) {
  return (
    <div>
      <div className={`figures text-[22px] leading-7 font-semibold tracking-[-.02em] ${tone === "warn" ? "text-warn" : ""}`}>{value}</div>
      <div className="text-xs text-text-2">{label}</div>
    </div>
  );
}
