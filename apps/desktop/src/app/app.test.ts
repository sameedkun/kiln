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
const { mainPort } = await import("./Home");

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
  const instance = (name: string, enabled: boolean, running: boolean) => ({ name, kind: "x", version: "1", ports: {}, main_port: null, used_by: [], web: null, databases: false, enabled, running, project: null, env: [] as [string, string][] });

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

describe("a service's port", () => {
  it("is the one apps connect to, not the first by name", () => {
    const mailpit = { name: "mailpit-1", kind: "mailpit", version: "1.31.6", ports: { http: 8025, smtp: 1025 }, main_port: 1025, used_by: [], web: null, databases: false, enabled: true, running: true, project: null, env: [] as [string, string][] };
    expect(mainPort(mailpit)).toBe(1025);
  });
});

describe("names and targets typed in the app (ADR 0055 §3)", async () => {
  const { siteName, proxyTarget } = await import("./api");

  it("takes site names that no command line could read as an option", () => {
    expect(siteName("Shop")).toBe("shop");
    expect(siteName("my-shop.test")).toBe("my-shop");
    for (const bad of ["-x", "--json", "a b", "shop/../x", "", "-", "a-"]) expect(siteName(bad), bad).toBeNull();
  });

  it("takes a proxy's target as a port or a loopback address with one", () => {
    expect(proxyTarget("5173")).toBe("5173");
    expect(proxyTarget("127.0.0.1:3000")).toBe("127.0.0.1:3000");
    expect(proxyTarget("http://localhost:8080")).toBe("http://localhost:8080");
    for (const bad of ["0", "70000", "example.com:80", "10.0.0.5:80", "--confirmed", "5173; rm"]) expect(proxyTarget(bad), bad).toBeNull();
  });
});

describe("a site's framework", async () => {
  const { framework } = await import("./SitesPage");
  const site = (driver: string, framework_version: string | null) =>
    ({ driver, framework_version }) as unknown as Parameters<typeof framework>[0];

  it("is a word, with its version when Kiln read one", () => {
    expect(framework(site("laravel", "12.38.1"))).toBe("Laravel 12.38");
    expect(framework(site("wordpress", "6.8.3"))).toBe("WordPress 6.8");
    expect(framework(site("php-public", null))).toBe("PHP");
  });
});

describe("the Services page's helpers (ADR 0056)", async () => {
  const { categoryOf, databaseName } = await import("./ServicesPage");

  it("groups each kind", () => {
    expect(categoryOf("mariadb")).toBe("databases");
    expect(categoryOf("redis")).toBe("cache");
    expect(categoryOf("reverb")).toBe("realtime");
    expect(categoryOf("something-new")).toBe("other");
  });

  it("takes database names, never options or paths", () => {
    expect(databaseName(" shop ")).toBe("shop");
    expect(databaseName("shop_test")).toBe("shop_test");
    for (const bad of ["--all", "-x", "a b", "../x", "x;drop"]) expect(databaseName(bad), bad).toBeNull();
  });
});

describe("first-run onboarding (ADR 0057)", async () => {
  const { steps, nextStep } = await import("./Onboarding");
  const fresh = {
    ready: false,
    web: { installed: false, running: false },
    php: { versions: [], default: null },
    composer: false,
    node: 0,
    ca: { exists: false, trusted: null },
    hosts: { sites: 0, missing: [] },
    shims_on_path: false,
    login_item: "none" as const,
  };
  const at = (id: string) => steps.findIndex((s) => s.id === id);

  it("walks every step on a fresh PC", () => {
    expect(nextStep(fresh, at("welcome"))).toBe(at("web"));
    expect(nextStep(fresh, at("web"))).toBe(at("php"));
  });

  it("skips what's already done", () => {
    const halfway = { ...fresh, web: { installed: true, running: true }, php: { versions: ["8.4.27"], default: "8.4.27" }, ca: { exists: true, trusted: true } };
    expect(nextStep(halfway, at("welcome"))).toBe(at("projects"));
    const finished = { ...halfway, hosts: { sites: 2, missing: [] }, shims_on_path: true, login_item: "app" as const };
    expect(nextStep(finished, at("welcome"))).toBe(at("done"));
  });

  it("doesn't count HTTPS done while Windows doesn't trust the CA", () => {
    const https = steps[at("https")];
    expect(https.done({ ...fresh, ca: { exists: true, trusted: false } })).toBe(false);
    expect(https.done({ ...fresh, ca: { exists: true, trusted: true } })).toBe(true);
  });
});

describe("history, doctor and settings (ADR 0058)", async () => {
  const { byDay, dayOf, byArea } = await import("./HistoryDoctor");
  const { portError } = await import("./Pages");
  const now = new Date(2026, 10, 13, 15, 0);
  const op = (id: number, at: Date | null) => ({ id, at_ms: at ? at.getTime() : null, state: "in effect", kind: "x", description: `op ${id}` });

  it("names days relative to today", () => {
    expect(dayOf(new Date(2026, 10, 13, 0, 5).getTime(), now)).toBe("Today");
    expect(dayOf(new Date(2026, 10, 12, 23, 59).getTime(), now)).toBe("Yesterday");
    expect(dayOf(null, now)).toBe("Earlier");
  });

  it("groups operations by day, keeping their order", () => {
    const groups = byDay([op(3, new Date(2026, 10, 13, 9)), op(2, new Date(2026, 10, 13, 8)), op(1, new Date(2026, 10, 12, 8))], now);
    expect(groups.map(([day, ops]) => [day, ops.map((o) => o.id)])).toEqual([
      ["Today", [3, 2]],
      ["Yesterday", [1]],
    ]);
  });

  it("groups findings by area in the doctor's order", () => {
    const f = (area: string, message: string) => ({ area, status: "ok" as const, message });
    expect(byArea([f("web", "a"), f("php", "b"), f("web", "c")]).map(([a, g]) => [a, g.length])).toEqual([
      ["web", 2],
      ["php", 1],
    ]);
  });

  it("checks a typed port", () => {
    expect(portError("8080")).toBeUndefined();
    expect(portError("0")).toBeDefined();
    expect(portError("70000")).toBeDefined();
    expect(portError("80a")).toBeDefined();
    expect(portError("")).toBeDefined();
  });
});

describe("log lines (ADR 0059)", async () => {
  const { parseLine, parseLines, levelOf } = await import("./LogsPage");

  it("reads Caddy's JSON", () => {
    const line = parseLine('{"level":"error","ts":1794570330.03,"logger":"http","msg":"listen failed","address":":80"}');
    expect(line?.level).toBe("error");
    expect(line?.text).toBe("http: listen failed  address=:80");
    expect(line?.time).toMatch(/^\d\d:\d\d:\d\d$/);
  });

  it("reads Laravel's lines and joins its stack traces", () => {
    const lines = parseLines(["[2026-11-13 10:00:01] local.ERROR: Boom {\"exception\":\"x\"}", "[stacktrace]", "#0 /app/index.php(1): run()", "#1 {main}", '"}', "[2026-11-13 10:00:02] local.INFO: fine"]);
    expect(lines).toHaveLength(2);
    expect(lines[0]).toMatchObject({ time: "10:00:01", level: "error" });
    expect(lines[0].text.split("\n")).toHaveLength(5);
    expect(lines[1]).toMatchObject({ level: "info", text: "fine" });
  });

  it("reads PHP's error log", () => {
    expect(parseLine("[13-Nov-2026 10:00:00 UTC] PHP Warning:  Undefined variable $x in /a.php on line 3")?.level).toBe("warn");
    expect(parseLine("[13-Nov-2026 10:00:00 UTC] PHP Fatal error:  Uncaught Error")?.level).toBe("error");
    expect(parseLine("[13-Nov-2026 10:00:00 UTC] PHP Deprecated:  old")?.level).toBe("info");
  });

  it("reads Kiln's own logs and timestamped service lines", () => {
    expect(parseLine("2026-11-13 11:57:55.123456789 +00:00:00 repaired: the web server")?.text).toBe("repaired: the web server");
    expect(parseLine("1794570330 8.4 :9200 failed (non-zero exit); restarting in 500 ms")?.text).toMatch(/^8\.4 :9200/);
    expect(parseLine("2026-11-13T10:00:00.123456Z 0 [Warning] [MY-010068] [Server] CA certificate is self signed.")?.level).toBe("warn");
    expect(parseLine("2026-11-13T10:00:00.123456Z 0 [System] [MY-010931] ready for connections.")?.level).toBe("info");
    expect(parseLine("1234:M 13 Nov 2026 10:00:00.123 # WARNING overcommit")?.level).toBe("warn");
  });

  it("shows anything else as it is", () => {
    expect(parseLine("plain words")).toEqual({ time: "", level: "info", text: "plain words" });
    expect(parseLine("something ERROR happened")?.level).toBe("error");
    expect(levelOf("CRITICAL")).toBe("error");
    expect(levelOf("notice")).toBe("info");
  });
});

describe("fixes from the user's run (P4C-01)", async () => {
  const { parseLine } = await import("./LogsPage");
  const { fewNames } = await import("./hooks");
  const { addsNames, progressText } = await import("./Change");

  it("reads artisan's queue and schedule lines", () => {
    expect(parseLine("   INFO  Processing jobs from the [default] queue.")).toEqual({ time: "", level: "info", text: "Processing jobs from the [default] queue." });
    expect(parseLine("  2026-11-14 10:00:00 App\\Jobs\\Send ........ 12ms FAIL")).toMatchObject({ time: "10:00:00", level: "error" });
    expect(parseLine("  2026-11-14 10:00:00 App\\Jobs\\Send ........ RUNNING")?.text).toBe("App\\Jobs\\Send · RUNNING");
  });

  it("lists two names and a count", () => {
    expect(fewNames(["a.test"])).toBe("a.test");
    expect(fewNames(["a.test", "b.test", "c.test"])).toBe("a.test, b.test, c.test");
    expect(fewNames(["a.test", "b.test", "c.test", "d.test", "e.test"])).toBe("a.test, b.test and 3 more");
  });

  it("knows which changes add names for the hosts file", () => {
    expect(addsNames(["link", "shop", "--path", "D:\\shop"])).toBe(true);
    expect(addsNames(["park", "D:\\Laravel"])).toBe(true);
    expect(addsNames(["tool", "add", "adminer"])).toBe(true);
    expect(addsNames(["tool", "remove", "adminer"])).toBe(false);
    expect(addsNames(["service", "stop", "mysql-9.7"])).toBe(false);
  });

  it("says a download's progress in a few words", () => {
    expect(progressText({ type: "progress", label: "Downloading mysql.zip", done: 34 * 1_048_576, total: 120 * 1_048_576 })).toBe("Downloading mysql.zip · 34 of 120 MB");
  });
});

describe("a site's processes (ADR 0069)", async () => {
  const { siteInstances, nextWorkerName } = await import("./SitePage");
  const inst = (name: string, kind: string, project = "D:/shop") => ({ name, kind, version: "1", ports: {}, main_port: null, used_by: [], web: null, databases: false, enabled: true, running: true, project, env: [] as [string, string][] });

  it("names a site's processes in order, numbering extra queue workers", () => {
    const all = [inst("queue-shop-10", "queue"), inst("vite-shop", "vite"), inst("queue-shop-2", "queue"), inst("queue-shop", "queue"), inst("queue-blog", "queue", "D:/blog")];
    expect(siteInstances(all, "D:/shop").map((p) => [p.instance.name, p.label])).toEqual([
      ["queue-shop", "Queue worker"],
      ["queue-shop-2", "Queue worker 2"],
      ["queue-shop-10", "Queue worker 3"],
      ["vite-shop", "Vite"],
    ]);
  });

  it("picks the next free worker name, up to eight", () => {
    expect(nextWorkerName([])).toBeNull();
    expect(nextWorkerName([inst("queue-shop", "queue")])).toBe("queue-shop-2");
    expect(nextWorkerName([inst("queue-shop", "queue"), inst("queue-shop-3", "queue")])).toBe("queue-shop-2");
    const eight = [inst("queue-shop", "queue"), ...[2, 3, 4, 5, 6, 7, 8].map((n) => inst(`queue-shop-${n}`, "queue"))];
    expect(nextWorkerName(eight)).toBeNull();
  });
});

describe("a new site from a template (ADR 0070)", async () => {
  const { newArgs, templates } = await import("./NewSiteDialog");

  it("offers the CLI's templates, Laravel first", () => {
    expect(templates.map((t) => t.id)).toEqual(["laravel", "react", "vue", "livewire", "symfony", "bedrock", "empty"]);
  });

  it("passes only what was chosen", () => {
    expect(newArgs({ name: "shop", template: "laravel", folder: "D:\\Code", php: "", secure: false, db: "" })).toEqual(["new", "shop", "--template", "laravel", "--path", "D:\\Code"]);
    expect(newArgs({ name: "shop", template: "react", folder: "D:\\Code", php: "8.4", secure: true, db: "mysql-8.4" })).toEqual([
      "new",
      "shop",
      "--template",
      "react",
      "--path",
      "D:\\Code",
      "--php",
      "8.4",
      "--secure",
      "--db",
      "mysql-8.4",
    ]);
  });
});
