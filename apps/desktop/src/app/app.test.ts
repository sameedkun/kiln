import { beforeEach, describe, expect, it, vi } from "vitest";

// Tauri, mocked: `invoke` answers from `answers`, `listen` keeps the handler so tests can
// send the daemon's events.
const answers: Record<string, (args: Record<string, unknown>) => unknown> = {};
let handler: ((e: { payload: unknown }) => void) | null = null;
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: Record<string, unknown>) => {
    const key = command === "api" ? (args.method as string) : command;
    const answer = answers[key];
    if (!answer) throw new Error(`no answer for ${key}`);
    return answer(command === "api" ? ((args.params as Record<string, unknown>) ?? {}) : args);
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, h: (e: { payload: unknown }) => void) => {
    handler = h;
    return () => (handler = null);
  }),
}));

const { runJob } = await import("./api");
const { trayState } = await import("./MainApp");

const send = (method: string, params: Record<string, unknown>) => handler?.({ payload: { method, params } });

beforeEach(() => {
  for (const key of Object.keys(answers)) delete answers[key];
  handler = null;
});

describe("runJob", () => {
  it("passes on the job's lines and resolves with how it finished", async () => {
    answers["jobs.start"] = () => {
      // The job's first lines can arrive before its id does.
      queueMicrotask(() => send("job.event", { job: 7, event: { type: "note", text: "Started mailpit-1." } }));
      return { job: 7 };
    };
    answers["jobs.get"] = () => {
      setTimeout(() => {
        send("job.event", { job: 8, event: { type: "note", text: "another job" } });
        send("job.finished", { job: 7, ok: true, exit: 0, needs_confirmation: false, last: { type: "result" } });
      });
      return { running: true, finished: null, events: [] };
    };
    const lines: string[] = [];
    const end = await runJob(["service", "start", "mailpit-1"], "apply", (e) => lines.push(e.text ?? ""));
    expect(end.ok).toBe(true);
    expect(lines).toEqual(["Started mailpit-1."]);
    expect(handler).toBeNull();
  });

  it("takes the end from the job's record when it finished before its events came", async () => {
    answers["jobs.start"] = () => ({ job: 3 });
    answers["jobs.get"] = () => ({
      running: false,
      finished: { job: 3, ok: false, exit: 3, needs_confirmation: true, last: { type: "result" } },
      events: [{ type: "preview", summary: "Undo #12" }, { type: "confirm", question: "Undo it?" }],
    });
    const lines: string[] = [];
    const end = await runJob(["undo", "12"], "preview", (e) => lines.push(e.type));
    expect(end.needs_confirmation).toBe(true);
    expect(lines).toEqual(["preview", "confirm"]);
  });

  it("stops listening when the daemon refuses the job", async () => {
    answers["jobs.start"] = () => {
      throw "`kiln uninstall` can't be run through the API";
    };
    await expect(runJob(["uninstall"], "apply")).rejects.toContain("can't be run");
    expect(handler).toBeNull();
  });
});

describe("the tray icon", () => {
  const instance = (name: string, enabled: boolean, running: boolean) => ({ name, kind: "x", version: "1", ports: {}, enabled, running, project: null });

  it("is stopped without the daemon, and says why", () => {
    expect(trayState("unreachable")).toEqual({ state: "stopped", tooltip: "Kiln: not running" });
    expect(trayState("too-old").tooltip).toContain("update Kiln");
  });

  it("asks for attention when a service that should run doesn't", () => {
    const services = { supervisor_running: true, instances: [instance("mysql-8.4", true, true), instance("redis-8", true, false), instance("pg", false, false)] };
    expect(trayState("running", services)).toEqual({ state: "attention", tooltip: "Kiln: redis-8 not running" });
  });

  it("is quiet when everything that should run does", () => {
    const services = { supervisor_running: true, instances: [instance("mysql-8.4", true, true), instance("pg", false, false)] };
    expect(trayState("running", services)).toEqual({ state: "running", tooltip: "Kiln: 1 service running" });
  });
});
