# 0050. The desktop app on Windows: a tray app over `kilnd`'s API

- **Status:** accepted (architecture); the visual design is chosen in P4-02
- **Date:** 2026-11-12
- **Task:** Phase 4 plan · Builds on [ADR 0002](0002-rust-core-and-tauri-gui.md), [0026](0026-kilnd-daemon-and-local-api.md), [0039](0039-windows-first-and-services-catalog.md), [0048](0048-kilnd-and-services.md)

## Context

Phase 4 is Kiln's desktop app. The user asked for it on Windows first, as everything else,
with macOS (Phase 4b) and Linux left for later. The app should keep working when its window
is closed: it lives in the notification area (the tray).

[ADR 0002](0002-rust-core-and-tauri-gui.md) already set the foundations:

- a Tauri 2 app, which uses Windows' own WebView2, at around 10 MB;
- it talks to `kilnd` only through the local API ([ADR 0026](0026-kilnd-daemon-and-local-api.md));
- it has no system access of its own.

`kilnd` already keeps the web server and services up and has a JSON-RPC API
([ADR 0048](0048-kilnd-and-services.md)). Its methods so far cover status, the web server,
the CA, and starting and stopping services. The app needs much more: everything the CLI can
show and change, with the same previews.

The CLI is the one implementation of every operation. The daemon already runs it for each
change, so changes keep the CLI's change lock, journal and checks.

## Decision

1. **Three parts, each with one job:**
   - **`kilnd`** keeps things running and answers the API. It is unchanged in role.
   - **`kiln`** (the CLI) makes every change. The daemon runs it, as now.
   - **The desktop app** shows state and asks for changes, through the API only.
2. **The app lives in the tray:**
   - **The tray icon** shows Kiln's state: running, something needs attention, or stopped.
   - **Its menu** has the sites to open, services to start and stop, "Open Kiln", and
     "Quit".
   - **Closing the window** hides it to the tray, and everything keeps running.
   - **"Quit"** closes the app only. `kilnd` keeps the web server and services up; stopping
     those is "Stop all" in the menu or `kiln web stop` / `kiln services down`.
   - **One instance:** starting it again shows the open window.
   - **At login:** the app starts and makes sure `kilnd` runs. This is one journaled,
     undoable login item that takes over from `kilnd`'s own (`kiln daemon enable`). Uninstall
     removes it like any other.
3. **The API grows to cover the app** (P4-01):
   - **Read methods** for sites, PHP and Node versions, services and the catalog, projects,
     history, doctor, settings and logs.
   - **Change methods:** each one runs the CLI with a new `--json` output mode, so the app
     gets structured results and errors, not text.
   - **Previews first:** a change that previews in the CLI previews in the app, and the
     operations' own previews are what the app shows.
   - **Progress:** long operations, such as downloads, send progress events over
     `events.subscribe`.
   - **Versioning:** `kiln.hello` reports API version 2; the app refuses an older daemon
     with a clear message.
4. **The frontend:** TypeScript and React with Vite, inside Tauri; styling with Tailwind; no
   UI library that hides the markup.
   - **Windows 11's look:** Mica background, Segoe UI Variable, light and dark following
     Windows.
   - **Keyboard and accessibility:** full keyboard use, and screen-reader labels.
   - **The visual design itself is chosen in P4-02,** from the user's design made with
     Claude or Kiln's own mockup.
5. **Where it lives:** `apps/desktop`, a workspace member. Its Rust side uses
   `kiln-daemon`'s client and holds no logic of its own.
6. **Tests on CI** (P4-13):
   - frontend tests against a recorded API;
   - API contract tests against a real `kilnd`;
   - end-to-end runs of the built app on the Windows runner, with WebDriver through
     `tauri-driver` and Edge's driver, next to the services scenario.

## Consequences

- One implementation of each operation stays in the CLI, so the app can't drift from it,
  and everything the app does shows in `kiln history`.
- The CLI gains a `--json` mode, which scripts can use too.
- The app adds Node and a frontend build to the toolchain for `apps/desktop` only. The rest
  of the workspace still builds with Rust alone.
- Installers and signing stay with Phase 5. Until then the app is built from source like
  the rest.

## Alternatives considered

- **The app calling the core crates directly.** Rejected by ADR 0002: a second way into
  every operation, with its own bugs.
- **Electron.** Rejected by ADR 0002: about 150 MB, and it duplicates logic.
- **WinUI 3 (C#).** It looks most native, but it adds a second language and toolchain,
  and would be thrown away for macOS and Linux later. Tauri's WebView2 with Windows 11's
  styling comes close enough.
- **The app replacing `kilnd`.** Rejected: services must keep running when the app is
  quit, and the daemon is also what scripts and future tools talk to.
