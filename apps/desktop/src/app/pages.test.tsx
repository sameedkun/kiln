import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The real pages, rendered against canned daemon answers: what the gallery's mock screens
// don't cover (ADR 0058, ADR 0059).
const answers: Record<string, (args: Record<string, unknown>) => unknown> = {};
const asked: { key: string; args: Record<string, unknown> }[] = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args: Record<string, unknown>) => {
    const key = command === "api" ? (args.method as string) : command;
    const params = command === "api" ? ((args.params as Record<string, unknown>) ?? {}) : args;
    asked.push({ key, args: params });
    const answer = answers[key];
    if (!answer) throw new Error(`no answer for ${key}`);
    return answer(params);
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const { ToastProvider } = await import("../components/Toast");
const { ChangeProvider } = await import("./Change");
const { HistoryPage, DoctorPage } = await import("./HistoryDoctor");
const { LogsPage } = await import("./LogsPage");
const { Settings } = await import("./Pages");
const { SitePage } = await import("./SitePage");
const { ServicePage, size } = await import("./ServicePage");
const { SitesPage, groupName } = await import("./SitesPage");

const shell = (page: ReactNode) =>
  render(
    <ToastProvider>
      <ChangeProvider>{page}</ChangeProvider>
    </ToastProvider>,
  );

beforeEach(() => {
  for (const key of Object.keys(answers)) delete answers[key];
  asked.length = 0;
});
afterEach(cleanup);

describe("History", () => {
  it("groups changes by day and offers Undo only on what's in effect", async () => {
    const today = Date.now();
    answers["history.list"] = ({ all }) => ({
      hidden: all ? 0 : 1,
      operations: [
        { id: 12, at_ms: today, state: "in effect", kind: "link", description: "Link shop.test" },
        ...(all ? [{ id: 11, at_ms: today, state: "undone", kind: "park", description: "Park C:\\Sites" }] : []),
      ],
    });
    shell(<HistoryPage />);
    expect(await screen.findByText("Link shop.test")).toBeTruthy();
    expect(screen.getByRole("region", { name: "Today" })).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Undo" })).toHaveLength(1);

    await userEvent.click(screen.getByRole("switch", { name: "Show undone" }));
    expect(await screen.findByText("Park C:\\Sites")).toBeTruthy();
    expect(screen.getByText("undone")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Undo" })).toHaveLength(1);
  });

  it("says when nothing is in effect", async () => {
    answers["history.list"] = () => ({ hidden: 2, operations: [] });
    shell(<HistoryPage />);
    expect(await screen.findByText(/2 earlier changes were undone/)).toBeTruthy();
  });
});

describe("Doctor", () => {
  it("shows what needs a look, with fixes, and the rest on request", async () => {
    answers["doctor.report"] = () => ({
      findings: [
        { area: "web", status: "ok", message: "Caddy installed" },
        { area: "dns", status: "warn", message: "in the hosts file but no longer sites: old.test", fix: "kiln hosts sync" },
      ],
    });
    shell(<DoctorPage />);
    expect(await screen.findByText(/1 thing needs a look/)).toBeTruthy();
    expect(screen.getByText("kiln hosts sync")).toBeTruthy();
    expect(screen.queryByText("Caddy installed")).toBeNull();
    await userEvent.click(screen.getByRole("switch", { name: "Show what's fine" }));
    expect(screen.getByText("Caddy installed")).toBeTruthy();
  });
});

describe("Logs", () => {
  it("lists the logs by group and follows the first that exists", async () => {
    answers["logs.sources"] = () => ({
      sources: [
        { id: "kiln", group: "Kiln", label: "Kiln's daemon", path: "C:\\k\\kilnd.log", exists: false, bytes: null, modified_ms: null },
        { id: "web", group: "Kiln", label: "Web server", path: "C:\\k\\caddy.log", exists: true, bytes: 10, modified_ms: 1 },
        { id: "service:mysql-8.4", group: "Services", label: "mysql-8.4", path: "C:\\k\\mysql.log", exists: true, bytes: 10, modified_ms: 1 },
      ],
    });
    answers["logs.tail"] = ({ source }) => ({
      source,
      path: "C:\\k\\caddy.log",
      lines: ['{"level":"error","ts":1794570330,"msg":"listen failed"}', '{"level":"info","ts":1794570331,"msg":"serving"}'],
      offset: 100,
      reset: false,
    });
    shell(<LogsPage />);
    expect(await screen.findByText("listen failed")).toBeTruthy();
    expect(asked.find((a) => a.key === "logs.tail")?.args).toMatchObject({ source: "web" });
    expect(screen.getByRole("navigation", { name: "Logs" }).textContent).toContain("none yet");
    expect(screen.getByRole("log", { name: "Web server log" })).toBeTruthy();

    await userEvent.click(screen.getByRole("radio", { name: /^Errors/ }));
    expect(screen.queryByText("serving")).toBeNull();
    expect(screen.getByText("listen failed")).toBeTruthy();
  });
});

describe("Settings", () => {
  it("shows Kiln's settings and checks the ports typed", async () => {
    answers.settings_get = () => ({ start_in: "window", close_notice_shown: true, onboarding_dismissed: false, window: null });
    answers.login_item = () => "none";
    answers["settings.get"] = () => ({ tld: "test", http: { port: 8080, auto: true }, https: { port: 8443, auto: true }, php_workers: 4 });
    answers["setup.status"] = () => ({ ready: true, web: { installed: true, running: true } });
    answers["updates.check"] = () => ({
      php: [],
      kiln: { version: "0.1.0", checking: true, latest: "v0.2.0", update: true, error: null, page: "https://github.com/sameedkun/kiln/releases" },
    });
    answers["footprint.get"] = () => ({
      processes: [{ label: "PHP 8.4.27 workers", count: 4, memory_bytes: 90_000_000, cpu_percent: 0.4 }],
      memory_bytes: 90_000_000,
      folders: [{ label: "Download cache", path: "C:/k/downloads", bytes: 104_857_600 }],
      disk_bytes: 104_857_600,
      cache_bytes: 104_857_600,
    });
    shell(<Settings setUp={() => {}} />);
    expect(await screen.findByText(/HTTP on 8080, HTTPS on 8443\. Kiln picks them/)).toBeTruthy();
    expect(screen.getByText(".test")).toBeTruthy();
    expect(await screen.findByText("PHP 8.4.27 workers")).toBeTruthy();
    expect(screen.getByText("100.0 MB")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Clear the download cache" })).toBeTruthy();
    expect(screen.getByText(/Kiln v0.2.0 is out; this is 0.1.0/)).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Check GitHub for a new Kiln" })).toBeTruthy();
    expect((screen.getByRole("combobox", { name: "PHP workers" }) as HTMLSelectElement).value).toBe("4");

    await userEvent.click(screen.getByRole("button", { name: "Change…" }));
    const dialog = await screen.findByRole("dialog", { name: "Web server ports" });
    // The dialog isn't inside the settings list.
    expect(dialog.closest("ul")).toBeNull();
    await userEvent.click(screen.getByRole("switch", { name: /Let Kiln pick/ }));
    const http = screen.getByRole("textbox", { name: "HTTP" });
    await userEvent.clear(http);
    await userEvent.type(http, "8443");
    expect(screen.getByText("Not the same as HTTP")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Restart on these ports" }) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("a site's dev processes (ADR 0060)", () => {
  it("shows each process, and starts one for the site's project", async () => {
    const site = {
      host: "shop.test",
      url: "https://shop.test",
      driver: "laravel",
      document_root: "D:/Code/shop/public",
      secure: true,
      source: "link",
      parked_in: null,
      php: { version: "8.4.27", pinned: null, from_project: false, fell_back: false },
      in_hosts: true,
      project: "D:/Code/shop",
      framework_version: "12.38.1",
      node: { version: "22.11.0", asked_by: null, asks_for: null, fell_back: false },
    };
    answers["sites.list"] = () => ({ parked: [], sites: [site], proxies: [], skipped: [], warnings: [] });
    answers["php.list"] = () => ({ versions: [{ version: "8.4.27", installed: true }], default: "8.4.27" });
    answers["ca.status"] = () => ({ intermediate_days_left: 20 });
    answers["services.list"] = () => ({
      supervisor_running: true,
      instances: [{ name: "queue-shop", kind: "queue", version: "12.38.1", ports: {}, used_by: [], web: null, databases: false, main_port: null, enabled: true, running: true, project: site.project }],
    });
    answers["jobs.start"] = () => ({ job: 1 });
    answers["jobs.get"] = () => ({ running: false, finished: { job: 1, ok: true, exit: 0, needs_confirmation: false, last: { type: "result" } }, events: [] });
    shell(<SitePage host="shop.test" go={() => {}} />);
    const section = await screen.findByRole("region", { name: "Dev processes" });
    expect(section.textContent).toContain("Queue worker");
    expect(section.textContent).toContain("Scheduler");
    expect(section.textContent).toContain("Vite");
    expect(await screen.findByRole("button", { name: "Stop the queue worker" })).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Start the scheduler" }));
    await vi.waitFor(() => expect(asked.find((a) => a.key === "jobs.start")?.args).toMatchObject({ args: ["service", "add", "scheduler", "--project", site.project] }));

    // The Requests tab: the site's figures and its slowest requests (ADR 0065).
    answers["requests.get"] = ({ host }) => ({
      log: "C:/k/caddy/access.log",
      sites: [
        {
          host,
          count: 21,
          failures: 1,
          median_ms: 12.5,
          p95_ms: 240,
          since: 1794570330,
          slowest: [{ at: 1794570400, method: "GET", uri: "/reports?year=2026", status: 500, ms: 300 }],
        },
      ],
    });
    await userEvent.click(screen.getByRole("tab", { name: "Requests" }));
    expect(await screen.findByText("12.5 ms")).toBeTruthy();
    expect(screen.getByText("GET /reports?year=2026")).toBeTruthy();
    expect(asked.find((a) => a.key === "requests.get")?.args).toMatchObject({ host: "shop.test" });
  });
});

describe("a service's page (ADR 0068)", () => {
  it("lists its databases with their sites, and asks for the name before deleting one", async () => {
    answers["services.list"] = () => ({
      supervisor_running: true,
      instances: [
        {
          name: "mysql-8.4",
          kind: "mysql",
          version: "8.4.13",
          ports: { mysql: 3306 },
          main_port: 3306,
          used_by: ["shop.test"],
          web: null,
          databases: true,
          enabled: true,
          running: true,
          project: null,
          env: [["DB_CONNECTION", "mysql"], ["DB_PORT", "3306"]],
        },
      ],
    });
    answers["sites.list"] = () => ({ parked: [], sites: [], proxies: [], skipped: [], warnings: [] });
    answers["services.databases"] = ({ name }) => {
      expect(name).toBe("mysql-8.4");
      return { databases: [{ name: "shop", bytes: 48_234_496, used_by: ["shop.test"] }, { name: "scratch", bytes: 0, used_by: [] }] };
    };
    answers["jobs.start"] = () => ({ job: 1 });
    answers["jobs.get"] = () => ({ running: false, finished: { job: 1, ok: true, exit: 0, needs_confirmation: false, last: { type: "result" } }, events: [] });
    shell(<ServicePage name="mysql-8.4" go={() => {}} />);

    expect(await screen.findByRole("heading", { name: "mysql-8.4" })).toBeTruthy();
    expect(screen.getByText("127.0.0.1:3306")).toBeTruthy();
    await userEvent.click(screen.getByRole("tab", { name: "Databases" }));
    expect(await screen.findByText("46.0 MB")).toBeTruthy();
    expect(screen.getByText("no site")).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: "Delete shop" }));
    const del = screen.getByRole("button", { name: "Delete" });
    expect((del as HTMLButtonElement).disabled).toBe(true);
    await userEvent.type(screen.getByLabelText("Type shop to confirm"), "shop");
    expect((del as HTMLButtonElement).disabled).toBe(false);
    await userEvent.click(del);
    await vi.waitFor(() => expect(asked.find((a) => a.key === "jobs.start")?.args).toMatchObject({ args: ["db", "drop", "mysql-8.4", "shop"], mode: "apply" }));

    await userEvent.click(screen.getByRole("tab", { name: ".env" }));
    expect(screen.getByText("DB_CONNECTION")).toBeTruthy();
  });

  it("reads sizes as people do", () => {
    expect(size(0)).toBe("0 B");
    expect(size(2048)).toBe("2.0 KB");
    expect(size(48_234_496)).toBe("46.0 MB");
  });
});

describe("a site at a glance (P4C-05)", () => {
  it("shows the day's requests by hour, the branch, and what it uses", async () => {
    const site = {
      host: "shop.test",
      url: "https://shop.test:8443",
      driver: "laravel",
      framework: "laravel",
      document_root: "D:/Code/shop/public",
      secure: true,
      source: "link",
      parked_in: null,
      php: { version: "8.4.27", pinned: null, from_project: false, fell_back: false },
      in_hosts: true,
      project: "D:/Code/shop",
      framework_version: "12.38.1",
      git_branch: "feature/checkout",
      node: { version: "22.11.0", asked_by: null, asks_for: null, fell_back: false },
    };
    answers["sites.list"] = () => ({ parked: [], sites: [site], proxies: [], skipped: [], warnings: [] });
    answers["php.list"] = () => ({ versions: [{ version: "8.4.27", installed: true }], default: "8.4.27" });
    answers["ca.status"] = () => ({ intermediate_days_left: 20 });
    answers["services.list"] = () => ({
      supervisor_running: true,
      instances: [
        { name: "mysql-8.4", kind: "mysql", version: "8.4.13", ports: { mysql: 3306 }, used_by: ["shop.test"], web: null, databases: true, main_port: 3306, enabled: true, running: true, project: null, env: [] },
        { name: "queue-shop", kind: "queue", version: "12.38.1", ports: {}, used_by: [], web: null, databases: false, main_port: null, enabled: true, running: true, project: site.project, env: [] },
      ],
    });
    const hours = Array.from({ length: 24 }, (_, i) => (i === 23 ? 5 : i === 20 ? 2 : 0));
    answers["requests.get"] = ({ host }) => ({
      log: "C:/k/caddy/access.log",
      sites: [
        {
          host,
          count: 7,
          failures: 1,
          median_ms: 12.5,
          p95_ms: 240,
          since: 1794570330,
          slowest: [],
          hours,
          failing_hours: hours.map((_, i) => (i === 20 ? 1 : 0)),
          statuses: { "2xx": 5, "3xx": 0, "4xx": 1, "5xx": 1 },
        },
      ],
    });
    shell(<SitePage host="shop.test" go={() => {}} />);
    const glance = await screen.findByRole("region", { name: "At a glance" });
    // Three reads answer in any order: wait for each.
    await vi.waitFor(() => {
      expect(glance.textContent).toContain("feature/checkout");
      expect(glance.textContent).toContain("Requests today7");
      expect(glance.textContent).toContain("1 of 1 running");
    });
    expect(glance.textContent).toContain("5 × 2xx · 1 × 4xx · 1 × 5xx");
    expect(glance.textContent).toContain("Uses mysql-8.4 (mysql).");
    expect(screen.getByText("3 h ago: 2 requests, 1 failed")).toBeTruthy();
  });
});

describe("site groups (ADR 0074)", () => {
  it("shows a group's sites in a section of its own, above the rest", async () => {
    const site = (host: string, group: string | null) => ({
      host,
      url: `http://${host}`,
      driver: "laravel",
      framework: "laravel",
      document_root: `D:/Code/${host}/public`,
      secure: false,
      source: "link",
      parked_in: null,
      php: { version: "8.4.27", pinned: null, from_project: false, fell_back: false },
      in_hosts: true,
      project: `D:/Code/${host}`,
      framework_version: "12.38.1",
      group,
      node: { version: null, asked_by: null, asks_for: null, fell_back: false },
    });
    answers["sites.list"] = () => ({
      parked: [],
      sites: [site("admin.orbit.test", "Orbit Labs"), site("blog.orbit.test", "Orbit Labs"), site("shop.test", null)],
      proxies: [],
      skipped: [],
      warnings: [],
    });
    shell(<SitesPage go={() => {}} />);
    const group = await screen.findByRole("region", { name: "Orbit Labs" });
    expect(group.textContent).toContain("admin.orbit.test");
    expect(group.textContent).toContain("blog.orbit.test");
    expect(group.textContent).not.toContain("shop.test");
    expect(screen.getByRole("region", { name: "Linked" }).textContent).toContain("shop.test");
  });

  it("takes plain names only", () => {
    expect(groupName(" Acme Corp ")).toBe("Acme Corp");
    expect(groupName("orbit-labs_2.0")).toBe("orbit-labs_2.0");
    expect(groupName("")).toBeNull();
    expect(groupName("a/b")).toBeNull();
    expect(groupName("x".repeat(41))).toBeNull();
  });
});
