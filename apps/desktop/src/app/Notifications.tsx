import { useCallback, useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Page } from "../components/Page";
import { Row, Section } from "../components/Rows";
import { Tag } from "../components/Tag";
import { call, message, onEvent } from "./api";

/** One of kilnd's notifications (ADR 0087 §1). */
export interface Notice {
  id: number;
  /** Unix seconds. */
  at: number;
  kind: "mail" | "process" | "job" | string;
  title: string;
  body: string;
}

const SEEN = "kiln.notifications.seen";

function seen(): number {
  try {
    return Number(localStorage.getItem(SEEN) ?? 0) || 0;
  } catch {
    return 0;
  }
}

function markSeen(id: number) {
  try {
    localStorage.setItem(SEEN, String(id));
  } catch {
    // A private window: the count just doesn't stick.
  }
}

/** How many are newer than the newest one the page showed. */
export function unread(notices: Notice[], seenId: number): number {
  return notices.filter((n) => n.id > seenId).length;
}

/** "just now", "5 min ago", "3 h ago", or the date. */
export function ago(at: number, now = Date.now() / 1000): string {
  const s = Math.max(0, now - at);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(at * 1000).toLocaleDateString([], { day: "numeric", month: "short" });
}

const kindWord: Record<string, string> = { mail: "Mail", process: "Process", job: "Job" };

/** The newest notifications, kept fresh as kilnd sends them. */
export function useNotices() {
  const [notices, setNotices] = useState<Notice[]>([]);
  const [error, setError] = useState<string>();
  const load = useCallback(() => {
    call<{ notifications: Notice[] }>("notifications.list")
      .then((r) => {
        setNotices(r.notifications);
        setError(undefined);
      })
      .catch((e) => setError(message(e)));
  }, []);
  useEffect(() => {
    load();
    let stop: (() => void) | undefined;
    onEvent((note) => note.method === "notification" && load()).then((u) => (stop = u));
    return () => stop?.();
  }, [load]);
  return { notices, error, reload: load };
}

/** The sidebar's count of unread notifications. */
export function useUnread(open: boolean): number {
  const { notices } = useNotices();
  const [seenId, setSeenId] = useState(seen);
  useEffect(() => {
    if (open && notices[0]) {
      markSeen(notices[0].id);
      setSeenId(notices[0].id);
    }
  }, [open, notices]);
  return unread(notices, seenId);
}

// Notifications (ADR 0087 §3): mail caught, a program that failed, a long job done.
export function NotificationsPage() {
  const { notices, error, reload } = useNotices();
  return (
    <Page
      title="Notifications"
      actions={
        notices.length > 0 && (
          <Button size="sm" onClick={() => call("notifications.clear").then(reload).catch(() => {})}>
            Clear
          </Button>
        )
      }
    >
      <p className="m-0 max-w-[68ch] px-1 text-[13px] text-text-2">
        Mail Mailpit caught, a dev process that failed, and a long job when it ends. Kiln keeps the newest 50; Windows shows each too unless it's turned off in Settings.
      </p>
      {error && <p className="m-0 px-1 text-[13px] text-text-2">{error}</p>}
      <Section title="Newest first" meta={notices.length ? String(notices.length) : "none yet"}>
        {notices.map((n) => (
          <Row key={n.id} columns="80px 1fr 90px">
            <Tag>{kindWord[n.kind] ?? n.kind}</Tag>
            <span>
              <span className="block text-[13px]">{n.title}</span>
              {n.body && <span className="mt-0.5 block text-xs text-text-2">{n.body}</span>}
            </span>
            <span className="figures text-right text-xs text-text-3">{ago(n.at)}</span>
          </Row>
        ))}
      </Section>
    </Page>
  );
}
