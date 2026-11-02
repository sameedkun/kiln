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
| **3** | Services: MySQL, MariaDB, PostgreSQL, Redis, MongoDB, Meilisearch, RustFS, Mailpit, phpMyAdmin/Adminer, Reverb; `.env` wiring | Windows first (Linux on hold, [ADR 0039](adr/0039-windows-first-and-services-catalog.md)) | 6–8 weeks | **in progress** (4 of 16 tasks done) |
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

## Phase 3: services (Windows first)

Design: [ADR 0035](adr/0035-services.md). Re-planned 2026-11-02 by
[ADR 0039](adr/0039-windows-first-and-services-catalog.md):
- **Windows first.** Linux is on hold; it keeps building and passing its unit tests, and its
  services will run as rootless Podman containers when it resumes.
- **Herd Pro's services** on Windows.
- **Integration tests** on GitHub Actions' Windows runners.

macOS is Phase 4b ([ADR 0007](adr/0007-platform-delivery-order.md), amended).

**Exit criteria** (Windows)

- **Databases.** MySQL, MariaDB and PostgreSQL run as Kiln instances, two versions of one
  kind side by side. A new Laravel app migrates and serves pages against each.
- **The rest of the catalog:**
  - **Redis:** Laravel's cache, sessions and queue work against it.
  - **Meilisearch, RustFS and MongoDB** run and answer their clients.
  - **phpMyAdmin and Adminer** open on Kiln's databases.
  - **Mail** sent by the app is caught by Mailpit, whose UI is at `https://mail.test`.
  - **Laravel Reverb** runs for a project.
- **Next to other tools.** Herd, XAMPP or a Windows MySQL/PostgreSQL service keep their
  ports, processes and data. Kiln takes the next free port and says why.
- **Wiring.** `kiln env use` changes only the needed `.env` keys, after a preview, and
  `kiln undo` restores `.env` byte for byte. A project with `[services]` in `kiln.toml` (or
  a `herd.yml` with services) is brought up by `kiln services up`.
- **Lifecycle.** Services survive `kiln web stop`. With `kilnd`, they come back after a
  crash and after a reboot.
- **Data.** No command deletes a database without `--delete-data`. `kiln db dump` and
  `restore` round-trip a database.
- **Uninstall.** With `--delete-data`, the machine is as it was. With `--keep-data`, the
  only thing left is the named data folder.
- **Verification:** all of the above is checked by the Windows scenario on CI. Herd
  coexistence is checked by hand in the VM before a release.

**Tasks** (status is tracked in [progress.md](progress.md#phase-3))

| ID | Task | Depends on | Notes |
|---|---|---|---|
| P3-01 | Service framework: instances in the state database, the `ServiceProvider` trait and its contract tests, `kiln service add/list/start/stop/restart/logs/remove`, `kiln services up/down`, a second `kiln-supervisor` for services; Mailpit as the first provider | — | **Done** 2026-10-31, [ADR 0036](adr/0036-service-framework.md): the supervisor runs programs too, stopped gracefully; Mailpit runs as `kiln-mailpit`; uninstall refuses while services hold data, unless `--delete-data` |
| P3-02 | Spike: a Redis-compatible cache on Windows. Garnet against Laravel's cache (locks, tags), sessions, queue (Lua) and rate limiter | — | **Done** 2026-11-01, [spike 03](spikes/results/03-windows-cache.md): Garnet passes 19 of 22. Its Lua scripts ignore `SELECT` ([microsoft/garnet#2258](https://github.com/microsoft/garnet/issues/2258)), so Windows gets Garnet once a release fixes that; until then, no Windows cache |
| P3-03 | Port choice and coexistence for services: usual port, else the next free one; `kiln doctor` names other tools' services | P3-01 | **Done** 2026-11-01, [ADR 0037](adr/0037-service-ports-and-coexistence.md), extends [ADR 0019](adr/0019-coexistence.md): a busy port names its holder; a start refuses a port another program took, and `kiln service port` moves the instance; doctor has a Services area and lists other services |
| P3-04 | MySQL and MariaDB providers: vendor downloads (pinned digests, license shown), data folder set-up, graceful stop, `kiln db` client | P3-01 | **Done** 2026-11-02, [ADR 0038](adr/0038-mysql-and-mariadb.md): MySQL 8.4/9.7 and MariaDB 11.8/12.3 from their vendors; loopback-only config; root for 127.0.0.1 and ::1 at first start; Linux libaio linked from Ubuntu's renamed copy, else the package is named. Downloads now resume (I-74) |
| P3-13 | The Windows scenario on GitHub Actions (`windows-latest`) on every push to `main`: the services checks, ports, data and uninstall with the before/after comparison. No Herd, no desktop | — | **Done** 2026-11-02, [ADR 0039](adr/0039-windows-first-and-services-catalog.md) §3: `.github/workflows/scenario.yml` runs `tests/ci/windows-services.ps1` on every push to `main`; 17 checks plus the before/after comparison, about 5 minutes |
| P3-05 | PostgreSQL provider on Windows: EDB's binaries, `initdb`, `trust` on loopback only, `pg_ctl` stop, `psql` for `kiln db`; two versions side by side | P3-01 | ADR 0035 §8; Linux PostgreSQL waits for the container design |
| P3-06 | Redis on Windows: choose between `zkteco-home/redis-windows` (downloaded at the user's request, its terms shown) and Valkey built by Kiln; Laravel's cache, sessions and queue checked as in spike 03 | P3-01 | [ADR 0039](adr/0039-windows-first-and-services-catalog.md) §2; Garnet stays a fallback ([I-75](issues.md#i-75)) |
| P3-14 | Meilisearch, RustFS and MongoDB: official Windows releases, one provider each, one ADR | P3-01 | MongoDB is SSPL: from its vendor at the user's request (ADR 0003) |
| P3-15 | phpMyAdmin and Adminer: pinned releases served as Kiln sites (`pma.test`, `adminer.test`), pointed at Kiln's instances | P3-04 | They're PHP apps; no new process |
| P3-07 | Mailpit's UI served as `https://mail.test` through a proxy site, and the `MAIL_*` wiring | P3-01, P3-08 | [ADR 0032](adr/0032-proxy-sites.md) |
| P3-08 | `.env` wiring: `kiln env use <instance>`, a journaled key-level operation with preview and byte-exact undo; creates the project's database; `kiln new --db` | P3-04 | ADR 0035 §9 |
| P3-09 | Project config: `[services]` in `kiln.toml`, `kiln services up` in a project, `herd.yml` services in `kiln import herd` | P3-08 | Finishes [ADR 0027](adr/0027-project-config-kiln-toml.md) |
| P3-16 | Laravel Reverb for a project: `php artisan reverb:start` run and kept up by Kiln, `REVERB_*` wiring | P3-08 | A per-project process, not a server binary |
| P3-10 | `kilnd` and services: the watchdog restarts the services supervisor after a crash or reboot; `service.*` API methods | P3-01 | [ADR 0026](adr/0026-kilnd-daemon-and-local-api.md) |
| P3-11 | Data safety: `kiln db dump/restore`; `--keep-data` at uninstall moves data out | P3-04 | ADR 0035 §7 |
| P3-12 | The Phase 3 exit run: the CI scenario covering every criterion, plus a by-hand Herd coexistence check in the Windows VM | all | Extends [ADR 0024](adr/0024-integration-test-vms.md) |

**Order:** the table's: P3-13 first (tests on CI), then PostgreSQL, Redis, the other
services, the admin UIs, the wiring tasks, Reverb, `kilnd`, data safety, and P3-12 last.

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
