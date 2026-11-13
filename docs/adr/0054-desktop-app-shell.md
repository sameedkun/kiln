# 0054. The desktop app's shell: Tauri project, bridge to `kilnd`, tray, login item

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-03 · Implements [ADR 0050](0050-desktop-app.md) §2, §5 (amended here) · Builds on [ADR 0051](0051-desktop-visual-design.md) §6–7, [0053](0053-api-v2-and-json-output.md)

## Context

P4-03 puts the screens from P4-15 inside a real window, with the tray and its popup, over
`kilnd`'s API version 2. Some constraints decide the structure:

- **CI:** it runs `cargo clippy --workspace` and `cargo test --workspace` on Ubuntu, and on
  macOS nightly. On Linux, Tauri needs WebKitGTK system libraries, which those runners
  don't have, and the app is Windows-only for now (ADR 0050).
- **Uninstall:** it must leave the machine as it was. WebView2 keeps a profile folder,
  which by default goes to `%LOCALAPPDATA%\<app identifier>`, outside Kiln's folders.
- **The client:** `kiln_daemon::Client` handles one call at a time on its connection. It
  waits up to 60 s for an answer, and skips notifications while it waits.
- **Tauri 2.12:**
  - commands that aren't `async` run on the main thread, so blocking socket I/O must not
    happen in them;
  - window controls and dragging need permissions in a capability file;
  - a CSP applies to the page.
- **Login item:** `kiln daemon enable` writes `kilnd` into the `Kiln` value of the Run key.
  ADR 0050 asks for one login item that starts the app, and the app makes sure `kilnd`
  runs.

## Decision

1. **The app's Rust side is its own Cargo project** (amends ADR 0050 §5).
   - **Where:** `apps/desktop/src-tauri`, with its own committed `Cargo.lock`. It is
     excluded from the workspace, so Linux and macOS CI never build it.
   - **Shared folder:** it builds into the workspace's `target` folder
     (`.cargo/config.toml`), so `kiln-desktop.exe` lands next to `kiln.exe` and `kilnd.exe`.
   - **Kiln code:** it uses `kiln-daemon` by path, for the client and the socket path. It
     holds no logic of its own.
   - **The Windows `desktop` CI job keeps the workspace's guards for it:**
     - `cargo fmt --check`;
     - pedantic clippy with `-D warnings`;
     - `unsafe_code = "forbid"`;
     - its tests;
     - `cargo deny` on its lockfile;
     - building the app.
   - **Tauri 2's stable line** (2.12) and its plugins: single instance, opener.
2. **Everything the app keeps is in Kiln's folders,** so uninstall removes it:
   - **WebView2's profile:** `%LOCALAPPDATA%\Kiln\app\webview`, set when each window is
     created.
   - **The app's settings:** `%APPDATA%\Kiln\app.json`, holding:
     - "when Kiln starts" (window or tray);
     - whether the close-to-tray notice was shown;
     - the window's size and position.
   - **Uninstall while the app runs:** uninstall stops `kiln-desktop.exe` first, as it stops
     `kilnd`.
3. **The bridge:**
   - **One `api` command** forwards a method and its parameters to `kilnd`.
     - It is `async` and does the socket work on a blocking thread, never the main one.
     - Each call opens its own connection, so a slow read never queues the others.
   - **Events:**
     - A thread holds one subscribed connection and re-emits each notification to the
       pages (`kiln-event`).
     - A read timeout means "no event yet"; anything else means reconnect.
     - After a reconnect the pages are told (`kiln-reconnected`) and read everything again.
   - **The daemon's state** is one value the pages read (`daemon_state`):
     - `starting`;
     - `running` (API version 2 or later);
     - `too-old`: refused, with a message to update Kiln;
     - `unreachable`.
   - **Starting `kilnd`:** if it isn't running when the app starts, the app runs
     `kiln daemon start` without a window, then connects.
   - **Opening a site:** the page sends a host name. Rust checks it against `sites.list` and
     opens that site's URL in the browser. The page never opens arbitrary URLs.
   - **Appearance:** the page reports what WebView2 sees through `matchMedia` (dark mode,
     forced colours, reduced transparency). The Rust side reads the Windows registry only
     for a setting WebView2 doesn't report. That is checked by hand first (§7).
4. **Windows and the tray** (ADR 0051 §6–7):
   - **Main window:**
     - frameless, with Kiln's title bar and caption buttons;
     - minimum size 880 × 560;
     - closing hides it.
   - **Tray icon:**
     - **left click:** shows and focuses the main window;
     - **right click:** shows the popup;
     - **tooltip:** says the state.
   - **Popup:** a second frameless window, 340 px wide, hidden from the taskbar and on top.
     - **Placement:** next to the tray icon, inside the work area of the icon's monitor, in
       physical pixels. This is a pure function with unit tests: taskbar on any edge, scaled
       and secondary monitors.
     - **Closing:** it hides when it loses focus or on Esc. A tray click within 300 ms of it
       hiding doesn't reopen it, so clicking the icon again closes it.
   - **Native menu:** if the popup window can't be created, the tray gets a native menu with
     the same items.
   - **Single instance:** starting the app again shows the open window.
   - **Starting:**
     - with `--background` (the login item), or when "when Kiln starts" is the tray, the
       app stays in the tray;
     - otherwise it opens the window.
   - **Quit** exits the app only: `kilnd`, the web server and services keep running.
   - **The close-to-tray notice:** the first close doesn't hide the window yet. It shows a
     short dialog in it, saying Kiln keeps running in the tray, and the window hides when the
     dialog is dismissed. It appears once. It isn't a Windows notification, which an
     unpackaged app can't show reliably.
5. **The pages in the real app:**
   - **Home and the tray popup:** they read `sites.list` and `services.list`, start and stop
     services through `jobs.start`, and refresh on `state.changed`.
   - **Pages not connected yet** (P4-04 to P4-08): they show an honest empty state naming
     the task that fills them. The app never shows sample data.
   - **The design gallery stays its own page** (`gallery.html`), so its browser checks keep
     working.
6. **One login item** (`kiln app enable|disable`):
   - **It is the same journaled `Kiln` Run value.** The value becomes
     `"<folder>\kiln-desktop.exe" --background`.
   - **The CLI finds the app** next to itself; no path comes from a caller.
   - **Preview and confirmation** like `daemon enable`. It is on the API's allow-list as a
     command that asks first.
   - **Turning it off** undoes Kiln's login-item changes, newest first, as `daemon disable`
     does. Nothing then starts at sign-in.
   - **`kiln daemon status`** says which program the login item starts: the app, `kilnd`,
     or nothing.
7. **Checked by hand on a developer machine before P4-03 is done** (`tests/manual/app-check.md`):
   - **Clicks:** left and right click on the tray icon; clicking it again closes the popup.
   - **Closing:** close to tray with its notice once; a second launch; Quit leaving
     services up.
   - **Sign-in:** starting at sign-in in the tray.
   - **Window and look:** Snap Layouts on the maximise button (ADR 0051 §3); transparency
     off and dark mode following Windows.
   - **Prompts from a job:** the UAC and certificate prompts from a job
     ([ADR 0053](0053-api-v2-and-json-output.md) §5).

   CI builds the app and runs its unit tests. The end-to-end runs through WebDriver are
   P4-13.

## Consequences

- **CI:** Linux and macOS jobs stay as they are. The Windows `desktop` job grows by a Rust
  build of the app.
- **Two lockfiles:** the workspace's and the app's, each checked by `cargo deny`.
- **Uninstall:** it has one more program to stop and one more folder that sits inside
  Kiln's.
- **Not connected yet:** until P4-04 to P4-08 connect their pages, the app is the shell,
  Home and the tray. The gallery shows the rest.

## Alternatives considered

- **A workspace member behind a feature or a target filter.** `--workspace` still resolves
  and builds it on Linux. Excluding it is simpler and honest about the app being
  Windows-only.
- **One shared, locked connection for every call.** A 2-minute read would hold up a click
  on Stop.
- **Windows notifications for "still running in the tray".** An unpackaged app has no
  registered identity for toasts. The in-window notice needs nothing.
- **Sample data in unconnected pages.** It would look as if Kiln knew things it doesn't.
