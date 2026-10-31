# Plan

Kiln serves PHP projects at `https://<name>.test` on Windows, Linux and macOS. Per-project PHP
versions, trusted local HTTPS and managed services. Every system change it makes can be
previewed and undone.

## Principles (non-negotiable)

1. **Reversible.** DNS, certificates, PATH and services go through `kiln-ops`: preview,
   journal, verify, undo. Uninstall restores the machine. ([ADR 0004](adr/0004-reversible-operation-engine.md))
2. **Unprivileged by default.** One tiny helper with a fixed list of operations does the few
   admin tasks. No generic "run as admin".
3. **Loopback only.** Nothing listens beyond `127.0.0.1` / `::1` unless the user opts in per site.
   No firewall rules by default.
4. **Coexistence.** Never stop, modify or delete other tools' processes, files or config
   (Herd, XAMPP, Laragon, system PHP/nginx/MySQL). Kiln ships its server binaries under
   Kiln-specific image names ([I-11](issues.md#i-11)).
5. **Clean room.** No code, text or assets from proprietary tools. Upstream open source is
   reused only under its license, with attribution.

## Phases

| Phase | Goal | Platforms | Estimate | Status |
|---|---|---|---|---|
| **0** | Foundations: repo, CI, operation engine, key decisions via spikes | all (CI) | 2–3 weeks | **done** (one optional recheck open) |
| **1** | MVP CLI: serve PHP sites over trusted HTTPS, per-site PHP, clean uninstall | Windows, Linux | 6–8 weeks | **done** 2026-10-27: exit criteria met in both VMs |
| **2** | Developer tooling: Node, Composer, Laravel installer, `kiln.toml`, daemon + local API | Windows, Linux | 4–5 weeks | **done** 2026-10-31: exit criteria met in both VMs |
| **3** | Services: MySQL, MariaDB, PostgreSQL, a Redis-compatible cache, Mailpit; `.env` wiring | Windows, Linux | 6–8 weeks | **planned** 2026-10-31 ([ADR 0035](adr/0035-services.md)) |
| 4 | Tauri GUI, log viewer, dev processes, Xdebug on demand, dump capture | Windows, Linux (macOS in 4b) | 6–8 weeks | later |
| 4b | macOS bring-up: resolver file, Keychain, launchd, privileged helper, PHP, services and the GUI on macOS ([ADR 0007](adr/0007-platform-delivery-order.md), amended) | macOS | 5–7 weeks | later; needs Apple hardware for testing |
| 5 | Installers, signed updates, MCP server, share tunnels, external security review → 1.0 | all | 4–6 weeks | later |
| 6 | Backlog: insights, SPX profiler, Windows wildcard DNS, LAN sharing, ARM64 Windows | — | — | later |

Staffing assumption: 2 core Rust engineers, a part-time UI engineer from Phase 4, and part-time
QA/DevOps for VM test infrastructure. That gives about 8–10 months to 1.0.

## Phase 0: foundations (done)

Gate: ADRs accepted ✓ · operation engine passes its property tests ✓ · CI green on 3 OSes ✓.
Details in [progress.md](progress.md#phase-0).

## Phase 1: MVP on Windows + Linux

**Exit criteria**

- On clean Windows 11 and Ubuntu 24.04 VMs, `kiln park ~/Code` serves 3 sample apps
  (Laravel, WordPress, static) at `https://*.test` with a trusted certificate, and per-site
  PHP versions work.
- `kiln uninstall` restores the hosts file, PATH and trust stores **byte-identical** to a
  snapshot taken before install. This is verified by scripted VM runs before each release
  ([ADR 0024](adr/0024-integration-test-vms.md): GitHub's hosted runners can't host these VMs).
- With Herd installed and running on the Windows VM, nothing Herd owns changes. Kiln reports
  port conflicts clearly, and it survives Herd restarting.

**Tasks** (IDs are stable; status is tracked in [progress.md](progress.md#phase-1))

| ID | Task | Depends on | Notes / findings to honour |
|---|---|---|---|
| P1-01 | State store (SQLite) and journal migration from JSONL | — | **done** 2026-10-21 (`kiln-state`) |
| P1-02 | `kiln-helper` (Windows): fixed operation list, named pipe with an ACL for the installing user, signed requests | P1-01 | **done** 2026-10-21 ([ADR 0009](adr/0009-windows-privileged-helper.md)) |
| P1-03 | `HostsBlock` DNS provider (Windows): `ReplaceFileW`, read-only refusal, sharing-violation retry, create-if-missing | P1-02 | **done** 2026-10-21 (`kiln_platform::hosts_op`, `kiln_helper::session`) |
| P1-04 | Linux, in four parts ([ADR 0020](adr/0020-linux-dns-and-privilege.md)): **a** hosts DNS + polkit helper + undo + Caddy on Linux; **b** CA ([ADR 0021](adr/0021-linux-ca-key-and-trust.md): Secret Service key, NSS, system store); **c** PHP ([ADR 0022](adr/0022-linux-php-runtimes-and-fpm.md): pinned static builds or system PHP, PHP-FPM pools); **d** `php`/`composer` shims ([ADR 0023](adr/0023-linux-shims-and-path.md)) | P1-01 | **done** 2026-10-23 (a 10-22; b–d 10-23), checked in the Ubuntu 24.04 VM |
| P1-05 | Local CA: `.test`-only (name-constrained), key protected by DPAPI/libsecret, user-store trust by default, Firefox/NSS | P1-02 | **done** 2026-10-21 on Windows ([ADR 0010](adr/0010-local-certificate-authority.md)); libsecret + NSS import with P1-04 |
| P1-06 | Caddy integration: typed JSON config, loopback bind, `skip_install_trust`, catch-all 404, Kiln-named binary; HTTP by default, HTTPS per site | P1-05 | **done** 2026-10-21 on Windows ([ADR 0011](adr/0011-caddy-integration.md)) |
| P1-07 | Caddy admin API off TCP (Unix socket on Windows, or random port plus origin enforcement) | P1-06 | **done** 2026-10-21: Unix socket, live reload ([ADR 0012](adr/0012-caddy-admin-channel.md)) |
| P1-08 | PHP runtime provider: windows.php.net NTS (SHA-256 verified); static-php-cli / distro on Linux | P1-01 | **done** 2026-10-21 on Windows ([ADR 0013](adr/0013-downloads-and-php-runtimes.md)); Linux with P1-04 |
| P1-09 | Process supervisor: php-cgi pools with **staggered** recycling, PHP-FPM on Linux, health checks | P1-08 | **done** 2026-10-22 on Windows ([ADR 0014](adr/0014-php-cgi-supervisor.md)); PHP-FPM with P1-04 |
| P1-10 | Sites: park / link / unlink, framework drivers (document-root detection) | P1-06 | **done** 2026-10-22 ([ADR 0015](adr/0015-parked-folders-and-drivers.md)); Valet considered, not used |
| P1-11 | Per-site PHP version (`kiln isolate`), shims for `php` / `composer` | P1-08, P1-10 | **done** 2026-10-22 on Windows ([ADR 0016](adr/0016-per-site-php-and-shims.md)) |
| P1-12 | `kiln doctor`: ports with owning process, DNS, trust, runtimes, journal consistency, Schannel revocation hint | P1-03..P1-09 | **done** 2026-10-22 ([ADR 0017](adr/0017-kiln-doctor.md)) |
| P1-13 | `kiln plan` / `kiln undo` / `kiln uninstall --dry-run` | P1-01 | **done** 2026-10-22 ([ADR 0018](adr/0018-history-undo-uninstall.md)) |
| P1-14 | Coexistence detection (Herd, XAMPP, Laragon, system nginx/PHP) | P1-12 | **done** 2026-10-22 ([ADR 0019](adr/0019-coexistence.md)) |
| P1-15 | Integration test VMs (Windows 11, Ubuntu 24.04): byte-identical uninstall, Herd coexistence ([ADR 0024](adr/0024-integration-test-vms.md)) | all | **done** 2026-10-26: both scenarios pass, with a real Laravel app, WordPress and a second PHP version (2026-10-27); lessons [I-34](issues.md#i-34)–[I-36](issues.md#i-36) |

**Suggested order:** P1-01 → P1-02 → P1-03 → P1-05 → P1-06/07 → P1-08/09 → P1-10/11 →
P1-13 → P1-12/14 → P1-15. Linux tasks (P1-04, the Linux half of P1-08) run in parallel.

## Phase 2: developer tooling (Windows + Linux)

**Exit criteria**

- **Project config.** A project with a `kiln.toml` pinning PHP 8.3 and Node 22 gets those
  versions from `php`, `node` and `npm` in a new terminal, and in its site. A project with a
  `herd.yml` is imported to the same result.
- **Daemon.** `kilnd` starts at login, answers its versioned local API, and keeps HTTPS
  working across an intermediate-certificate renewal without anything from the user. The
  web server restarts by itself for under a second ([I-57](issues.md#i-57)). This is tested
  with a renewal forced through the API.
- **PHP maintenance.** `kiln php update` followed by `kiln php rollback` round-trips. Turning
  an extension on and off works and is undoable.
- **New projects and proxies.** `kiln new` produces a Laravel app served over HTTPS. A proxy
  site serves a local dev server, such as Vite.
- **Uninstall.** `kiln uninstall` still leaves the machine as it was, including autostart,
  Node and the new shims. The VM scenarios on Windows (with Herd) and Ubuntu verify all of
  the above.

**Tasks** (status is tracked in [progress.md](progress.md#phase-2))

| ID | Task | Depends on | Notes |
|---|---|---|---|
| P2-01 | `kilnd`: per-user daemon and a versioned local API. JSON-RPC over a per-user Unix socket. The CLI keeps working with or without it | Phase 1 | [ADR 0026](adr/0026-kilnd-daemon-and-local-api.md). The API is what the Phase 4 GUI uses |
| P2-02 | Per-user autostart for `kilnd`: Windows `HKCU\…\Run` value, Linux systemd user unit. Journaled and undoable (`kiln daemon enable/disable`) | P2-01 | No admin rights; ADR 0026 §6 |
| P2-03 | Automatic renewal. `kilnd` re-issues the 30-day intermediate before it expires; a running server restarts to use it. `kiln daemon status` shows the days left | P2-01 | Today it is renewed only at `kiln web start` ([ADR 0011](adr/0011-caddy-integration.md) §7) |
| P2-04 | Project config: `kiln.toml` (PHP and Node versions, HTTPS, extra names), read by link, park, isolate and the shims; `kiln import herd` for `herd.yml` | — | **Done** 2026-10-28, [ADR 0027](adr/0027-project-config-kiln-toml.md): an isolate pin beats `kiln.toml`, which beats the default. For Node (P2-05), `kiln.toml` beats `.nvmrc` / `.node-version` |
| P2-05 | Node runtimes: `kiln node install/list/remove` from nodejs.org, checked against its signed SHASUMS. `node`, `npm`, `npx` and `corepack` shims follow `kiln.toml`, `.nvmrc` and `.node-version` | P2-04 | **Done** 2026-10-28, [ADR 0028](adr/0028-node-runtimes-and-shims.md): the signature is checked against Node's release keys. Same shim binary as PHP; the shims step aside while Kiln has no Node, and on Windows a machine-wide Node stays first |
| P2-06 | PHP patch updates with rollback: `kiln php update [minor]` installs the newest patch next to the current one and switches pools live; `kiln php rollback` goes back | — | **Done** 2026-10-29, [ADR 0029](adr/0029-php-patch-updates-and-rollback.md). Windows: latest from the release index. Linux: newest pinned build, so new patches arrive with a kiln-php release and a Kiln update ([ADR 0025](adr/0025-own-static-linux-php-builds.md)). Kiln also pins the previous patch of each line, so it can be installed again |
| P2-07 | PHP extension toggles: `kiln php ext list/on/off [--php X.Y]` for the extensions a build ships. Edits Kiln's `php.ini` section, journaled; restarts pools | — | **Done** 2026-10-29, [ADR 0030](adr/0030-php-extension-toggles.md): one line per toggle, undone byte for byte; `php -m` decides what is loaded. Kiln's static Linux builds can't toggle (compiled in); a system PHP can. Xdebug on demand stays in Phase 4 |
| P2-08 | `kiln new <name>`: a Laravel project made with Kiln's PHP and Composer, linked, optionally HTTPS, URL printed | P2-04 | **Done** 2026-10-30, [ADR 0031](adr/0031-kiln-new-laravel-projects.md): `composer create-project laravel/laravel` with Kiln's PHP and Composer; no extra installer binary or starter kits. `--php` is kept in the project's `kiln.toml` |
| P2-09 | Proxies: `kiln proxy <name> <port or URL>`. A `.test` site that Caddy reverse-proxies to a local server, over HTTP or HTTPS, with live reload | — | **Done** 2026-10-30, [ADR 0032](adr/0032-proxy-sites.md): Caddy sends the target's own `Host` (Vite 6 checks it) and `X-Forwarded-Host`; WebSockets pass through. Targets are loopback only (plan principle 3) |
| P2-10 | Configurable TLD at `kiln ca init`: `test` (default) or another reserved, non-public name | — | **Done** 2026-10-30, [ADR 0033](adr/0033-configurable-tld.md), amends [ADR 0010](adr/0010-local-certificate-authority.md): `test`, `internal`, `example` or `invalid`, recorded once and used everywhere; another TLD later needs a new root, so `ca init` refuses it |
| P2-11 | VM scenarios for the Phase 2 exit criteria, on Windows (with Herd) and Ubuntu | all | **Done** 2026-10-31, [ADR 0034](adr/0034-phase-2-vm-scenarios.md), extends [ADR 0024](adr/0024-integration-test-vms.md): a real reboot inside each scenario after `kiln daemon enable`, and the renewal restart timed (under a second; fixed [I-73](issues.md#i-73)). Phase 2's exit criteria are met in both VMs |

**Suggested order:** P2-01 → P2-02 → P2-03 (the daemon and what depends on it), then P2-04 →
P2-05 → P2-08 (project config, Node, `kiln new`), then P2-06, P2-07, P2-09 and P2-10 in any
order, and P2-11 last.

**Carried over from Phase 1** (not blocking):

- [I-41](issues.md#i-41): watch the elevation once, with someone at the screen.
- Remove the five leftover test entries from the Ubuntu VM's keyring at its desktop
  ([I-59](issues.md#i-59)).
- [I-39](issues.md#i-39): the optional Edge DoH recheck.

## Phase 3: services (Windows + Linux)

Design: [ADR 0035](adr/0035-services.md). macOS moved to Phase 4b
([ADR 0007](adr/0007-platform-delivery-order.md), amended).

**Exit criteria**

- **Databases.** MySQL, MariaDB and PostgreSQL run as Kiln instances, two versions of one
  kind side by side. A new Laravel app migrates and serves pages against each.
- **Cache and mail.** On Linux, Laravel's cache, sessions and queue work against Valkey. On
  Windows they work against the cache the spike chose, or Kiln says plainly that Windows has
  no cache service yet. Mail sent by the app is caught by Mailpit, whose UI is at
  `https://mail.test`.
- **Next to other tools.** Herd (with its services where available), XAMPP or a system
  MySQL/PostgreSQL keep their ports, processes and data. Kiln takes the next free port and
  says why.
- **Wiring.** `kiln env use` changes only the needed `.env` keys, after a preview, and
  `kiln undo` restores `.env` byte for byte. A project with `[services]` in `kiln.toml`
  (or a `herd.yml` with services) is brought up by `kiln services up`.
- **Lifecycle.** Services survive `kiln web stop`. With `kilnd`, they come back after a
  crash and after a reboot.
- **Data.** No command deletes a database without `--delete-data`. `kiln db dump` and
  `restore` round-trip a database.
- **Uninstall.** With `--delete-data`, the machine is as it was. With `--keep-data`, the
  only thing left is the named data folder. The VM scenarios on Windows (with Herd) and
  Ubuntu verify all of the above.

**Tasks** (status is tracked in [progress.md](progress.md#phase-3))

| ID | Task | Depends on | Notes |
|---|---|---|---|
| P3-01 | Service framework: instances in the state database, the `ServiceProvider` trait and its contract tests, `kiln service add/list/start/stop/restart/logs/remove`, `kiln services up/down`, a second `kiln-supervisor` for services | — | ADR 0035 §1–3; the supervisor generalized from php-cgi pools to programs |
| P3-02 | Spike: a Redis-compatible cache on Windows. Garnet against Laravel's cache (locks, tags), sessions, queue (Lua) and rate limiter | — | ADR 0035 §8; decides whether Windows gets a cache service in Phase 3 |
| P3-03 | Port choice and coexistence for services: usual port, else the next free one; `kiln doctor` names other tools' services | P3-01 | ADR 0035 §5, extends [ADR 0019](adr/0019-coexistence.md) |
| P3-04 | MySQL and MariaDB providers: vendor downloads (pinned digests, license shown), data folder set-up, graceful stop, `kiln db` client | P3-01 | Linux: libraries such as `libaio.so.1` found without system changes |
| P3-05 | PostgreSQL provider: EDB archives on Windows; Kiln's own relocatable Linux builds from the PHP build pipeline | P3-01 | ADR 0035 §8, [ADR 0025](adr/0025-own-static-linux-php-builds.md); `trust` on loopback only |
| P3-06 | Cache provider: Valkey on Linux; on Windows what P3-02 chose | P3-01, P3-02 | |
| P3-07 | Mailpit provider, with its UI served as `https://mail.test` through a proxy site | P3-01 | [ADR 0032](adr/0032-proxy-sites.md) |
| P3-08 | `.env` wiring: `kiln env use <instance>`, a journaled key-level operation with preview and byte-exact undo; creates the project's database; `kiln new --db` | P3-04 | ADR 0035 §9 |
| P3-09 | Project config: `[services]` in `kiln.toml`, `kiln services up` in a project, `herd.yml` services in `kiln import herd` | P3-08 | Finishes [ADR 0027](adr/0027-project-config-kiln-toml.md) |
| P3-10 | `kilnd` and services: the watchdog restarts the services supervisor after a crash or reboot; `service.*` API methods | P3-01 | [ADR 0026](adr/0026-kilnd-daemon-and-local-api.md) |
| P3-11 | Data safety: `kiln db dump/restore`; `service remove` and `uninstall` keep data unless `--delete-data`; `--keep-data` moves it out | P3-04 | ADR 0035 §7 |
| P3-12 | VM scenarios for the Phase 3 exit criteria, on Windows (with Herd) and Ubuntu, including a system MySQL or PostgreSQL to coexist with | all | Extends [ADR 0024](adr/0024-integration-test-vms.md); adding that service to the test VMs' `clean` checkpoints needs approval first |

**Suggested order:** P3-01 and the P3-02 spike first, then P3-03, then the providers P3-04
→ P3-05 → P3-06 → P3-07, then P3-08 → P3-09, P3-10 and P3-11, and P3-12 last.

## Phase 3–5 scope (summary)

- **Phase 3:** see above.
- **Phase 4:** Tauri tray app (sites, PHP/Node, services, onboarding that previews every system
  change, journal viewer with undo); log viewer; dev processes; Xdebug cookie routing; dump
  capture via an open-source collector.
- **Phase 4b:** macOS adapters (resolver file plus stub, Keychain, launchd, a privileged
  helper through `SMAppService`); PHP and services builds for macOS; the GUI on macOS. Needs
  Apple hardware to test on. Builds stay unsigned until Phase 5.
- **Phase 5:** MSIX/winget, Homebrew cask, deb/rpm/AppImage; Developer ID signing and
  notarization on macOS (needs an Apple Developer Program account); signed update
  manifests; `kiln mcp`; share-tunnel plug-ins; workspaces; external security review; SBOM.

## Out of scope

Agent studio, dictation and AI widgets; Forge/Vapor/Cloud integrations; site preview
screenshots; Docker orchestration.
