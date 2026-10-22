# 0017. `kiln doctor`: one read-only health check

- **Status:** accepted
- **Date:** 2026-10-22
- **Task:** P1-12 · Builds on ADRs [0010](0010-local-certificate-authority.md)–[0016](0016-per-site-php-and-shims.md); addresses [I-12](../issues.md#i-12), [I-45](../issues.md#i-45)

## Context

Kiln now has several moving parts: the state database and journal, the hosts block, the CA and
its trust, Caddy, the php-cgi supervisor and pools, PHP builds, pins, shims and `PATH`. When a
site doesn't load, the cause can be any of them. During Phase 1 the same questions kept coming
up:

- Who holds port 80? Herd's nginx does on the dev machine.
- Is the site in the hosts file?
- Is the CA trusted?
- Why does `curl` say "revocation status unknown"? That's [I-12](../issues.md#i-12).
- Is `kiln` running inside a packaged app's container ([I-45](../issues.md#i-45)), where
  AppData is redirected?

Every answer needed hand-run commands.

## Decision

1. **`kiln doctor` only reads.** It never changes anything. Each finding says what to run to
   fix it, usually a Kiln command.
2. A finding has an **area**, a **status** and a **message**, optionally with a **fix** (a
   command or short instruction):
   - status: `ok`, `info`, `warn` or `fail`;
   - areas: state, environment, ports, web, php, sites, dns, certificates, shims.

   Output is grouped by area. `--json` prints the same findings as JSON for scripts and the
   later GUI. The exit code is 1 if anything failed, else 0.
3. **Checks (Windows, Phase 1):**

   | Area | Checks |
   |---|---|
   | state | the database opens; schema version; operations stuck in `prepared`/`applying` (fix: `kiln undo <n>`) |
   | environment | **packaged-app container**: `kiln` runs under a process whose image is in `C:\Program Files\WindowsApps\` (fix: run from a normal terminal, I-45) |
   | ports | for the HTTP and (if any secure site) HTTPS ports, on 127.0.0.1 and ::1: free, held by Kiln's Caddy, or **held by another program, named with PID and path** (e.g. Herd's nginx; fix: `kiln web start --http-port …`) |
   | web | Caddy installed (version); running or not; when running, its admin socket answers |
   | php | builds complete on disk; the default version; pins that fall back; the supervisor running with every pool's workers listening; recent lines from `php-errors.log` |
   | sites | links and parked sites, skipped folders with reasons |
   | dns | each site in Kiln's hosts block (fix: `kiln hosts sync`), and resolving to loopback; hosts entries for sites that no longer exist |
   | certificates | (only with secure sites) root present, root trusted for this user, intermediate days left; **Schannel note**: `curl.exe` and Git on Windows fail revocation checks against any local CA, so use `--ssl-revoke-best-effort` (I-12); Firefox profiles that don't trust Windows roots |
   | shims | installed or not; when installed, whether `php` on this `PATH` resolves to Kiln's shim, and what comes first otherwise |

4. **Port owners** come from the TCP listener table (`GetExtendedTcpTable` with
   owning-PID listeners), with the image path from the verified process handle. **The parent
   process** (for the container check) comes from a ToolHelp snapshot. Both are `kiln-sys`
   wrappers (ADR 0008).
5. **New crate `kiln-doctor`.**
   - Each check is a pure function from observed facts to findings, unit-tested with made-up
     facts.
   - Gathering the facts (database, sockets, processes, files) is separate and runs only in
     the CLI.
6. Linux checks (systemd-resolved, NSS, PHP-FPM) come with P1-04. Coexistence *handling*
   (offering other ports, recognising XAMPP/Laragon) is P1-14. The doctor only reports what it
   sees.

## Consequences

- One command answers "why doesn't my site load?", and its JSON form is ready for the GUI.
- The I-12 and I-45 traps are explained where users will look.
- More `unsafe` in `kiln-sys` (two small wrappers), covered by tests.

## Alternatives considered

- **`netstat -ano` / `tasklist` parsing:** localized output and extra processes; the APIs are
  direct.
- **Fixing things automatically:** a health check that changes the system is surprising.
  Fixes stay explicit commands, most of them journaled and undoable.
