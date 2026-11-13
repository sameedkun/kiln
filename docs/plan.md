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
  - **Mail** sent by the app is caught by Mailpit, whose UI is at `https://mailpit.test`.
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
- **Verification:** all of the above is checked by the Windows scenario on CI, which runs
  once ending with `--delete-data` and once with `--keep-data`. Herd coexistence is checked
  by hand before a release on a developer machine running Herd
  (`tests/manual/herd-check.ps1`).
- **Met 2026-11-12:** both CI runs pass (89 checks each), and the Herd check passed next to
  a running Herd (P3-12).

**Tasks** (status is tracked in [progress.md](progress.md#phase-3))

| ID | Task | Depends on | Notes |
|---|---|---|---|
| P3-01 | Service framework: instances in the state database, the `ServiceProvider` trait and its contract tests, `kiln service add/list/start/stop/restart/logs/remove`, `kiln services up/down`, a second `kiln-supervisor` for services; Mailpit as the first provider | — | **Done** 2026-10-31, [ADR 0036](adr/0036-service-framework.md): the supervisor runs programs too, stopped gracefully; Mailpit runs as `kiln-mailpit`; uninstall refuses while services hold data, unless `--delete-data` |
| P3-02 | Spike: a Redis-compatible cache on Windows. Garnet against Laravel's cache (locks, tags), sessions, queue (Lua) and rate limiter | — | **Done** 2026-11-01, [spike 03](spikes/results/03-windows-cache.md): Garnet passes 19 of 22. Its Lua scripts ignore `SELECT` ([microsoft/garnet#2258](https://github.com/microsoft/garnet/issues/2258)), so Windows gets Garnet once a release fixes that; until then, no Windows cache |
| P3-03 | Port choice and coexistence for services: usual port, else the next free one; `kiln doctor` names other tools' services | P3-01 | **Done** 2026-11-01, [ADR 0037](adr/0037-service-ports-and-coexistence.md), extends [ADR 0019](adr/0019-coexistence.md): a busy port names its holder; a start refuses a port another program took, and `kiln service port` moves the instance; doctor has a Services area and lists other services |
| P3-04 | MySQL and MariaDB providers: vendor downloads (pinned digests, license shown), data folder set-up, graceful stop, `kiln db` client | P3-01 | **Done** 2026-11-02, [ADR 0038](adr/0038-mysql-and-mariadb.md): MySQL 8.4/9.7 and MariaDB 11.8/12.3 from their vendors; loopback-only config; root for 127.0.0.1 and ::1 at first start; Linux libaio linked from Ubuntu's renamed copy, else the package is named. Downloads now resume (I-74) |
| P3-13 | The Windows scenario on GitHub Actions (`windows-latest`) on every push to `main`: the services checks, ports, data and uninstall with the before/after comparison. No Herd, no desktop | — | **Done** 2026-11-02, [ADR 0039](adr/0039-windows-first-and-services-catalog.md) §3: `.github/workflows/scenario.yml` runs `tests/ci/windows-services.ps1` on every push to `main`; 17 checks plus the before/after comparison, about 5 minutes |
| P3-05 | PostgreSQL provider on Windows: EDB's binaries, `initdb`, `trust` on loopback only, `pg_ctl` stop, `psql` for `kiln db`; two versions side by side | P3-01 | **Done** 2026-11-02, [ADR 0040](adr/0040-postgresql.md): PostgreSQL 18.6 and 17.11 from EDB (pgAdmin left out); `initdb` with `trust` on loopback, UTF-8, C locale; ready once `postmaster.pid` says so; services run without admin rights (restricted token, as `pg_ctl` uses). Linux waits for containers |
| P3-06 | Redis on Windows: choose between `zkteco-home/redis-windows` (downloaded at the user's request, its terms shown) and Valkey built by Kiln; Laravel's cache, sessions and queue checked as in spike 03 | P3-01 | **Done** 2026-11-02, [ADR 0041](adr/0041-redis.md): Redis 8.10.3 and 8.8.3 from `redis-windows/redis-windows`' MSYS2 builds (zkteco rejected: no source, wrong license); databases, Lua in database 1, restart and clean shutdown checked in CI. Laravel's own cache, session and queue checks (spike 03's 22) move to P3-08, with `.env` wiring |
| P3-14 | Meilisearch, RustFS and MongoDB: official Windows releases, one provider each, one ADR | P3-01 | **Done** 2026-11-02, [ADR 0042](adr/0042-meilisearch-rustfs-mongodb.md): Meilisearch 1.56.1, RustFS 1.0.4, MongoDB 9.0.4 and 8.0.34 (SSPL, from MongoDB at the user's request) with `mongosh` as a companion for its clean shutdown and `kiln db`; CI checks a round trip for each |
| P3-15 | phpMyAdmin and Adminer: pinned releases served as Kiln sites (`pma.test`, `adminer.test`), pointed at Kiln's instances | P3-04 | **Done** 2026-11-03, [ADR 0043](adr/0043-phpmyadmin-and-adminer.md): `kiln tool add phpmyadmin|adminer` serves phpMyAdmin 5.2.3 and Adminer 6.2.0 as `pma.test` and `adminer.test`, listing Kiln's instances from `servers.json`, logged in without a password; CI checks both on PHP 8.5 |
| P3-07 | Mailpit's UI served as `mailpit.test` through a proxy site that follows its instance | P3-01 | **Done** 2026-11-03, [ADR 0044](adr/0044-service-web-uis.md): providers name a web UI; the site follows port changes and goes with the instance; over HTTPS when Kiln's CA is set up. `MAIL_*` moved to P3-08 |
| P3-08 | `.env` wiring (`DB_*`, `REDIS_*`, `MAIL_*`): `kiln env use <instance>`, a journaled key-level operation with preview and byte-exact undo; creates the project's database; `kiln new --db` | P3-04 | **Done** 2026-11-03, [ADR 0045](adr/0045-env-wiring.md): key-level edits (in place, uncommenting Laravel's examples, or by group), byte-exact undo, `--force` for an edited file; databases made through the instance's client; `kiln new --db` migrates; `predis` for Redis; uninstall leaves projects' `.env`. CI runs spike 03's Laravel checks on Kiln's Redis |
| P3-09 | Project config: `[services]` in `kiln.toml`, `kiln services up` in a project, `herd.yml` services in `kiln import herd` | P3-08 | **Done** 2026-11-03, [ADR 0046](adr/0046-project-services.md): `[services]` (version or `*`, optional port, file order kept); `kiln services up` reuses or adds, starts and wires them, one database at a time; releases fall back to the major line, then the newest, saying so; `herd.yml` services imported (`minio` → `rustfs`, placeholders dropped). Finishes ADR 0027 |
| P3-16 | Laravel Reverb for a project: `php artisan reverb:start` run and kept up by Kiln, `REVERB_*` wiring | P3-08 | **Done** 2026-11-03, [ADR 0047](adr/0047-reverb.md): providers can run in a project (`runs_in_project`, `project_launch`); schema v8 records the folder; Reverb runs with the project's PHP under the services supervisor; `kiln env use` wires it; `[services]` and `herd.yml` take `reverb` |
| P3-10 | `kilnd` and services: the watchdog restarts the services supervisor after a crash or reboot; `service.*` API methods | P3-01 | **Done** 2026-11-11, [ADR 0048](adr/0048-kilnd-and-services.md): `kiln services resume` (hidden) after a crash or reboot, own backoff, `services.repaired` events; `services.status`, `service.start/stop/restart`; `kiln daemon status` shows services. Extends ADR 0026 |
| P3-11 | Data safety: `kiln db dump/restore`; `--keep-data` at uninstall moves data out | P3-04 | **Done** 2026-11-11, [ADR 0049](adr/0049-data-safety.md): `kiln db dump`/`restore` for MySQL, MariaDB and PostgreSQL (one database or all, never overwriting, restore into a new name); `kiln uninstall --keep-data` copies the data folders to the home folder first |
| P3-12 | The Phase 3 exit run: the CI scenario covering every criterion, plus a by-hand Herd coexistence check on a developer machine | all | **Done** 2026-11-12: CI in both uninstall modes (pages on every database, HTTPS `mailpit.test`, services through `web stop` and a simulated reboot), and `tests/manual/herd-check.ps1` passed next to a running Herd. ADR 0039 (amended) |

**Order:** the table's: P3-13 first (tests on CI), then PostgreSQL, Redis, the other
services, the admin UIs, the wiring tasks, Reverb, `kilnd`, data safety, and P3-12 last.

## Phase 4: the desktop app (Windows)

Design: [ADR 0050](adr/0050-desktop-app.md), on [ADR 0002](adr/0002-rust-core-and-tauri-gui.md)'s
Tauri app over `kilnd`'s API. Windows only: macOS (Phase 4b) and Linux wait, at the user's
request (2026-11-12).

**Exit criteria** (Windows)

- **Tray.** The app starts at login and shows Kiln's state in the tray. Closing its window
  keeps it in the tray; quitting it leaves the web server and services running.
- **Everything the CLI does, from the app:** sites (park, link, HTTPS, PHP per site,
  proxies), PHP and Node versions and extensions, services (add, ports, start/stop, logs,
  wiring a project, dump and restore), projects (`[services]`, `herd.yml`), history with
  undo, doctor and settings. Every change shows the same preview as the CLI first, and
  appears in `kiln history`.
- **First run.** Onboarding previews each system change (trusting the CA, the hosts file,
  the shims on PATH, starting at login) and makes none without consent.
- **Developer tools:** a log viewer that follows logs live; a project's queue worker,
  scheduler and Vite run as project processes; Xdebug per PHP, triggered per request; dumps
  from `dump()` collected and shown.
- **What else the app shows** ([ADR 0052](adr/0052-phase-4-screens-and-scope.md)): Kiln's
  memory, CPU and disk use with a way to clear downloads; updates for PHP and, if the user
  turns it on, for Kiln; each site's request timing from Caddy's log; and an MCP server
  that AI assistants can use once the user turns it on.
- **Data viewers:** WhoDB opens every Kiln database, Redis and MongoDB, already connected.
  phpMyAdmin and Adminer stay.
- **Verification:** the app's end-to-end tests run on CI's Windows runner, and uninstall
  removes the app's login item and leaves the machine as it was.

**Tasks** (status is tracked in [progress.md](progress.md#phase-4))

| ID | Task | Depends on | Notes |
|---|---|---|---|
| P4-01 | The API for the app: read methods for everything it shows; change methods that run the CLI with a new `--json` mode; previews; progress events for downloads; API version 2 | Phase 3 | ADR 0050 §3; [ADR 0053](adr/0053-api-v2-and-json-output.md); **Done** 2026-11-12 |
| P4-02 | The visual design: glass over Kiln's own backdrop, light and dark; the tray popup; a mockup reviewed with the user, then design tokens and base components in the repo | — | [ADR 0051](adr/0051-desktop-visual-design.md); **Done** 2026-11-12 |
| P4-03 | The app's shell: Tauri 2 in `apps/desktop`; tray icon with state and menu; close to tray; one instance; starts at login and starts `kilnd` (one journaled login item) | P4-01 | ADR 0050 §2; [ADR 0054](adr/0054-desktop-app-shell.md); **Done** 2026-11-13 (by-hand check: `tests/manual/app-check.md`) |
| P4-04 | Sites, PHP and Node pages | P4-03 | [ADR 0055](adr/0055-sites-php-node-pages.md); **Done** 2026-11-13 |
| P4-05 | Services page: the catalog with download progress, instances, ports, logs, wiring a project, dump and restore, the data tools | P4-03 | [ADR 0056](adr/0056-services-page.md); done |
| P4-06 | First-run onboarding: each system change previewed, consent per change | P4-03 | ADR 0004 previews; [ADR 0057](adr/0057-first-run-onboarding.md); done |
| P4-07 | History with undo, doctor and settings pages | P4-03 | [ADR 0058](adr/0058-history-doctor-settings.md); built |
| P4-08 | Log viewer: Caddy, PHP errors per site, services, `kilnd`; follow live | P4-03 | [ADR 0059](adr/0059-log-viewer.md); built |
| P4-09 | Dev processes per project: queue worker, scheduler, Vite, as project processes like Reverb | P4-03 | ADR 0047; [ADR 0060](adr/0060-dev-processes.md); built |
| P4-10 | Xdebug: per PHP version, triggered per request (cookie or browser extension); a separate debug worker so other requests stay fast | P4-03 | [ADR 0061](adr/0061-xdebug.md); built |
| P4-11 | Dump capture: `dump()`/`dd()` output collected and shown, by an open-source collector chosen in a spike | P4-03 | [ADR 0062](adr/0062-dump-capture.md): Symfony's dump server; built |
| P4-12 | WhoDB: spike, then "Browse data" with Kiln's instances already connected; phpMyAdmin and Adminer stay | P4-05 | Its server binds every interface: see below |
| P4-13 | Tests on CI: frontend tests, API contract tests against a real `kilnd`, end-to-end runs of the built app (WebDriver) on the Windows runner | P4-03 | Runs with every task |
| P4-14 | The Phase 4 exit run: CI plus a by-hand check on a developer machine | all | |
| P4-15 | The screens: every page drawn in the design gallery from the base components, with sample data that drafts the API's shapes | P4-02 | [ADR 0052](adr/0052-phase-4-screens-and-scope.md); **Done** 2026-11-12 |
| P4-16 | Kiln's footprint: memory and CPU per Kiln process, disk use by folder, clearing the download cache | P4-03 | ADR 0052 |
| P4-17 | Updates: PHP patch updates shown where they apply; checking GitHub for a new Kiln, off by default | P4-03 | ADR 0052 |
| P4-18 | Request timing per site from Caddy's access log: median, slowest 5%, counts, failures, slowest requests; kept on the PC | P4-03 | ADR 0052; its own ADR first |
| P4-19 | An MCP server for AI assistants (`kiln mcp`, from Phase 5): reads sites, services, logs and history; changes through the CLI, in History; off by default | P4-01 | ADR 0052; its own ADR first |
| P4-20 | PECL extensions for Kiln's PHP on Windows (phpredis first, then imagick and others): a spike for builds of 8.4 and 8.5 Kiln can pin and verify, then turning them on per version like the bundled ones. Laravel needs none of them for Redis today (`REDIS_CLIENT=predis`) | P4-04 | Asked for by the user (2026-11-13); shares the DLL question with Xdebug (P4-10) |

**WhoDB (P4-12).** WhoDB (Apache-2.0) covers what Adminer doesn't: MongoDB, Redis and Valkey,
SQLite, ClickHouse, Elasticsearch, as well as MySQL, MariaDB and PostgreSQL. It can be given
connection profiles through `WHODB_MYSQL`, `WHODB_POSTGRES`, `WHODB_MONGODB`, `WHODB_REDIS`
and others. Two facts shape how Kiln can use it (release 0.138.0):
- Its server listens on every network interface (`":" + PORT`), with no setting to limit it
  to 127.0.0.1. Kiln's services listen on loopback only, so its server can't be run as it is.
- Its Windows release is a desktop app (Wails), not the server.
The spike checks the desktop app: does it open a network port, does it read the connection
profiles from its environment, and what does it send (an install heartbeat that
`WHODB_HEARTBEAT_DISABLED` turns off; analytics behind consent). If it passes, Kiln opens it
with its instances preconfigured. If not, Kiln builds the server from source with a
loopback setting and offers that setting upstream.

**Order:** P4-02 and P4-15 first (the design and every screen, with the user), then P4-01
(the API, shaped by the screens), P4-03, the pages (P4-04 to P4-07), the developer tools,
WhoDB and the additions (P4-08 to P4-12, P4-16 to P4-19), with P4-13 growing alongside, and
P4-14 last.

## Phase 3–5 scope (summary)

- **Phase 3:** see above.
- **Phase 4:** see above (Windows). The summary as first planned: a Tauri tray app (sites, PHP/Node, services, onboarding that previews every system
  change, journal viewer with undo); log viewer; dev processes; Xdebug cookie routing; dump
  capture via an open-source collector.
- **Phase 4b:** macOS adapters (resolver file plus stub, Keychain, launchd, a privileged
  helper through `SMAppService`); PHP and services builds for macOS; the GUI on macOS. Needs
  Apple hardware to test on. Builds stay unsigned until Phase 5.
- **Phase 5:** MSIX/winget, Homebrew cask, deb/rpm/AppImage; Developer ID signing and
  notarization on macOS (needs an Apple Developer Program account); signed update
  manifests; share-tunnel plug-ins; workspaces; git worktrees as sites; database snapshots
  before migrations; a Tinker console; LAN sharing; plugins and themes (ADR 0052; `kiln mcp`
  moved to Phase 4 as P4-19); external security review; SBOM.

## Out of scope

Agent studio, dictation and AI widgets; Forge/Vapor/Cloud integrations; site preview
screenshots; Docker orchestration.
