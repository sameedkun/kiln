# 0026. `kilnd`: an optional per-user daemon with a versioned local API

- **Status:** accepted
- **Date:** 2026-10-27
- **Task:** P2-01, P2-02, P2-03 · Builds on [ADR 0002](0002-rust-core-and-tauri-gui.md), [0011](0011-caddy-integration.md), [0012](0012-caddy-admin-channel.md), [0014](0014-php-cgi-supervisor.md), [0018](0018-history-undo-uninstall.md)

## Context

[ADR 0002](0002-rust-core-and-tauri-gui.md) names three programs: `kiln`, `kiln-helper` and
a long-running per-user daemon, `kilnd`. The Phase 4 GUI talks to the daemon only.

Phase 1 shipped without the daemon. The CLI starts Caddy and `kiln-supervisor` as detached
background processes, which then run on their own. That works, but three things need
something that keeps running:

- **Certificate renewal.** Caddy's intermediate certificate is valid for 30 days and is
  renewed only at `kiln web start` ([ADR 0011](0011-caddy-integration.md) §7). A server left
  running for a month would end up with an expired intermediate, and HTTPS would break.
- **Starting at login** (Phase 2 exit criterion). Today the user has to run `kiln web start`
  after each reboot.
- **A local API** for the GUI (Phase 4) and later the MCP server (Phase 5). It must not
  depend on parsing CLI output.

Constraints:

- **Unprivileged** (plan principle 2) and **loopback-free:** no TCP port at all (principle 3).
- **Reversible:** autostart is a system change, so it goes through the journal and
  `kiln uninstall` (principle 1, [ADR 0018](0018-history-undo-uninstall.md)).
- **No async runtime yet:** the workspace uses blocking `std` I/O throughout.

## Decision

1. **`kilnd` is a separate per-user binary and is optional.**
   - Every CLI command keeps working without it, as in Phase 1.
   - The CLI stays the authority for changes and never needs the daemon. To change
     something, such as restarting the web stack, the daemon runs the `kiln` program next to
     it (`kiln web start`) and logs what it printed. There is one implementation of each
     operation, not a CLI path and a daemon path.
   - Read-only answers (`web.status`, …) come from the library crates in the daemon's own
     process.
   - **One change at a time:** every `kiln` command that can change state takes a per-user
     lock file, `<state>/kiln.lock` (OS file locks via `File::lock`).
     - A second command waits and says so, whether it comes from the user or the daemon.
     - Read-only commands don't take the lock.
     - Neither does `kiln uninstall`: it stops the daemon first and then deletes the
       folder the lock file is in.
2. **What it does:**
   - **Keeps the web stack running.** Every 15 s it reads Caddy's and `kiln-supervisor`'s
     PID files, with the image checks of [ADR 0011](0011-caddy-integration.md) §12.
     - A PID file whose process has gone means the program exited without `kiln web stop`,
       such as after a crash or a reboot, because a clean stop removes the PID file.
     - Only then does it repair: `kiln web start` if Caddy exited, `kiln web restart` if
       only the supervisor did. A server the user stopped stays stopped.
     - Repairs back off (0, 30, 60, 120, 300 and then 600 s apart), and the delays reset
       after 10 healthy minutes.
   - **Runs timers:** intermediate renewal (P2-03; daily check, renew when under 10 days
     left, then a live Caddy reload). Later: PHP update checks.
   - **Serves the local API** (§3).
   - It never elevates. Anything that needs the helper, such as `hosts sync`, is reported
     to the user through the API, not done silently.
3. **Local API: JSON-RPC 2.0 over a per-user Unix socket,** one JSON message per line.
   - **Socket:** `<local data>/kilnd.sock` (`%LOCALAPPDATA%\Kiln\kilnd.sock` on Windows).
     Access control is the file system, as for Caddy's admin socket
     ([ADR 0012](0012-caddy-admin-channel.md) §2): the user's profile ACL on Windows, a
     `0700` directory on Unix. No TCP fallback.
   - **Windows needs an `AF_UNIX` listener.** `kiln-sys` already connects to `AF_UNIX`
     sockets; it gains a listener (bind, listen, accept), with `unsafe` confined there
     ([ADR 0008](0008-unsafe-confined-to-kiln-sys.md)).
   - **Versioning:** the first call is `kiln.hello`. It returns the API version (`1`), the
     daemon's version and its capabilities. Methods are namespaced (`sites.list`,
     `web.status`, `ca.renew`, …).
     - A breaking change adds `v2` methods next to `v1` for at least one release.
     - Errors use JSON-RPC error objects, with Kiln's message as `message`.
   - **Events:** a client may call `events.subscribe` and keep the connection open. The
     daemon then sends JSON-RPC notifications, such as `web.state_changed` or
     `ca.renewed`. The GUI's live views use this.
   - **Same style as `kiln-helper`:** newline-delimited JSON, already used by its stdio
     protocol. Easy to drive from a test, or with `kiln daemon call <method> [json]`.
4. **Threads, not async:** a blocking accept loop with one thread per connection (a handful
   of clients at most), plus one timer thread. No tokio or hyper dependency. This can be
   revisited if the GUI needs more.
5. **Single instance and lifecycle:**
   - The daemon holds `<state>/kilnd.lock` while it runs. A second `kilnd` exits with a
     message.
   - **`kiln daemon start | stop | status | call`.** `stop` uses the API's `daemon.shutdown`,
     with the verified terminate as a fallback.
   - **What stopping the daemon leaves running:** Caddy and the PHP pools keep running, as
     in Phase 1. `kiln web stop` stops them.
   - **Logs:** `<local data>/logs/kilnd.log`, rotated at 5 MB, keeping 3 files.
6. **Autostart (P2-02) is a journaled operation,** off until the user turns it on with
   `kiln daemon enable`:
   - **Windows:** the value `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Kiln` set to
     `"<path>\kilnd.exe"`. No admin rights, and it shows in Task Manager's startup apps,
     where users expect it.
   - **Linux:** an XDG autostart entry, `~/.config/autostart/kiln-kilnd.desktop`, which the
     desktop session starts after login. *Amended during P2-02:* this replaces the planned
     systemd user unit, for two reasons.
     - **Keyring:** certificate renewal (P2-03) needs the desktop keyring, which is unlocked
       only in the logged-in session. A systemd user service can start before that.
     - **Clean undo:** one file whose undo is exact. Enabling a unit also creates `*.wants`
       directories that `disable` leaves behind.
     - The daemon's own watchdog already does what `Restart=` would.
   - **No special startup mode is needed.** After a reboot, the web server's PID files are
     left over from processes the OS ended, so the watchdog's first pass runs
     `kiln web start`. A server the user stopped with `kiln web stop` has no PID file and
     stays stopped.
   - `kiln daemon disable`, `kiln undo` and `kiln uninstall` remove exactly what was added,
     byte for byte, like every other operation.
     - Undo restores an entry that Kiln replaced, and removes the `autostart` folder if
       Kiln created it and it is empty.
     - An entry edited by hand since is left alone unless `--force` is given.

## Consequences

- **Renewal:** HTTPS keeps working on machines that stay up for weeks, provided `kilnd`
  runs. Without it, renewal still happens at `kiln web start`, and `kiln doctor` warns
  10 days ahead.
- **One more binary:** `kilnd` must be built, shipped and uninstalled. Uninstall stops it
  before removing Kiln's folders.
- **GUI and MCP:** both get a stable, versioned API, and the GUI never reimplements logic.
- **Small attack surface:**
  - Only the user (and SYSTEM and Administrators on Windows) can reach the socket.
  - The daemon runs as the user and can do nothing the user's own `kiln` couldn't.
  - It holds no secrets beyond what the CLI already reads. The CA root key is unlocked
    only for a renewal, as today.

## Alternatives considered

- **HTTP over the socket (as Caddy's admin API):** easy to poke with `curl --unix-socket`,
  but Kiln would need an HTTP server: a new dependency, or hand-rolled parsing on the
  server side. JSON-RPC over lines needs neither, and gives notifications for events.
- **Loopback TCP with a token:** reachable by any local process that guesses or steals the
  token, and it breaks plan principle 3.
- **Windows named pipe:** native, but a second transport to maintain beside the Unix socket.
  `AF_UNIX` works on Windows 10 1803 and later, which Kiln already requires.
- **Make the CLI a client of the daemon:** that makes the daemon mandatory, puts every
  change behind an extra process, and breaks Phase 1's working model. Rejected.
- **Windows Task Scheduler for autostart:** more control, such as a delay or a restart
  policy. But the task would be created through `schtasks` or COM, is harder to undo byte
  for byte, and is hidden from the startup-apps view users know.
