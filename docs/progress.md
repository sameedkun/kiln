# Progress

**Last updated:** 2026-11-03 · **Current phase:** **Phase 3 in progress** (9 of 16 tasks done; ADRs 0035–0043; Windows first, Linux on hold (ADR 0039); next: P3-07) · **Phase 2 done** (11 of 11 tasks; exit criteria met in both VMs; ADRs 0026–0034) · **Phase 1 done** (15 of 15 tasks) · **CI:** green on Windows, Linux, macOS

## At a glance

| Area | State |
|---|---|
| Research (how existing tools work, feature parity, risks) | **done**: private reconnaissance package, outside this repo |
| Architecture and key decisions | **done**: ADRs 0001–0043 accepted |
| Local CA (`kiln-pki`) | **done** (Windows): `.test`-only root, DPAPI-protected key, 30-day intermediate; `kiln ca init/info/trust`; verified on the dev machine: trust → in `CurrentUser\Root` → undo, store identical to baseline ([I-43](issues.md#i-43)) |
| Privileged helper (`kiln-helper`, Windows) | **done**: UAC per batch, private pipe, HMAC-signed messages, fixed operations (protocol v2) |
| Hosts DNS on Windows (`HostsBlockOp`) | **done**: `kiln hosts set` / `clear` and `kiln undo`, journaled, one UAC prompt per change; verified on the dev machine: set → resolve → undo, byte-identical ([I-42](issues.md#i-42)) |
| Repository, CI, license and dependency policy | **done** (plus a CI guard keeping `unsafe` inside `kiln-sys`) |
| Operation engine (`kiln-ops`) | **done**: engine, rollback, undo, recovery, shared journal contract tests |
| Durable state (`kiln-state`) | **done**: SQLite store, journal with full history, one-time JSONL import |
| OS wrappers (`kiln-sys`) | **growing**: `replace_file` / `write_replacing` / `rename_replace`; Windows pipes, SIDs, system directory, UAC launch, process handles, `AF_UNIX` connect, kill-on-close jobs; shared background-process helpers |
| Hosts-file editor (`kiln-platform::hosts`) | **done**: byte-safe, property-tested |
| Web server choice | **done**: Caddy ([spike 01](spikes/results/01-web-server.md)) |
| Windows DNS strategy | **done**: hosts block; NRPT rejected ([spike 02](spikes/results/02-windows-dns-part2.md)) |
| Web server (`kiln-web`, Caddy) | **done** (Windows): `kiln link`, `secure` / `unsecure`, `sites`, `web install/start/stop/reload/status`, `hosts sync`. Site changes **reload live** over a per-user admin socket (P1-07). Sites are **HTTP by default; HTTPS per site**. Verified against real Caddy 2.11.7 and **in Chrome**: HTTP page, HTTPS padlock with no warning, HTTP→HTTPS redirect |
| PHP runtimes (`kiln-php`, `kiln-fetch`) | **done** (Windows): `kiln php available/install/list/remove`; verified downloads from windows.php.net; real install of 8.4.27 checked |
| Serving a PHP site end to end | **works** (Windows): linked or parked projects run PHP through supervised php-cgi workers behind Caddy, with the framework's document root detected (Laravel, Symfony, Drupal, Craft, WordPress, …) |
| Living next to other tools | **done** (Windows): `kiln doctor` names Herd, XAMPP, Laragon, WampServer and who holds each web port; `kiln web start` moves to a free port (80→8080, 443→8443) and says why; `.test` names shared with another tool are flagged. Nothing of theirs is touched |
| Linux (Ubuntu 24.04 Desktop VM) | **part a done**: `kiln link`, `web install/start` (Caddy 2.11.7 for Linux, live reload over a Unix socket), `hosts sync` and `undo` through **one polkit password dialog per action**, `kiln doctor` (nsswitch, systemd-resolved, port owners from `/proc`). Checked in Firefox in the VM. **Part b done:** the CA root key in the desktop keyring; `kiln ca trust` for Chrome and Firefox (no password), `--system` for curl/PHP/Node (one dialog); `kiln uninstall` removes it all, keyring entry included. **Part c done:** `kiln php install 8.4` (Kiln's own static builds from `sameedkun/kiln-php`, SHA-256 pinned in Kiln, ADR 0025) or `--system` (the distribution's PHP), served by PHP-FPM, per-site versions switched live. **Part d done:** `kiln shims install` puts `php`/`composer` shims on PATH through a block in the shell startup files, undone byte for byte. **Linux now matches Windows for Phase 1.** |
| Integration tests in VMs | **done**: scripted runs in Windows 11 (with Laravel Herd running) and Ubuntu 24.04. A full Kiln session, then `kiln uninstall`, leaves the machine **as it was** in both; Herd's site and settings are untouched, and Kiln survives Herd restarting |
| GUI | **not started** (Phase 4) |

<a id="phase-0"></a>
## Phase 0: foundations (done)

| Item | Status | Where |
|---|---|---|
| Apache-2.0 repo, README, CONTRIBUTING (clean-room rule, DCO), SECURITY, CODE_OF_CONDUCT, THIRD_PARTY_NOTICES | done | repo root |
| Rust workspace pinned to 1.99.1, edition 2024, `unsafe_code` forbidden, clippy pedantic | done | `Cargo.toml`, `rust-toolchain.toml` |
| `kiln-core`: validated `Tld` / `HostName` (refuses public TLDs) | done | `crates/kiln-core` |
| `kiln-ops`: `Operation` trait, `Engine` (batch all-or-nothing, undo, revert-all, crash recovery), `MemoryJournal`, `FileJournal` (fsync, torn-line repair) | done | `crates/kiln-ops` |
| `kiln-platform`: adapter traits (`DnsProvider`, `TrustStore`, `PathManager`), hosts block editor (bytes) | done | `crates/kiln-platform` |
| `kiln` CLI: `kiln plan hosts` (read-only preview) | done | `crates/kiln-cli` |
| CI: fmt, clippy `-D warnings`, tests on 3 OSes, cargo-deny, Dependabot | done | `.github/` |
| ADRs 0001–0007 | done | `docs/adr` |
| Spike 01: web server (nginx vs Caddy) | done | `docs/spikes/results/01-web-server.md` |
| Spike 02 Part 1: hosts block in Windows Sandbox | done | `docs/spikes/results/02-windows-dns-part1.md` |
| Spike 02 Part 2: NRPT vs hosts in a Hyper-V VM | done (recheck optional, [I-39](issues.md#i-39)) | `docs/spikes/results/02-windows-dns-part2.md` |

**Tests:** 20 unit and property tests (up to 20,000 property cases run locally). Four real bugs
were caught before release ([I-01](issues.md#i-01), [I-02](issues.md#i-02),
[I-04](issues.md#i-04), [I-05](issues.md#i-05)).

**Gate:** ADRs accepted ✓ · operation-engine property tests ✓ · CI green ✓.

<a id="phase-1"></a>
## Phase 1: MVP on Windows + Linux (done)

Tasks are defined in [plan.md](plan.md#phase-1-mvp-on-windows--linux).

| ID | Task | Status | Notes |
|---|---|---|---|
| P1-01 | State store (SQLite) + journal migration | **done** 2026-10-21 | see below |
| P1-02 | `kiln-helper` (Windows) | **done** 2026-10-21 | [ADR 0009](adr/0009-windows-privileged-helper.md); see below. Open: [I-41](issues.md#i-41); [I-42](issues.md#i-42) verified |
| P1-03 | `HostsBlock` provider (Windows) | **done** 2026-10-21 | see below |
| P1-04 | Linux (parts a–d) | **done** 2026-10-23 (a 10-22; b, c, d 10-23) | [ADR 0020](adr/0020-linux-dns-and-privilege.md), [0021](adr/0021-linux-ca-key-and-trust.md), [0022](adr/0022-linux-php-runtimes-and-fpm.md); see below. a: hosts DNS via pkexec, undo, Caddy, port owners. b: CA key in the keyring, trust in browsers and the system store. c: pinned static PHP or the system's, PHP-FPM pools. d: `php`/`composer` shims on PATH via the shell startup files ([ADR 0023](adr/0023-linux-shims-and-path.md)) |
| P1-05 | Local CA + trust stores | **done** 2026-10-21 (Windows) | see below; Linux key encryption + NSS with P1-04 |
| P1-06 | Caddy integration | **done** 2026-10-21 (Windows) | [ADR 0011](adr/0011-caddy-integration.md); see below. HTTP by default, HTTPS per site |
| P1-07 | Caddy admin API lockdown + live reload | **done** 2026-10-21 | [ADR 0012](adr/0012-caddy-admin-channel.md); see below. Admin on a per-user Unix socket, live reload; verified from a normal terminal |
| P1-08 | PHP runtime provider | **done** 2026-10-21 (Windows) | [ADR 0013](adr/0013-downloads-and-php-runtimes.md); see below. Shared verified downloader; `kiln web install` downloads Caddy |
| P1-09 | Process supervisor | **done** 2026-10-22 (Windows) | [ADR 0014](adr/0014-php-cgi-supervisor.md); see below. php-cgi pools behind Caddy, staggered recycling |
| P1-10 | Sites + drivers | **done** 2026-10-22 | [ADR 0015](adr/0015-parked-folders-and-drivers.md); see below. `kiln park`, document-root drivers (no Valet code) |
| P1-11 | Per-site PHP + shims | **done** 2026-10-22 (Windows) | [ADR 0016](adr/0016-per-site-php-and-shims.md); see below. `kiln isolate`, a pool per version, `php`/`composer` shims, reversible user PATH |
| P1-12 | `kiln doctor` | **done** 2026-10-22 | [ADR 0017](adr/0017-kiln-doctor.md); see below. Read-only checks with fixes; `--json` |
| P1-13 | `kiln plan` / `undo` / `uninstall --dry-run` | **done** 2026-10-22 | [ADR 0018](adr/0018-history-undo-uninstall.md); see below. `kiln history`, `kiln undo` (newest), `kiln uninstall` with full plan |
| P1-14 | Coexistence detection | **done** 2026-10-22 | [ADR 0019](adr/0019-coexistence.md); see below. Herd/XAMPP/Laragon/WampServer detected, free ports picked, shared names warned |
| P1-15 | Integration test VMs | **done** 2026-10-26 (Linux 10-23) | [ADR 0024](adr/0024-integration-test-vms.md); see below. Both scenarios pass: uninstall leaves the machine as it was, Herd untouched and restartable. Found and fixed [I-59](issues.md#i-59), [I-60](issues.md#i-60); [I-36](issues.md#i-36) fixed |

### P1-01 delivered (2026-10-21)

- **I-38 decided** ([ADR 0008](adr/0008-unsafe-confined-to-kiln-sys.md)): no crate wraps
  `ReplaceFileW` safely (`atomicwrites`, `atomic-write-file` and `staged_file` all rename).
  The new `kiln-sys` crate is the only place `unsafe` is allowed, with stricter lints: one
  operation per `unsafe` block, each with a `SAFETY` comment. A CI step fails if any other crate
  drops the workspace lints. `kiln_sys::replace_file` / `write_replacing` have tests on real
  Windows, including the I-25 regression: an explicit deny ACE survives `ReplaceFileW`, and a
  rename drops it.
- **`kiln-state`:** per-user SQLite database (`%APPDATA%\Kiln\state.db`, macOS
  `~/Library/Application Support/Kiln`, Linux `$XDG_STATE_HOME/kiln`).
  - WAL with `synchronous = FULL`, so every acknowledged journal entry is durable.
  - Numbered migrations through `user_version`; databases from a newer Kiln are refused.
  - Tables `settings`, `operations` (latest state) and `operation_events` (every transition).
  - `StateStore` implements `kiln_ops::Journal`.
- **Journal migration:** `StateStore::import_jsonl` replays the Phase 0 JSON Lines history in one
  transaction. It's read-only on the source, refused if anything was already imported or recorded,
  and ignores a torn tail without repairing it. `StateStore::open_dir` does this automatically once
  and keeps the old file as `journal.jsonl.imported`.
- **Contract tests:** `kiln_ops::testing` (feature `testing`) is one conformance suite that all
  three journals pass: memory, JSONL and SQLite.
- **End to end:** `tests/engine_on_sqlite.rs` shows a later "process" rolling back an interrupted
  operation and undoing everything, using only the SQLite journal.
- **CLI:** `kiln state info [--dir]` is read-only and never creates a database.
- **Tests:** 42 total (22 new). Clippy pedantic and cargo-deny are clean.

### P1-02 delivered (2026-10-21)

Design: [ADR 0009](adr/0009-windows-privileged-helper.md). Summary of what exists now:

- **`kiln-sys::win`** (the only `unsafe`, strict lints): `ServerPipe` is single instance, local
  only, with an SDDL DACL; `connect_pipe` uses identification-only QoS and retries;
  `client_process_id`; `current_user_sid` / `process_user_sid`; `system_directory`
  (`GetSystemDirectoryW`, never `SystemRoot`); `spawn_elevated` (UAC) plus `ElevatedProcess::wait`.
- **`kiln-helper` crate (no `unsafe`):**
  - `proto`: framed JSON. HMAC-SHA256 over protocol, direction, `seq` and the exact body bytes.
    The body is parsed only after verification. Strictly increasing `seq`, 1 MiB limit,
    unknown fields and operations rejected.
  - Fixed operations: `Ping`, `HostsSetBlock`, `Shutdown`.
  - `validate`: the helper re-checks the TLD (public ones refused), names, loopback only, and at
    most 10,000 entries.
  - `hosts`: `ReplaceFileW` write, read-only refused, sharing violations retried with backoff,
    missing file created (`created: true`), idempotent.
  - `server::serve`: DACL for the client's user only plus a medium label, checks the client PID,
    one session.
  - `client::HelperClient` and `client::launch` (UAC + connect).
  - `kiln-helper` binary with strict argument parsing (unknown flags → exit 64) and a 120 s
    session deadline.
- **Tests:** 28 new. 6 in `kiln-sys` + 1 manual (UAC). 17 helper unit tests: replay, reflection,
  tampering, wrong key, oversized frames, unknown operations, validation, lock retry, read-only.
  5 end-to-end on real pipes: full session byte-identical, raw signed attack requests refused, wrong key
  cut off, wrong process refused even with the key, and the real binary session plus a smuggled
  `--hosts` refused. The end-to-end suite passed 20 of 20 repeated runs.
- **Not yet:** `kiln` doesn't call the helper. That's the hosts `Operation` in P1-03.

### P1-03 delivered (2026-10-21)

- **`kiln_platform::hosts_op::HostsBlockOp`** (platform-neutral, journal kind
  `dns.hosts.block`) makes Kiln's block contain exactly a set of names. It sits behind a small
  `PrivilegedHosts` trait, so Linux reuses it with a polkit writer.
  - `prepare` records the previous Kiln block and whether the file existed.
  - `apply` and `revert` **ask for elevation only when something has to change**. Re-applying,
    verifying and crash recovery never prompt.
  - `revert` restores the *previous block*, so other tools' later edits survive. It refuses a
    hand-edited block unless `--force`, and deletes the file only if Kiln created it.
  - `register_hosts_block` lets a later process rebuild the op from the journal.
- **`kiln_helper::session::HelperSession`**: a lazily launched, shared helper session. The
  first real write launches `kiln-helper` (one UAC prompt), and the rest of the batch reuses
  it. A declined prompt becomes "nothing was changed".
- **Helper protocol v2** ([ADR 0009 amendment](adr/0009-windows-privileged-helper.md)):
  `remove_file_if_empty` deletes a file only if it would be left with zero bytes (undoing a
  creation).
- **CLI:** `kiln hosts set <names…>` / `kiln hosts clear` show a preview with before/after,
  confirm (`--yes` to skip), apply through the engine with the SQLite journal, and print the
  operation number. `kiln undo <n> [--force]` undoes it. Both roll back interrupted operations
  first.
- **Tests:** 12 new (82 total).
  - 8 operation tests with a direct writer: byte-exact undo, no privileged call when nothing
    changes (including recovery), drift protection, foreign edits survive, a created file is
    deleted, the previous block is restored, params round-trip, preview.
  - 1 helper test for deletion of an emptied created file only.
  - 3 full-stack tests on a real pipe: SQLite journal → engine → op → session → helper →
    file, with undo from a separate "process" and session.
- **Verified on the real machine (2026-10-21):** `kiln hosts set demo` → `ping demo.test` →
  `kiln undo 1` left the hosts file byte-identical, with the same ACL ([I-42](issues.md#i-42)).

### P1-05 delivered (2026-10-21)

Design: [ADR 0010](adr/0010-local-certificate-authority.md).

- **`kiln-pki`** (rcgen with the `ring` backend):
  - Root: ECDSA P-256, `CA:TRUE pathlen:1`, **critical Name Constraints permitting only the dev
    TLD**, valid 3 years. The subject names the owner and date and starts with `Kiln Local
    Development CA`.
  - Intermediate: same constraints, `pathlen:0`, valid 30 days, for Caddy (P1-06).
  - `CaStore`: never overwrites a root. The root key is written **only** as a
    `KeyProtector` blob (DPAPI on Windows, Kiln-specific entropy) and decrypted in memory just
    long enough to sign an intermediate.
- **`kiln-sys::wincrypt`:** `dpapi_protect` / `dpapi_unprotect`; `CertStore` (current user)
  `contains` / `add` / `remove`. `add` **refuses any certificate whose subject isn't a Kiln
  root**. Test stores can be deleted; `Root` and `My` can't.
- **`kiln-platform::trust::TrustCaOp`** (`trust.ca.user`): reversible per-user trust. Undo
  removes the root only if Kiln added it.
- **CLI:** `kiln ca init [--tld]` (idempotent; re-issues the intermediate), `kiln ca info`,
  `kiln ca trust` (preview → confirm → the Windows dialog → journaled, so `kiln undo <n>`
  removes it).
- **Tests:** 13 new (95 total).
  - The chain is verified independently with **rustls-webpki**: `blog.test` and
    `api.blog.test` are accepted; `example.com`, `test.example.com` and `bank.test.com` are
    rejected.
  - Critical name constraints and basic constraints are checked; thumbprints are stable;
    `Debug` never prints the key.
  - The store never overwrites a root, holds no plaintext key on disk, a reloaded store
    issues a working intermediate, and a DPAPI blob is useless without DPAPI.
  - DPAPI round-trips and rejects tampering or wrong entropy.
  - Trust round-trips through the journal, a pre-existing cert is left alone, and a foreign
    root is refused.
  - All store tests use private stores, deleted afterwards (0 left behind).
- **Firefox/NSS:** planned in ADR 0010 §10–12. Windows uses Firefox's enterprise-roots setting
  via a journaled `user.js` change, and Linux uses `certutil` NSS imports. Implemented with
  `kiln doctor` and P1-04.
- **Manual check done 2026-10-21:** the real `CurrentUser\Root` dialog flow ([I-43](issues.md#i-43)).
  It surfaced [I-44](issues.md#i-44) (renames failing in the user profile), fixed with
  `kiln_sys::rename_replace`.

<a id="p1-06"></a>
### P1-06 delivered (2026-10-21)

Design: [ADR 0011](adr/0011-caddy-integration.md). **Sites are plain HTTP by default; HTTPS
is turned on per site.**

- **New crate `kiln-web`:**
  - `config`: Caddy JSON from typed structs.
    - The HTTP server always runs. The HTTPS server exists only when a site is secure.
    - A secure site gets a `308` redirect on HTTP and a certificate from Kiln's intermediate
      (`internal` issuer, `install_trust: false`). The root key is never given to Caddy.
    - Every listener is loopback, every server ends in a 404 catch-all, and both automatic
      HTTPS and the admin endpoint are off.
  - `release`: Caddy 2.11.7 archives pinned by SHA-512 (Windows x64/arm64). The binary is
    extracted as `kiln-caddy.exe` into `%LOCALAPPDATA%\Kiln\bin`.
  - `process`: background start (validate first, no console window, its own process group),
    readiness check, and a PID file with the executable path.
    - Stop and status open the process and compare its image path, so a reused PID is
      never killed.
    - A port in use (Herd on 80/443) is reported, with `--http-port` / `--https-port` suggested.
- **`kiln-state`:** migration v2, a `sites` table (`secure` defaults to 0).
  `default_local_dir()` points at `%LOCALAPPDATA%\Kiln`.
- **`kiln-sys::win::Process`:** open by PID, image path, is running, terminate, wait.
- **CLI:**
  - Sites: `kiln link [name] [--path] [--secure]`, `kiln unlink`, `kiln secure` /
    `kiln unsecure`, `kiln sites`.
  - Web server: `kiln web install --archive`, `kiln web start [--http-port] [--https-port]`,
    `kiln web stop`, `kiln web restart`, `kiln web status`, `kiln web config`.
  - DNS: `kiln hosts sync`.
  - Site changes restart a running server.
  - Making a site secure re-issues the intermediate if it has fewer than 10 days left, and
    warns if the root isn't trusted.
- **Tests:** 24 new, 119 total.
  - Config invariants, the installer (pinned / unpinned / missing executable), PID-file safety,
    process handles and site storage.
  - **A real-Caddy end-to-end test** (`KILN_TEST_CADDY=<caddy.exe> cargo test -p kiln-web
    --test caddy`):
    - HTTP returns 200, an unknown host returns 404, and the secure site redirects (308).
    - HTTPS verifies against Kiln's root with rustls. A plain site has no certificate.
    - With no secure sites, the HTTPS port isn't bound, and stopping releases the ports.
    - It passes locally against Caddy 2.11.7. CI runs it on Windows and Linux since 2026-10-26, against the Caddy `kiln web install` downloads.
- **Manual run from the agent session:** install, link `hello` (HTTP) and `shop` (`--secure`),
  then start.
  - With default ports, start refused: `127.0.0.1:80 is already in use … Herd?`.
  - On 8080/8443: HTTP returned 200, `shop.test` redirected (308) to
    `https://shop.test:8443/some/page`, unknown hosts got 404, and all four listeners were on
    loopback.
  - HTTPS failed there only because Caddy can't write its certificate storage inside the
    Claude app's MSIX container ([I-45](issues.md#i-45)).

**Browser check (passed 2026-10-21, run by the user in Chrome: all three URLs as expected, HTTPS with no warning; hosts and trust undone afterwards)** to run from a normal terminal (Windows Terminal or PowerShell from the Start
menu, not from the Claude app; state created inside the app isn't visible there):

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe web install --archive spikes\web-server\.cache\dl\caddy_2.11.7_windows_amd64.zip
.\target\debug\kiln.exe ca init
.\target\debug\kiln.exe ca trust            # Windows dialog: Yes
.\target\debug\kiln.exe link hello --path D:\Code\Rea\laravel-herd\demo-sites\hello
.\target\debug\kiln.exe link shop  --path D:\Code\Rea\laravel-herd\demo-sites\shop --secure
.\target\debug\kiln.exe hosts sync          # UAC prompt: Yes
.\target\debug\kiln.exe web start --http-port 8080 --https-port 8443
# Browser: http://hello.test:8080/ (plain) and https://shop.test:8443/ (padlock, no warning)
.\target\debug\kiln.exe web stop
.\target\debug\kiln.exe undo <n>            # the hosts and trust operation numbers printed above
```

<a id="p1-07"></a>
### P1-07 delivered (2026-10-21)

Design: [ADR 0012](adr/0012-caddy-admin-channel.md). Resolves [I-20](issues.md#i-20).

- **Spike:** Caddy's admin endpoint works on a Unix socket on Windows.
  - No TCP admin port is opened, `POST /load` reloads live, and `POST /stop` shuts down
    cleanly and removes the socket.
  - Paths must stay under 108 bytes.
  - On Windows the first admin request after each load gets an EOF ([I-46](issues.md#i-46)).
- **Config:** `admin.listen = unix/<local data>/caddy/admin.sock` (was: admin disabled).
  Socket paths over 107 bytes are refused.
- **`kiln-sys::win::connect_unix`:** `AF_UNIX` connect over Winsock, returned as a std socket
  stream. Path bytes are UTF-8, as in Go. Tested against a test-only `AF_UNIX` listener.
- **`kiln-web::admin::AdminClient`:** minimal HTTP/1.1 over the socket (length-delimited,
  chunked, or read to end of stream). Retries until a response arrives (I-46). `load`, `stop`,
  `config`.
- **`WebServer`:**
  - `reload`: refuses up front if a newly needed port is taken. Caddy validates the config
    atomically and keeps the old one on error. The config file is updated only on success.
  - `stop`: graceful through the admin API first, with the verified terminate as a fallback.
  - The run directory is private (inherits the profile ACL on Windows, `0700` on Unix).
  - A leftover socket no longer blocks or hides start-up errors, which now include the errors
    Caddy logged during that start.
- **CLI:** `link`, `unlink`, `secure` and `unsecure` reload instead of restarting. New
  `kiln web reload`. `restart` stays for port changes.
- **Tests:** 9 new, 128 total. The real-Caddy end-to-end test now covers:
  - start HTTP-only, then reload HTTPS on (same PID, certificate issued) and off (HTTPS
    listener released);
  - 9 reloads under continuous load (the server never goes down; at most one empty close per
    reload, [I-47](issues.md#i-47));
  - a reload refused for a taken port while the old config keeps serving;
  - a graceful stop (socket removed, ports released).
- **Honest numbers ([I-47](issues.md#i-47)):** under a saturating load (about 1,400 new
  connections/s), a reload drops one brand-new connection per 3–7 reloads. A restart refuses
  every connection for about 540 ms.
- **Not checkable from the agent session:** the Claude app's container breaks `AF_UNIX` in
  AppData ([I-45](issues.md#i-45)), so `kiln web start` fails there with Caddy's real error,
  which is now shown.

**Live-reload check (passed 2026-10-21, run by the user: `unsecure` and `secure` both reloaded with the server PID unchanged (21656); `admin.sock` present as a socket reparse point; nothing on port 2019; graceful stop; hosts and trust undone)** to run from a normal terminal (your sites, CA and Caddy from P1-06 are
still there; trust and the hosts entries were undone):

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe hosts sync          # UAC prompt: Yes
.\target\debug\kiln.exe ca trust            # Windows dialog: Yes
.\target\debug\kiln.exe web start --http-port 8080 --https-port 8443
.\target\debug\kiln.exe web status          # note the PID
.\target\debug\kiln.exe unsecure shop       # "Web server reloaded"; http://shop.test:8080/ now loads over HTTP
.\target\debug\kiln.exe secure shop         # reloaded; http://shop.test:8080/ jumps to https://shop.test:8443/ (padlock)
.\target\debug\kiln.exe web status          # same PID: no restart
dir $env:LOCALAPPDATA\Kiln\caddy            # admin.sock is there
netstat -ano | findstr :2019                # prints nothing: no TCP admin port
.\target\debug\kiln.exe web stop            # admin.sock is gone afterwards
.\target\debug\kiln.exe undo <n>            # the hosts, then the trust operation numbers printed above
```

<a id="p1-08"></a>
### P1-08 delivered (2026-10-21)

Design: [ADR 0013](adr/0013-downloads-and-php-runtimes.md).

- **New crate `kiln-fetch`:** verified downloads.
  - The expected SHA-256/SHA-512 is known before the first byte. The body is hashed while it
    streams to `.partial`, and only a matching file is moved into
    `%LOCALAPPDATA%\Kiln\downloads`.
  - HTTPS only, redirects included. Server certificates are checked against the Windows
    certificate store (`rustls-platform-verifier`), and proxies come from the environment.
  - The cache is reused only after re-hashing. There is a 512 MB cap and a 1 h body timeout
    ([I-48](issues.md#i-48)).
  - Client: `ureq` 3 with rustls and ring, so no OpenSSL.
- **New crate `kiln-php`:**
  - `index`: parses `releases.json` from `downloads.php.net/~windows/releases/` (where
    windows.php.net redirects). Per version it picks the x64 NTS build (`nts-vs16/17-x64`).
    File names must be plain, and SHA-256s are validated.
  - `resolve`: `8.4` → the latest patch. An exact older patch is refused, because it can't be
    verified (archives have no checksums).
  - `install`: verified download; extraction that refuses `..`/absolute paths and symlinks,
    with a 1 GB cap; `php.ini` from `php.ini-development` plus a Kiln section; `php -v` must
    report the version with no extension warnings.
    - A missing Visual C++ runtime is explained, with Microsoft's link, and not installed.
    - The folder carries an incomplete-marker until the check passes ([I-49](issues.md#i-49)).
- **`kiln-state`:** migration v3, a `php_versions` table (version, folder, archive, SHA-256).
- **CLI:** `kiln php available`, `kiln php install <8.4|8.4.27>`, `kiln php list`,
  `kiln php remove <version>` (only deletes inside Kiln's PHP folder). `kiln web install` now
  downloads the pinned Caddy itself; `--archive` stays for offline use.
- **`deny.toml`:** licences are judged for the platforms Kiln ships for. A wasm-only CDLA crate
  in `rustls-platform-verifier`'s graph is never built into Kiln.
- **Tests:** 20 new, 148 total.
  - The downloader against a local server: cache reuse offline, mismatch discarded, tampered
    cache refetched, SHA-512, redirect, 404, size limit, HTTP refused.
  - Index parsing and resolution, including an index pointing outside its folder.
  - `php.ini` rendering, extraction escape refused, interrupted install redone, a failed check
    leaving nothing behind, `php -v` parsing, and PHP state rows.
- **Real run (agent session, 2026-10-21):**
  - `kiln php available` listed 7.4.33 to 8.5.12 from the live index.
  - `kiln php install 8.4.26` was refused, pointing to 8.4.27.
  - `kiln php install 8.4` downloaded 34 MB and verified SHA-256 `93bd8a81…a41bf5236`. The
    first try hit I-48 and then I-49; both are fixed. PHP 8.4.27 installed, with
    `curl, fileinfo, gd, intl, mbstring, exif, openssl, pdo_mysql, pdo_pgsql, pdo_sqlite,
    sqlite3, sodium, zip` enabled and loading cleanly. PDO SQLite and OpenSSL work.
  - Install again was a no-op, remove deleted it, and a reinstall worked offline from the
    cache.
  - `kiln web install` downloaded Caddy 2.11.7 from GitHub through its CDN redirect and
    accepted it on the pinned SHA-512.

<a id="p1-09"></a>
### P1-09 delivered (2026-10-22)

Design: [ADR 0014](adr/0014-php-cgi-supervisor.md). Linked sites now run PHP.

- **New crate and binary `kiln-supervisor`** (next to `kiln.exe`, so rebuilding `kiln` isn't
  blocked by a running server):
  - `pools.json` is the desired state, re-read when it changes.
  - Worker *i* of *N* recycles after `500 + i·500/N` requests (staggered, I-14).
  - Exited workers restart on the same port.
  - Only a non-zero exit is a failure ([I-51](issues.md#i-51)), and a quick failure backs off
    0.25 s → 5 s.
  - Every worker sits in a **kill-on-close job**, so workers can't outlive the supervisor and
    nothing is ever killed by image name (Herd's php-cgi is safe).
  - Removing `pools.json` stops it gracefully.
- **`kiln-sys`:** `win::KillOnCloseJob`, and `background` (PID files that verify the
  executable, background spawn, verified terminate), now shared by Caddy and the supervisor.
- **`kiln-web`:** with a pool, every site gets Caddy's `php_fastcgi` route as typed JSON
  (checked against `caddy adapt`):
  - directory redirect, `index.php` front controller, `*.php` → FastCGI, static files direct;
  - `least_conn`, retries bounded by time (2 s / 50 ms), no passive health checks (I-14, I-15).
  - Without PHP, sites stay static.
- **CLI:**
  - `kiln web start` starts the workers first, then Caddy. `stop` stops Caddy, then the
    supervisor. `status` shows the PHP pool.
  - `kiln php default <version>` and `kiln php workers <n>` switch a running server live
    (both pools side by side, Caddy reload, 5 s drain, retire old).
  - `kiln php remove` refuses the version that's serving sites.
- **Tests:** 19 new, 167 total. The real end-to-end test (`KILN_TEST_CADDY` +
  `KILN_TEST_PHP`) uses a real supervisor, real php-cgi 8.4.27 and real Caddy:
  - PHP, front controller, directory redirect, `.php` files and static files;
  - **400 requests while workers recycle every 10–16 requests: 0 failures, 25–34 different
    php-cgi processes**, in all 12 runs;
  - a live pool switch under load costs at most one request at the Caddy reload
    ([I-50](issues.md#i-50));
  - stopping the supervisor leaves no worker running.
- **Found and fixed on the way:**
  - healthy workers held back as "crashes" ([I-51](issues.md#i-51));
  - perl edits using `|` delimiters mangled two files. Caught by the compiler, rewritten, and
    such edits are no longer used.
- **Checked by the user from a normal terminal (2026-10-22):** see the P1-10 log row.

**Check to run from a normal terminal** (the demo site
`D:\Code\Rea\laravel-herd\demo-sites\phpdemo` is ready):

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe php install 8.4      # ~34 MB from windows.php.net, SHA-256 verified
.\target\debug\kiln.exe link phpdemo --path D:\Code\Rea\laravel-herd\demo-sites\phpdemo
.\target\debug\kiln.exe hosts sync           # UAC prompt: Yes; note the operation number
.\target\debug\kiln.exe web start --http-port 8080 --https-port 8443
# Browser: http://phpdemo.test:8080/ and http://phpdemo.test:8080/any/path
#          (PHP version, the path, and the php-cgi PID; reload: the PID changes as workers take turns)
.\target\debug\kiln.exe web status           # Caddy + "PHP 8.4.27: 4 php-cgi workers on …"
Get-Process php-cgi | Select-Object Id, Path # 4 workers, all under %LOCALAPPDATA%\Kiln\php
.\target\debug\kiln.exe web stop             # stops Caddy, then the supervisor and all 4 workers
Get-Process php-cgi -ErrorAction SilentlyContinue | Where-Object Path -like "*\Kiln\php\*"   # nothing
.\target\debug\kiln.exe undo <n>             # the hosts operation number
```

<a id="p1-10"></a>
### P1-10 delivered (2026-10-22)

Design: [ADR 0015](adr/0015-parked-folders-and-drivers.md).

- **New crate `kiln-sites`:**
  - `driver::detect` picks the document root from a project's documented layout:
    Laravel/Symfony → `public/`; Drupal/Craft/Bedrock → `web/`; WordPress, plain PHP and
    static sites → the folder itself. `public/index.php` and `web/index.php` fall back to
    generic PHP.
  - `resolve` merges links with parked folders. A link wins; the folder parked first wins.
    Hidden and invalid names are skipped and reported.
- **Valet's drivers were considered and not used.** Kiln only needs the document root, and its
  Caddy route already does the rest. No Valet code is included, and
  `THIRD_PARTY_NOTICES.md` says so.
- **`kiln-state`:** migration v4, a `parked` table.
- **`kiln-web`:** `SiteSpec.php`. Static sites get the plain file server even when a PHP pool
  runs.
- **CLI:**
  - `kiln park [folder]` / `kiln unpark [folder]`.
  - `kiln sites` shows URL, driver, link/parked and document root, plus skipped folders.
  - `kiln link` reports the detected driver.
  - `kiln secure` on a parked site pins it as a link; `unlink` returns it to parked.
  - `kiln hosts sync` covers parked sites and reloads a running server.
- **Tests:** 10 new, 177 total.
  - Every driver rule, including "a folder named `artisan` doesn't count".
  - Parking precedence, pinning, unreadable folders, and parked rows in the database.
  - In the real Caddy + PHP test, a parked Laravel-shaped project is served from `public/`
    with its front controller. `/.env`, `/../.env`, `/%2e%2e/.env` and `/artisan` never reveal
    the project's secrets.
- **Smoke run in the agent session:** parking `demo-sites` found `larademo` (laravel → `public`)
  and `phpdemo` (php). The existing `hello`/`shop` links took precedence over their parked
  folders. Securing `larademo` pinned it, and unlinking returned it to parked.
- **CI:** macOS now runs nightly (03:17 UTC) and on manual dispatch. Windows and Linux run on
  every push (`c37bf8a`).

**Check to run from a normal terminal** (`demo-sites` now also holds `larademo`, a minimal
Laravel-shaped project):

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe park D:\Code\Rea\laravel-herd\demo-sites   # lists each subfolder and its driver
.\target\debug\kiln.exe sites                # larademo: laravel, serving ...\larademo\public
.\target\debug\kiln.exe hosts sync           # UAC prompt: Yes; note the operation number
.\target\debug\kiln.exe web start --http-port 8080 --https-port 8443
# Browser: http://larademo.test:8080/any/path   ("Laravel-style front controller: /any/path")
#          http://larademo.test:8080/../artisan (must not show the artisan file)
#          http://phpdemo.test:8080/            (the P1-09 PHP page)
.\target\debug\kiln.exe web stop
.\target\debug\kiln.exe unpark D:\Code\Rea\laravel-herd\demo-sites
.\target\debug\kiln.exe undo <n>             # the hosts operation number
```

<a id="p1-11"></a>
### P1-11 delivered (2026-10-22)

Design: [ADR 0016](adr/0016-per-site-php-and-shims.md).

- **Per-site PHP:** `kiln isolate <version> [site]` / `kiln unisolate [site]`. The site defaults
  to the one containing the current folder.
  - Pins are stored per host (`isolations`, migration v5), so they work for links and parked
    sites alike.
  - A minor pin (`8.3`) follows the newest installed 8.3.x.
  - A removed version falls back to the default, with a warning.
- **One pool per version in use.** `WebConfig.php_pools` (id → upstreams) and
  `SiteSpec.php_pool` route each site to its own version's workers.
  - Changing pins, the default version or the worker count on a running server reuses pools
    that are still needed (same ports, untouched sites). Only new pools start, and unused ones
    retire after the grace period.
  - `kiln sites` shows each site's PHP, marking pins and fallbacks.
- **New crate `kiln-runtime`:** the single "which PHP" logic (default, pins, folder → site),
  shared by the CLI, the pool planner and the shim.
- **Shims:** the `kiln-shim` binary, installed as `php.exe` and `composer.exe`.
  - It runs the folder's PHP with the same arguments and console, and passes the exit code
    back.
  - It survives Ctrl+C itself without stopping PHP from handling it.
  - `kiln which [folder]` explains the choice.
- **Composer:** `kiln composer install` downloads the current stable `composer.phar`
  (version from getcomposer.org, SHA-256 from its checksum file).
- **PATH:** `kiln shims install` copies the shims and puts their folder first on the **user**
  `PATH` as a journaled operation (`env.user_path.prepend`).
  - The preview shows which `php`/`composer` new terminals get now and after.
  - `kiln undo` removes only Kiln's entry, keeping later edits.
  - The registry type is kept, and Windows is notified.
- **`kiln-sys`:** current-user registry strings (`REG_SZ`/`REG_EXPAND_SZ`, unexpanded),
  `WM_SETTINGCHANGE` broadcast, and `survive_ctrl_c`.
- **Tests:** 15 new, 192 total. Real checks with PHP 8.4.27 + 8.3.36 + Composer 2.10.3:
  - The real `kiln-shim` as `php.exe` in a throwaway Kiln state reports 8.3.36 inside the
    pinned site and 8.4.27 elsewhere. Exit codes and `a b "c" ^%PATH%! & | < >` pass through
    unchanged. `composer --version` runs on the site's 8.3.36.
  - One Caddy serves `modern.test` from PHP 8.4.27 and `legacy.test` from 8.3.36.
  - The PATH operation is tested in memory: undo keeps later edits, an existing entry isn't
    duplicated, an empty PATH works. Registry access is tested in a private throwaway key,
    never your real `PATH`.
- Installed in the agent session for testing: PHP 8.3.36 and Composer 2.10.3, both verified.

**Check to run from a normal terminal** (changes your user PATH, with your confirmation and
undoable):

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe php install 8.3
.\target\debug\kiln.exe composer install
.\target\debug\kiln.exe park D:\Code\Rea\laravel-herd\demo-sites
.\target\debug\kiln.exe isolate 8.3 larademo
.\target\debug\kiln.exe sites                 # larademo: 8.3.36 (pinned); phpdemo: 8.4.27
.\target\debug\kiln.exe hosts sync            # UAC: Yes; note the number
.\target\debug\kiln.exe web start --http-port 8080 --https-port 8443   # two PHP pools listed
# Browser: http://phpdemo.test:8080/ → PHP 8.4.27 ; http://larademo.test:8080/x → "(PHP 8.3.36)"
.\target\debug\kiln.exe shims install         # shows today's `php` (e.g. Herd's) vs Kiln's; y to apply; note the number
# NEW terminal:
#   cd D:\Code\Rea\laravel-herd\demo-sites\larademo ; php -v      → 8.3.36
#   cd ..\phpdemo ; php -v                                       → 8.4.27
#   composer --version                                           → Composer 2.10.3 … PHP 8.4.27
.\target\debug\kiln.exe web stop
.\target\debug\kiln.exe undo <shims number>   # PATH back as it was (your own later edits kept)
.\target\debug\kiln.exe unisolate larademo ; .\target\debug\kiln.exe unpark D:\Code\Rea\laravel-herd\demo-sites
.\target\debug\kiln.exe undo <hosts number>
```

<a id="p1-12"></a>
### P1-12 delivered (2026-10-22)

Design: [ADR 0017](adr/0017-kiln-doctor.md).

- **`kiln doctor`** checks state, environment, ports, web server, PHP, sites, DNS,
  certificates and shims. It **changes nothing**: each finding says what to run.
  - It opens the database without migrating it.
  - `--json` prints the same findings for scripts and the later GUI.
  - The exit code is 1 if anything failed.
- **New crate `kiln-doctor`:** the rules are pure functions from observed facts to findings,
  unit-tested with made-up facts. The CLI gathers the facts.
- **`kiln-sys`:**
  - `tcp_listeners()` (IPv4/IPv6 listener table with owning PIDs), so a taken port names its
    program and path, e.g. Herd's nginx;
  - `process_parents()` (ToolHelp snapshot);
  - `Process::open_for_query` (works on packaged apps that refuse terminate rights).
- **What it catches:**
  - operations left half-done (→ `kiln undo <n>`);
  - **running inside a packaged app's container (I-45)**;
  - ports held by other programs;
  - Caddy not installed or not running, or its admin socket silent;
  - incomplete PHP builds, pins without their version, the supervisor down or workers not
    listening, recent `php-errors.log` lines;
  - skipped folders;
  - sites missing from the hosts file, not resolving, or resolving elsewhere, and stale hosts
    entries;
  - an untrusted CA, the intermediate expiring, the **Schannel revocation note (I-12)**, and
    Firefox;
  - shims installed but not first on `PATH`.
- **Found while building it:** Windows reports a packaged app's real path (`E:\WindowsApps\…`
  on this machine, where apps are installed to E:), not `C:\Program Files\WindowsApps\…`. The
  first version missed the container for that reason. The fix and a test with the real paths
  are in.
- **Tests:** 11 new, 203 total. Run in the agent session, it flags the container, lists the
  ports as free, PHP 8.3.36 and 8.4.27, sites missing from the hosts file and the untrusted
  CA, and says shims aren't first on `PATH`.

**Check from a normal terminal:**

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe doctor            # Environment: "normal terminal"; your real sites, PHP, shims state
.\target\debug\kiln.exe web start
.\target\debug\kiln.exe doctor            # Ports: "Kiln's web server (PID …)"; PHP pools listening
.\target\debug\kiln.exe doctor --json     # the same, as JSON
.\target\debug\kiln.exe web stop
```

<a id="p1-13"></a>
### P1-13 delivered (2026-10-22)

Design: [ADR 0018](adr/0018-history-undo-uninstall.md).

- **`kiln history [--all]`:** what Kiln changed, newest first, with time, state and a plain
  description, e.g. `hosts file: blog.test, shop.test`, `trusted for your account: CN=Kiln…`,
  `C:\…\shims first on your user PATH`. By default it shows only changes still in effect or
  needing attention.
- **`kiln undo [n]`:** without a number it undoes the newest change still in effect. It
  describes the change before asking, and refuses one that's already undone, giving its state.
- **`kiln uninstall [--dry-run] [--keep-files] [--yes]`** (and `kiln plan uninstall`) always
  prints the full plan first:
  - what it stops;
  - every change it undoes, newest first;
  - stuck changes that need attention;
  - Kiln's two folders with their sizes;
  - what is never touched (projects, Herd and other tools, the kiln programs).

  The order is stop → undo (`revert_all`) → delete, with the state folder last.
  - If an undo fails, it stops and deletes nothing, so it can be re-run.
  - The state folder is kept while anything is still in effect or stuck.
  - It deletes only folders named `Kiln` under a parent, never one holding a linked or parked
    project, and doesn't follow junctions.
- **`kiln-platform::describe`:** one-line descriptions from journal parameters.
  **`kiln-state`:** `operation_times()`.
- **Tests:** 9 new, 212 total. A sandboxed run of the real `kiln` binary (own APPDATA /
  LOCALAPPDATA) checks that:
  - the dry run deletes nothing;
  - `uninstall --yes` removes both Kiln folders while the parked project survives;
  - **a junction inside Kiln's folder pointing elsewhere is removed without deleting what it
    points to**;
  - a Kiln folder holding a project is kept, even when APPDATA is spelled with 8.3 short names
    ([I-52](issues.md#i-52): a real bug the first CI run caught, fixed by comparing canonical
    paths);
  - `--keep-files` keeps everything.

  Real journal undos in order are covered by the engine's `revert_all` tests. In the sandbox
  they would touch the real hosts file, trust store or PATH.

**Check from a normal terminal** (the last, optional part really uninstalls):

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe hosts sync           # UAC: Yes (gives history something to show)
.\target\debug\kiln.exe history              # "#n … in effect  hosts file: …"
.\target\debug\kiln.exe uninstall --dry-run  # stop / undo #n / delete both Kiln folders (sizes) / never touched
.\target\debug\kiln.exe undo                 # no number: undoes that newest hosts change (UAC: Yes)
.\target\debug\kiln.exe history --all        # now shows it as "undone"
# Optional, real: removes Kiln completely (PHP builds and downloads too; `kiln php install` brings them back)
# .\target\debug\kiln.exe uninstall
```

<a id="p1-14"></a>
### P1-14 delivered (2026-10-22)

Design: [ADR 0019](adr/0019-coexistence.md). Surveyed first, read-only, on the dev machine:
Herd's `nginx.exe` holds 127.0.0.1:80 and :443 (as 12 processes sharing the sockets), Herd's
block in the hosts file lists 44 `.test` names, and `php` on PATH is Herd's `php.bat`.

- **New crate `kiln-coexist`** (read-only):
  - installed tools from their well-known folders: Herd, XAMPP, Laragon, WampServer;
  - which tool a program belongs to, from the folders in its path;
  - a port holder described for people: `Laravel Herd (nginx.exe)`,
    `Apache httpd (C:\Apache24\bin\httpd.exe)`, `the Windows HTTP service (IIS / http.sys)`;
  - names other programs claim in the hosts file, outside Kiln's block, credited to Herd
    when inside Herd's marked block;
  - the port lists and first-free picking.
- **`kiln web start` picks free ports.** Without `--http-port` / `--https-port` it uses 80,
  else 8080, 8000, 8081, 8888 (HTTPS: 443, else 8443, 4443, 9443), and says why:
  `HTTP port 80 is used by Laravel Herd (nginx.exe); Kiln uses 8080.`
  - The pick is saved with a marker, so URLs stay the same and the next start keeps it while
    it is free ([I-53](issues.md#i-53): a real bug a unit test caught).
  - A port the user chose is never moved. If it's busy, the start is refused with the owner,
    a free suggestion and `--auto-ports`; nothing is saved. Ports saved before this task
    count as chosen.
  - `kiln web start --auto-ports` hands both ports back to Kiln.
- **`kiln link` / `kiln park`** print a note when a site name is also another tool's, with
  the Kiln URL that opens Kiln's site.
- **`kiln doctor`** has an **Other tools** area: installed tools, who listens on the web ports
  (one line per program), shared site names (the only warning), `php` on PATH when it is
  another tool's, and Herd's `nginx.exe` killing ([I-11](issues.md#i-11)) explained. It is
  shown even before Kiln has been used. The **Ports** area names the tool too, and a port
  Kiln picks itself is only info, since the next start moves it.
- **Tests:** 10 new, 222 total. Unit tests use a real held port for the picking rules.
  Checked here against the running Herd: `kiln doctor` and a sandboxed `kiln web start`
  reported Herd on 80/443 and chose 8080/8443.

**Check from a normal terminal** (Herd running; Kiln has no state yet after the P1-13
uninstall):

```powershell
cd D:\Code\Rea\laravel-herd\kiln
.\target\debug\kiln.exe doctor                    # "Other tools": Herd installed, its nginx on :80/:443, php is Herd's
.\target\debug\kiln.exe web install               # downloads Caddy again (SHA-512 verified)
.\target\debug\kiln.exe link phpdemo --path D:\Code\Rea\laravel-herd\demo-sites\phpdemo
.\target\debug\kiln.exe link herd-studio --path D:\Code\Rea\laravel-herd\demo-sites\phpdemo   # Note: also Herd's name
.\target\debug\kiln.exe web start                 # "HTTP port 80 is used by Laravel Herd (nginx.exe); Kiln uses 8080."
.\target\debug\kiln.exe doctor                    # Ports: Kiln's server; Other tools: herd-studio.test warning
.\target\debug\kiln.exe web stop
.\target\debug\kiln.exe web start --http-port 80  # refused: names Herd, suggests 8080 or --auto-ports
.\target\debug\kiln.exe web start                 # still 8080 (nothing was saved)
.\target\debug\kiln.exe web stop
.\target\debug\kiln.exe unlink herd-studio
```

<a id="p1-04a"></a>
### P1-04 part a delivered (2026-10-22)

Design: [ADR 0020](adr/0020-linux-dns-and-privilege.md). P1-04 is split into parts a–d;
this is part a. Reference system: the new **Ubuntu 24.04 Desktop** Hyper-V VM
(`tests/vm/ubuntu/new-vm.ps1`, see [dev-setup](dev-setup.md)).

- **DNS:** `.test` names go in Kiln's block in `/etc/hosts`, as on Windows. Ubuntu's
  nsswitch puts `files` first, and systemd-resolved answers from `/etc/hosts` too.
- **Privilege:** `pkexec kiln-helper --stdio`, once per action, with `sudo` if `pkexec` is
  missing.
  - The same authenticated protocol as on Windows, over the helper's stdin/stdout.
  - The session key is the first stdin line, never an argument (`/proc` is readable).
  - A dismissed dialog, or no way to ask (SSH), is "approval was declined; nothing was
    changed".
- **`/etc/hosts` writes** keep mode, owner and extended attributes (the SELinux label; new
  `kiln-sys::xattr`). A symlinked hosts file is refused. A mount point (containers, WSL) is
  written in place. An immutable file is explained, never unlocked.
- **`kiln hosts sync`, `kiln undo`, `kiln uninstall`** work on Linux.
- **Web server:** Caddy 2.11.7's Linux `.tar.gz` releases are pinned (SHA-512), and installs
  read `.tar.gz` (`tar` + `flate2`). A normal user can't bind ports below 1024, so automatic
  ports use 8080/8443 and say why.
- **`kiln doctor` on Linux:** the nsswitch order, who manages `resolv.conf`, and port owners
  from `/proc` ([I-55](issues.md#i-55)). `which` follows Unix `PATH` rules.
- **Tests:** 7 new, 230 total on Windows; 198 run in the VM (the rest are Windows-only),
  all passing. New: stdio sessions on real OS pipes (wrong key, malformed key line, a public
  TLD refused inside the helper), xattrs kept on ext4, symlink refusal, `.tar.gz` install,
  the `/proc/net/tcp{,6}` parser, nsswitch order.

**Checked over SSH in the VM:**
- the build and every test;
- the web server serving a site on 8080;
- a site added live (same Caddy PID, owner-only admin socket, nothing on :2019);
- the doctor's DNS lines;
- `hosts sync` over SSH failing safely, with `/etc/hosts` byte-identical (SHA-256) and the
  operation rolled back;
- with Python on 8080, `web start` naming it and moving to 8000.

**Checked by the user in the VM's own Terminal:** see the log entry for 2026-10-22.

<a id="p1-04b"></a>
### P1-04 part b delivered (2026-10-23)

Design: [ADR 0021](adr/0021-linux-ca-key-and-trust.md). Measured first in the Ubuntu VM, with
Kiln's root and an HTTPS site:
- the system store (`update-ca-certificates`) makes curl trust the site, but **not Chrome**;
- the user NSS database `~/.pki/nssdb` makes Chrome trust it, and removing the root
  untrusts it again;
- the Firefox snap keeps its own NSS database per profile.

- **Root key in the desktop keyring:** a Secret Service item
  (`org.freedesktop.secrets`, over `zbus` 6), with only a reference in
  `root.key.protected`.
  - No usable keyring (a server, or SSH to a desktop nobody is logged in to): a `0600` file
    as before, and `kiln ca init` / `kiln ca info` / `kiln doctor` say how the stored key is
    kept.
  - A locked keyring with a desktop shows the desktop's unlock dialog. Without a desktop,
    Kiln explains at once instead of waiting.
- **`kiln ca trust`** (no password) adds the root to every browser NSS database with
  `certutil`: `~/.pki/nssdb`, created as Chrome would if a Chromium browser is installed;
  the Chromium snap; Firefox deb, snap and Flatpak profiles.
  - One journaled `trust.ca.nss` operation per database. Undo removes only what Kiln added,
    and a database Kiln created goes again if nothing else moved in.
  - Without `certutil`, Kiln says what to install per distribution.
- **`kiln ca trust --system`** adds `trust.ca.system` through the helper (protocol v3:
  `TrustSystemCa` / `UntrustSystemCa`). The helper accepts only a CA with a Kiln subject
  whose **critical name constraints permit one non-public TLD**
  (`kiln_pki::kiln_root_tld`). It writes the certificate from the parsed DER into the
  distribution's anchor folder and runs its update tool, with a cleared environment.
  Supported: Debian/Ubuntu, Fedora/RHEL, Arch, openSUSE.
- **`kiln doctor`** (Linux) reports each browser database's trust, `certutil`, the system
  store, where the key is kept, and stale site certificates ([I-57](issues.md#i-57)).
- **`kiln uninstall`** also removes the keyring item, and its plan says so.
- **Fixed on the way:** [I-57](issues.md#i-57), site certificates from an old CA (found in
  the user's check). [I-58](issues.md#i-58), two dependency advisories.
- **Tests:** 13 new on Windows (243 total) and 15 in the VM (213 total), all passing. They
  cover:
  - the NSS operation against a fake `certutil` (undo, created databases, already trusted);
  - finding the databases;
  - the system operation (distribution detection, undo);
  - the helper's certificate check (impostors refused, no paths through the fingerprint);
  - the constrained-root check;
  - stale-certificate clearing;
  - the doctor rules.

  The real keyring test runs by hand in the VM.

**Checked over SSH in the VM:** `kiln ca trust` added the root to Chrome's database and the
Firefox snap profile (#4, #5, no password). Headless Chrome then loaded the site, and after
two `kiln undo`s it rejected it again (`ERR_CERT_AUTHORITY_INVALID`). The I-57 reproduction
and fix are described in the issue.

<a id="p1-04c"></a>
### P1-04 part c delivered (2026-10-23)

Design: [ADR 0022](adr/0022-linux-php-runtimes-and-fpm.md). Checked first:
- static-php-cli (`dl.static-php.dev`) builds run on any distribution, but **publish no
  checksums**;
- the `bulk` set lacks `pdo_sqlite` and `pdo_pgsql` (checked with `php -m`), so Kiln pins the
  `common` set;
- the distribution's PHP is verified by its package manager, but is one version per release.

- **`kiln php install 8.4`** (Linux): the static CLI and FPM builds of 8.2.32, 8.3.32,
  8.4.23 or 8.5.8 for x86-64.
  - Each SHA-256 is pinned in `kiln-php::linux`, recorded from two separate downloads (in
    the VM and on Windows) that agreed for all eight files. Every build ran before pinning
    (`php -v`, `php -m`, `php-fpm -t`).
  - Only the named binary is extracted from each archive, never links or nested paths.
  - `kiln php available` lists the pins and any distribution PHP found.
- **`kiln php install 8.3 --system`** links the distribution's `php8.3` and `php-fpm8.3` (or
  Fedora/Arch's unversioned ones) into `<data>/php/8.3.6-system/` with Kiln's `php.ini`. The
  distribution's extension files still load. `kiln php list` marks it, and removing it
  deletes only the links.
- **PHP-FPM pools:** `pools.json` gains `kind: fpm`.
  - One master per version on one loopback port, with Kiln's `php-fpm.conf`: dynamic,
    `kiln php workers` children, `pm.max_requests = 500`, `clear_env = no`, running as the
    user.
  - The supervisor runs it in the foreground and stops it with SIGTERM (killing it after
    5 s). With `PR_SET_PDEATHSIG` (new `kiln-sys::linux`), it can't outlive the supervisor.
- `pools.json` written before this still reads (`php_cgi` → `program`).
- `kiln web start` reports `PHP 8.4.23: PHP-FPM on 127.0.0.1:<port>`.
- **Tests:** 247 on Windows and 218 in the VM, all passing. New: archive extraction
  (only the top-level binary), finding distribution PHP (Debian and Fedora layouts), pin
  resolution and shape, the FPM config, FPM pool specs and the old `pools.json` format.

**Checked over SSH in the VM:**
- `kiln php install 8.4` downloaded and verified both builds. A linked PHP site was served by
  PHP-FPM from two children, with `pdo_sqlite` loaded.
- `kiln web stop` left no `php-fpm` processes (there were three). After `kill -9` of the
  supervisor, none were left either.
- `kiln php install 8.3`, then `kiln isolate 8.3 phpapp` on the running server: the 8.3 pool
  started beside 8.4, the site moved without a restart, and the 8.4 pool retired after the
  grace period. `unisolate` moved it back.
- With Ubuntu's `php8.3-fpm` installed: `kiln php install 8.3 --system` found 8.3.6. A site
  isolated to it reported PHP 8.3.6 with **intl** (from Ubuntu's packages) and Kiln's
  `php.ini`. Ubuntu's own `php8.3-fpm` service, running as root, kept running untouched.

<a id="p1-04d"></a>
### P1-04 part d delivered (2026-10-23): P1-04 complete

Design: [ADR 0023](adr/0023-linux-shims-and-path.md). Linux has no user `PATH` value: it comes
from shell startup files (`~/.profile` for the desktop and login shells, `~/.bashrc` for
terminals).

- **Shims:** `kiln shims install` copies `kiln-shim` as `php` and `composer` into
  `~/.local/share/kiln/bin/shims`. They are real copies, not links: `/proc/self/exe` resolves
  links. The shim now takes its role from `argv[0]` first. The shim path constants use `/` on
  Linux.
- **`PATH`:** Kiln writes `~/.local/share/kiln/env` (and `env.fish`), which put the shims
  first on `PATH` once.
  - A journaled `env.shell_profile` operation per file adds a marked block sourcing it at the
    end of `~/.profile` (created if missing), `~/.bashrc` and `~/.zshrc` if present, and
    `~/.config/fish/conf.d/kiln.fish` if fish is installed.
  - The block is edited as bytes like the hosts block: other bytes, line endings and mode are
    kept, ambiguous markers are refused, and a file Kiln created is removed on undo.
- `kiln composer install` and `kiln which` work on Linux.
- **Tests:** 251 on Windows and 222 in the VM, all passing. New: block round trips (LF,
  CRLF, no final newline, empty), refused markers, the operation keeping user edits and file
  mode, created files removed, an older block restored, and the new journal description.

**Checked over SSH in the VM** (shims need no password):
- `kiln shims install` showed `php` moving from `/usr/bin/php` to Kiln's shim, and wrote
  blocks to `~/.profile` and `~/.bashrc` (#5, #6).
- In a new terminal, `php` in the project pinned to 8.3 ran 8.3.32, and elsewhere the default
  8.4.23. `composer --version` worked, and `exit(7)` came through as 7.
- A login shell found the shim too. A shell reading both files had the shims folder on
  `PATH` once.
- `kiln doctor` in a new terminal: "`php` on this terminal's PATH is Kiln's shim".
- Two `kiln undo`s left `~/.profile` and `~/.bashrc` **byte-identical** to before (SHA-256),
  and `php` was Ubuntu's again.

### P1-15 delivered (2026-10-26)

[ADR 0024](adr/0024-integration-test-vms.md). Each scenario follows the same steps:

1. Snapshot everything Kiln may touch.
2. Run a full Kiln session.
3. Run `kiln uninstall --yes`.
4. Snapshot again and compare.

Plain files are compared byte for byte. Trust stores and the keyring are compared by their
entries.

- **Ubuntu 24.04** (`tests/vm/ubuntu/run.sh`), passed 2026-10-23: **16 checks; 14 facts
  identical** after uninstall.
  - The session covers PHP 8.4, Caddy, the CA in the keyring, NSS and the system store,
    hosts, PHP-FPM sites over HTTP and HTTPS, and the shell-profile shims.
  - The polkit prompts are allowed by a test-VM-only rule.
- **Windows 11 with Laravel Herd running** (`tests/vm/windows/run.ps1`), passed 2026-10-26:
  **all 20 checks pass; 14 facts identical** after uninstall.
  - **Port:** Kiln moved off port 80, which Herd holds, and said so. HTTP and HTTPS sites
    answered at the URLs it printed, and the HTTPS site was trusted.
  - **Doctor:** `kiln doctor` named Herd.
  - **Shims:** the `php` shim worked in a new terminal.
  - **Herd restart:** `herd restart` left Kiln's server running with the same PID
    ([I-11](issues.md#i-11)), and both tools' sites still answered.
  - **After uninstall:** Herd's site still answered, and its hosts block and settings were
    unchanged.
  - **Prompts:** UAC is set to not prompt (test VM only). Windows' two certificate dialogs
    (add and delete) are clicked by the host with the VM's mouse, at the Yes position the
    scenario reports.
- **Found by these runs:**
  - [I-59](issues.md#i-59): with a locked keyring, uninstall orphaned the CA key's item.
  - [I-60](issues.md#i-60): on Windows, reading `kiln web start`'s output waited forever,
    because the background server inherited `kiln`'s output pipe. Both are fixed.
- **Extended 2026-10-27 to Phase 1's full exit criteria** (plan.md). Both scenarios now
  also cover a new Laravel project made with Kiln's `composer` shim, WordPress, and a site
  pinned to PHP 8.3 next to 8.4. They passed: **Windows 29 checks, Ubuntu 28**, each with
  the machine as it was after uninstall.
  - [I-61](issues.md#i-61): windows.php.net's new `8.4.27-1` rebuild names had broken every
    `kiln php install`.
  - [I-62](issues.md#i-62): WordPress needs `mysqli`. It is now enabled on Windows. On Linux
    it led to Kiln's own static PHP builds ([ADR 0025](adr/0025-own-static-linux-php-builds.md),
    `sameedkun/kiln-php`).
  - Stall detection for downloads ([I-48](issues.md#i-48)) and the real-Caddy test in CI
    were added on the way.
- **VM build:** the Windows test VM now builds unattended ([I-36](issues.md#i-36) fixed).
- **How to run them:** in [dev-setup.md](dev-setup.md#integration-tests-in-vms-p1-15).
  They are manual, not CI.

<a id="phase-2"></a>
## Phase 2: developer tooling (done)

Tasks and exit criteria are in [plan.md](plan.md#phase-2-developer-tooling-windows--linux).
Planned 2026-10-27, with [ADR 0026](adr/0026-kilnd-daemon-and-local-api.md) for the daemon,
its local API and autostart.

| ID | Task | Status | Notes |
|---|---|---|---|
| P2-01 | `kilnd` daemon + versioned local API | **done** 2026-10-27 | [ADR 0026](adr/0026-kilnd-daemon-and-local-api.md); see below. Watchdog, API, `kiln daemon …`, change lock; VM-checked on Windows and Ubuntu |
| P2-02 | Per-user autostart (journaled) | **done** 2026-10-27 | ADR 0026 §6 (amended: XDG entry on Linux); see below. Checked across real reboots on Windows and Ubuntu |
| P2-03 | Automatic intermediate renewal | **done** 2026-10-28 | ADR 0026 §2 (amended: a brief restart, not a live reload); see below. VM-checked on Windows and Ubuntu |
| P2-04 | `kiln.toml` project config + `herd.yml` import | **done** 2026-10-28 | [ADR 0027](adr/0027-project-config-kiln-toml.md); see below. VM-checked on Windows and Ubuntu |
| P2-05 | Node runtimes and shims | **done** 2026-10-28 | [ADR 0028](adr/0028-node-runtimes-and-shims.md); see below. Signature-checked installs; VM-checked on Windows (next to Herd's Node) and Ubuntu |
| P2-06 | PHP patch updates with rollback | **done** 2026-10-29 | [ADR 0029](adr/0029-php-patch-updates-and-rollback.md); see below. A real update and rollback round-trip in both VMs |
| P2-07 | PHP extension toggles | **done** 2026-10-29 | [ADR 0030](adr/0030-php-extension-toggles.md); see below. Journaled one-line toggles, live and undoable; VM-checked on Windows and Ubuntu |
| P2-08 | `kiln new` (Laravel) | **done** 2026-10-30 | [ADR 0031](adr/0031-kiln-new-laravel-projects.md); see below. A Laravel app over HTTPS in one command; VM-checked on Windows and Ubuntu |
| P2-09 | Proxy sites | **done** 2026-10-30 | [ADR 0032](adr/0032-proxy-sites.md); see below. A dev server (Vite and others) at `<name>.test`, with HTTPS and live reload; VM-checked on Windows and Ubuntu |
| P2-10 | Configurable TLD | **done** 2026-10-30 | [ADR 0033](adr/0033-configurable-tld.md); see below. `kiln ca init --tld internal` and the rest of Kiln follows; VM-checked on Windows and Ubuntu |
| P2-11 | VM scenarios for Phase 2 | **done** 2026-10-31 | [ADR 0034](adr/0034-phase-2-vm-scenarios.md); see below. A real reboot inside each scenario, a timed renewal restart; fixed I-73. Exit criteria met on Windows (with Herd) and Ubuntu |

### P2-01 delivered (2026-10-27)

[ADR 0026](adr/0026-kilnd-daemon-and-local-api.md).

- **New crate `kiln-daemon`**, with the `kilnd` binary:
  - **Protocol:** JSON-RPC 2.0, one message per line, lines capped at 1 MiB.
  - **Socket:** a per-user Unix socket, `<local data>/kilnd.sock`.
  - **Server:** a blocking accept loop with one thread per connection.
  - **Client:** what `kiln daemon …` uses.
  - **Events:** `events.subscribe`, delivered as notifications on the same connection.
  - **Methods:** `kiln.hello` (API version 1), `daemon.status`, `daemon.shutdown`,
    `web.status`.
  - **One daemon per user:** `<state>/kilnd.lock`.
  - **Log:** `<local data>/logs/kilnd.log`, rotated at 5 MB with 3 old files kept. On
    Windows the daemon has no console window.
- **Watchdog:** every 15 s it reads the web server's and PHP supervisor's PID files.
  - It repairs only a program that exited without `kiln web stop` (a crash or a reboot): it
    runs `kiln web start`, or `kiln web restart` if only PHP exited.
  - It backs off between repairs (0 s up to 10 min) and resets after 10 healthy minutes.
  - Repairs are logged, counted in `daemon.status` and sent as `web.repaired` events.
- **`kiln daemon start | stop | status | call <method> [json]`.**
  - `stop` leaves the web server running.
  - `kiln uninstall` stops the daemon first.
- **Change lock:** every `kiln` command that can change state now takes `<state>/kiln.lock`.
  - A second command waits and says so; read-only commands don't take the lock.
  - This also closes an older gap where two `kiln` commands could change state at once.
- **`kiln-sys`:** an `AF_UNIX` listener for Windows. Neither the listener nor the
  connections it accepts can be inherited, so a program the daemon starts can't hold its
  socket open (as in I-60).
- **Tests:** 263 on Windows (new: protocol, repair rules, backoff, log rotation, and socket
  tests for calls, errors, a too-long line and events). Clippy and the tests also pass on
  Linux in the VM.
- **VM scenarios gained a `kilnd` section:**
  - start the daemon;
  - force-kill `kiln-caddy` and check the daemon brings it back with a new PID (18 s on
    Windows), with the sites answering again and the restart counted;
  - call the API;
  - check `daemon stop` leaves the web server running;
  - check `uninstall` stops a running daemon.

  **Windows passed 37 checks, Ubuntu 35**, each leaving the machine as it was.
- **On the way:** the WordPress checks now open setup step 1 directly. Step 0 fetches the
  language list from api.wordpress.org, and a slow answer ran past PHP's 30 s limit in the
  VM ("critical error").

### P2-02 delivered (2026-10-27)

[ADR 0026](adr/0026-kilnd-daemon-and-local-api.md) §6, amended.

- **`kiln daemon enable`:** a journaled `daemon.autostart` operation, which starts the
  daemon now too.
  - **Windows:** the `Kiln` value under `HKCU\…\CurrentVersion\Run`.
  - **Linux:** `~/.config/autostart/kiln-kilnd.desktop`.
  - **Undo:** `kiln daemon disable`, `kiln undo` and `kiln uninstall` restore exactly what
    was there. That means an entry Kiln replaced, or nothing. The `autostart` folder goes
    too if Kiln created it and it's empty, and an entry edited by hand since is left alone
    unless forced.
  - `kiln daemon status` shows "Starts at login".
- **Linux uses an XDG autostart entry, not a systemd user unit** (ADR amended).
  - The desktop session starts it after login, when the keyring that renewal (P2-03)
    needs is unlocked.
  - It's one file with an exact undo.
- **No special startup mode:** after a reboot, the web server's PID files are stale, so
  `kilnd`'s first watchdog pass runs `kiln web start`. A server the user had stopped stays
  stopped.
- **Found on the way:**
  - [I-64](issues.md#i-64): a hard reset right after enabling lost the `Run` value. Windows
    writes the registry lazily. `kiln-sys` now flushes `HKCU` keys after each write or
    delete, which covers the user PATH too.
  - [I-63](issues.md#i-63): piping `kiln` into `head` panicked with "Broken pipe". It now
    exits quietly with code 141.
- **Tests:** the operation against an in-memory login item and a real file (replace and
  restore, hand edits, the folder Kiln created); registry value delete.
- **Checked in the VMs:**
  - Scenarios: `enable`, `disable`, `enable` again, then uninstall. The snapshots list the
    `Run` values and the `autostart` folder. **Windows 39 checks, Ubuntu 37**, each leaving
    the machine as it was.
  - **Real reboots** (`tests/vm/windows/autostart.ps1`, `tests/vm/ubuntu/autostart.sh`): a
    site served, `kiln daemon enable`, reboot with automatic login. Each time, `kilnd` was
    running again, had restarted the web server within seconds of login, and the site
    answered.
  - That held on Windows after a graceful reboot and after a hard reset (`-HardReset`), and
    on Ubuntu after a reboot.

### P2-03 delivered (2026-10-28)

[ADR 0026](adr/0026-kilnd-daemon-and-local-api.md) §2, amended.

- **`kiln ca renew [--force]`:**
  - With fewer than 10 days left (or with `--force`), it issues a new intermediate.
    Otherwise it says how many days are left.
  - A running server with HTTPS sites then restarts, in under a second, to use it.
- **Restart, not live reload (ADR amended):** Caddy keeps the old site certificates in
  memory across a reload ([I-57](issues.md#i-57)), so the server restarts instead. That is
  about once a month.
- **`kilnd`:**
  - It checks the intermediate every 6 hours and once at start, reading the certificate
    only, so the CA key stays locked until a renewal is due. When one is, it runs
    `kiln ca renew`.
  - Its output goes to the log, and it sends `ca.renewed` or `ca.renewal_failed`. A failure,
    such as a locked keyring, is retried at the next check.
  - New API methods: `ca.status`, and `ca.renew` with an optional `force`.
  - `kiln daemon status` shows the days left and the renewals made.
- **Plan wording:** the Phase 2 exit criterion now says HTTPS keeps working across a renewal
  "without anything from the user", not "with no restart".
- **Tests:** the due rule (fewer than 10 days, including none and expired). 269 on Windows.
- **VM scenarios:**
  - `daemon status` shows the days left.
  - A renewal forced through the API gives a new intermediate fingerprint and a restarted
    server, and the PHP site still answers over trusted HTTPS.
  - The renewal is counted.

  **Windows passed 42 checks, Ubuntu 40**, each leaving the machine as it was.
- **On the way:**
  - `composer create-project` in the scenarios retries once, after a dropped download in
    the VM (Composer exit code 100).
  - A PowerShell mistake in the new check was fixed: `Kiln ca info -split …` passed
    `-split` to `kiln`.

### P2-04 delivered (2026-10-28)

[ADR 0027](adr/0027-project-config-kiln-toml.md).

- **`kiln.toml` in the project folder:**
  - `[site]` `name`, `secure` and `aliases`; `[php] version`; `[node] version` (used by
    P2-05).
  - It is read live each time sites are resolved, never copied into Kiln's state, so
    editing it and running `kiln web reload` (or any site change) is enough.
  - Versions must be quoted: an unquoted `8.10` would read as `8.1`, so it is an error.
    Unknown keys and sections are warnings, not errors.
- **Precedence:** a `kiln isolate` pin beats `kiln.toml`, which beats the default PHP.
  `kiln which` and the shims say when the version comes from `kiln.toml`. `kiln isolate`
  notes when it overrides the file.
- **`secure = true`** serves the site over HTTPS. `kiln unsecure` says when `kiln.toml`
  keeps it on.
- **Aliases** become extra site names served from the same folder, with the same PHP.
  `kiln sites` lists them as `alias`. A name that is taken or invalid is skipped with a
  warning.
- **`kiln link`** takes its default name from `[site] name`.
- **`kiln doctor`** reports problems in a project's `kiln.toml`: a parse error or a skipped
  alias, with the fix.
- **`kiln import herd [path] [--force]`:**
  - It reads Laravel Herd's `herd.yml` (name, PHP version, `secured`, aliases) and writes
    `kiln.toml`, which must parse back before it is kept. It won't overwrite an existing
    `kiln.toml` without `--force`.
  - It notes what it doesn't import: services, workspaces, an unquoted PHP number.
  - It links the folder if Kiln doesn't serve it yet, else reloads.
- **New dependencies:** `toml` 1.x (parse only) and `yaml-rust2` 0.13, both MIT/Apache-2.0.
- **Fixed I-65:** PHP's limits are now the same on both systems: 512 MB of memory, 60 s per
  request, 64 MB uploads. Windows had PHP's 30 s default, and WordPress's first page timed
  out in the VM.
- **Tests:** the parser (quoting, unknown keys, Node `v` prefix), the `herd.yml` import,
  aliases and precedence. 279 on Windows, 248 on Linux.
- **VM scenarios:**
  - A parked project whose `kiln.toml` asks for PHP 8.3 and an alias: both names get 8.3,
    `kiln which` names `kiln.toml`, and the `php` shim in that folder runs 8.3.
  - A project with a `herd.yml`, imported: its name and alias are served over trusted
    HTTPS on PHP 8.3.

  **Windows passed 46 checks, Ubuntu 47**, each leaving the machine as it was.
- **On the way:**
  - Right after a renewal restart, Caddy issues site certificates in the background, and
    a handshake in the first second or two can fail. The scenarios retry for up to 20 s
    (noted in ADR 0026).
  - A Windows run timed out when Composer's downloads stalled in the VM; the rerun passed.

### P2-05 delivered (2026-10-28)

[ADR 0028](adr/0028-node-runtimes-and-shims.md).

- **`kiln node available | install | list | remove | default`:**
  - Builds come from nodejs.org: `win-x64.zip`, `linux-x64` / `linux-arm64.tar.gz`. Node 18
    and later.
  - Specs: `22`, `22.11`, `22.11.0` (with or without `v`), `lts`, `lts/jod`, `latest`.
  - Installs go to `<local data>/node/<version>/`, with an in-progress marker until
    `node --version` checks out, as for PHP.
- **Signature-checked:**
  - `SHASUMS256.txt.sig` is verified against Node's release keys, copied verbatim from
    `nodejs/release-keys` into `crates/kiln-node/keys/` (29 files, current and former
    releasers). Only then is the archive's SHA-256 taken from it.
  - A small OpenPGP reader in the new `kiln-node` crate handles v4 RSA and Ed25519 keys,
    subkeys included. `ring`, already in the tree, does the arithmetic: no new crates.
  - A key Kiln doesn't know is refused, with its fingerprint and "update Kiln".
- **Which Node runs in a folder:** the nearest `kiln.toml` `[node] version`, `.nvmrc` or
  `.node-version` up the tree, else `kiln node default`, else the newest installed. A missing
  version falls back to the default, with a note on stderr. `kiln which` shows the line and
  why.
- **Shims: `node`, `npm`, `npx`, `corepack`** are more copies of `kiln-shim`.
  - npm, npx and corepack run `node <bundled script>`, with no `.cmd` file and no
    `cmd.exe`. The chosen version's folder goes first on the child's `PATH`.
  - **While Kiln has no Node, they step aside** to the next `node` on `PATH`, so
    `kiln shims install` never breaks an existing Node.
  - **Windows:** a Node on the machine `PATH` stays first. `kiln shims install` and
    `kiln doctor` say so, and Kiln doesn't change the machine `PATH`.
- **`kiln-fetch`** gained `get_bytes`, for the binary signature.
- **Tests:**
  - Real release signatures: RSA (v20.20.2), Ed25519 (v22.23.4) and an Ed25519 subkey
    (v24.12.0). Tampered data and signatures fail; garbage is refused without panics.
  - Every key file parses to the fingerprint in its name.
  - Spec parsing; the index; zip and tar extraction, refusing escaping links; the
    version-file search; the shims' commands and `PATH` search; doctor's Node findings.

  301 on Windows.
- **Checked by hand** in an isolated folder: real installs of 22 and 20 from nodejs.org,
  and the shims, including the step-aside to an existing Node.
- **VM scenarios:**
  - Node 22 and `lts/iron` installed and signature-checked.
  - `node` follows `kiln.toml` (20), `.nvmrc` (`lts/iron`) and the default (22).
  - `kiln which` names the `.nvmrc`.
  - `npm install` and `npm run build` build the new Laravel app's assets with Kiln's Node.
  - **Windows, next to Herd:** Herd's Node is on the machine `PATH`, and stays first in new
    terminals; `kiln shims install` and `kiln doctor` say so. Kiln's shims, run directly,
    choose the versions above.

  **Windows passed 52 checks, Ubuntu 53**, each leaving the machine as it was.
- **On the way:**
  - The first Windows run lost DNS for nodejs.org for a moment ("No such host is known");
    the rerun passed.
  - The Ubuntu scenario piped `kiln node install` into `grep -q`, which stops reading at its
    match. Kiln then stopped, by design (I-63), before installing. The scenario now reads the
    whole output first, and the `kiln import herd` check too.

### P2-06 delivered (2026-10-29)

[ADR 0029](adr/0029-php-patch-updates-and-rollback.md).

- **The previous patch of each line is pinned**, so it can be installed again
  (`kiln php install 8.4.26`), and an update can be tested for real:
  - **Linux:** `sameedkun/kiln-php` now builds exact patches on request (a `versions` input).
    Its new release `2026-10-29` holds 8.2.34, 8.3.35, 8.4.26 and 8.5.11. Each pin now names
    its release.
    - Checked as before: the release's `.sha256` files, a download on Windows and one in the
      Ubuntu VM agree.
    - Each build ran there: `php -v`; `php -m` with mysqli, pdo_sqlite, intl, sodium and
      OPcache; `php-fpm -t`.
  - **Windows:** SHA-256s for windows.php.net's archived builds of the same patches. They
    matched from both of its addresses, and each ran (`php -v`). Older patches are still
    refused.
- **`kiln php update [minor]`:**
  - It installs the newest patch of each installed line (or the one named) beside the
    current one: from the release index on Windows, the newest pin on Linux. A
    distribution's PHP is skipped.
  - An exact `php.default` moves with it. Lines pinned as `8.4` follow by themselves; exact
    pins stay, and it says so.
  - A running server switches pools live.
  - It records the patch it replaced, and names older patches nothing uses any more.
- **`kiln php rollback [minor]`:**
  - It goes back to the recorded patch: the default moves back first, then the newer patch
    is taken out of use, the pools switch live, and its folder is deleted. A file held by
    an exiting worker is retried, else the folder is marked incomplete.
  - It refuses while a site is pinned exactly to the newer patch.
  - The download stays cached, so updating again is offline.
- **Not journaled,** like installs: it all lives in Kiln's folder and state.
- **`kiln-state`** gained `remove_setting`.
- **Tests:** the pinned previous patches resolve (Windows `archives/` URLs, Linux release
  URLs), a minor resolves to its newest pin, and rollback picks the recorded line. 304 on
  Windows.
- **VM scenarios:**
  - Both start from 8.4.26 and serve a page showing its PHP version.
  - `kiln php update 8.4` switches it live to the newest patch, keeping 8.4.26.
  - `kiln php rollback` switches it back live and removes the newer patch. A second rollback
    has nothing to do.

  **Windows passed 56 checks, Ubuntu 57**, each leaving the machine as it was.
- **On the way:**
  - One kiln-php build failed downloading a library through the GitHub API, likely its rate
    limit. A re-run passed, and the workflow now passes its token to static-php-cli.
  - The Ubuntu VM's address changed after a reboot. Its host key matched before the new
    address was trusted.

### P2-07 delivered (2026-10-29)

[ADR 0030](adr/0030-php-extension-toggles.md).

- **`kiln php ext list|on|off [--php X]`:**
  - It works on the default PHP, or on the build `--php` names.
  - **What's loaded:** PHP itself says, with `php -c <its php.ini> -m`.
  - **What `list` shows:** the extensions that can be turned on or off, those built in, and
    those a distribution's PHP loads from `/etc`. It also prints the `php.ini` path.
  - **Names:** any case; `php_xsl.dll` and `xsl.so` both mean `xsl`.
- **One journaled line per toggle** (`php.ini.extension`, new `PhpExtOp` in
  `kiln-platform`):
  - **On** appends `extension=` (`zend_extension=` for OPcache and Xdebug).
  - **Off** marks every active line as `;[kiln off] …`.
  - **Undo** takes back just that line, keeping the user's later edits.
  - `kiln history` shows "PHP 8.4.26: xsl turned on".
- **Checked and live:**
  - **After `on`,** `php -m` must list the extension. If it doesn't, the toggle is undone
    and PHP's start-up warning is shown.
  - **Running workers of that build restart:** a new pool starts, Caddy switches to it, and
    the old one retires.
  - **`kiln undo` of a toggle** restarts them the same way.
- **Lifecycle:**
  - `kiln php update` carries the toggles in effect over to the new patch.
  - `rollback` and `remove` undo the toggles of the folder they delete, so the journal never
    points at a file that's gone.
- **Linux:**
  - **Kiln's static builds** have everything compiled in. `list` says so, and `on`/`off`
    are refused.
  - **A system PHP:** Kiln can turn on a `.so` that isn't loaded. For one its `conf.d`
    loads, Kiln names `sudo phpdismod`. Kiln never edits `/etc`.
- **Tests:**
  - **The edit:** on/off and undo are byte-exact, including CRLF files and a missing final
    newline.
  - **Undo:** works out of order, and after the user edits the file.
  - **The rest:** `php -m` parsing and the toggle/built-in/distribution split.
  - 314 tests on Windows; clippy is also clean on Linux.
- **VM scenarios:**
  - **Windows:**
    - `xsl` on: live in the site and the `php` shim.
    - `curl` off: the site loses it, and `kiln undo` brings it back.
    - Unknown and built-in names are refused.
    - `xsl` carried through `php update`.
    - Still on after `php rollback`.
    - Finally undone, leaving `php.ini` byte-identical.
  - **Ubuntu:**
    - The static build lists everything as built in.
    - `off` is refused, and `php.ini` is unchanged.
    - The VM's own `php8.3` (`--system`) lists the distribution's modules and names
      `phpdismod` for `sqlite3`.

  **Windows passed 61 checks, Ubuntu 60**, each leaving the machine as it was.
- **On the way:**
  - **A transient DNS failure** for windows.php.net in the Windows VM. A re-run passed.
  - **One `npm run build` failure** (Vite/rolldown) in the Ubuntu VM. A re-run passed.
  - **A bulk edit broke two Linux-only lines.** The first Ubuntu run used the previous
    binary as a result. Caught there, then clippy and the tests were run in the VM.
- **Not covered by a VM:** turning a system PHP's unloaded `.so` on. Every module the VM's
  distribution installed is already enabled. Unit tests cover the edit.

### P2-08 delivered (2026-10-30)

[ADR 0031](adr/0031-kiln-new-laravel-projects.md).

- **`kiln new <name> [--path <folder>] [--php <version>] [--secure]`:**
  - **Creates the app:** `composer create-project laravel/laravel <name>` in the folder,
    with Kiln's PHP (the one `--php` names, else the default) and Kiln's Composer.
    - Composer is installed first if it's missing, verified as `kiln composer install`
      does.
    - That PHP's folder goes first on `PATH` for Composer's scripts.
    - The skeleton's own scripts write `.env` and `APP_KEY` and create the SQLite database.
  - **Serves it:**
    - **Linked** as `<name>.test`, with HTTPS if `--secure`, and the URL is printed.
    - **In a parked folder** it's already served, so Kiln only secures it if asked.
  - **`--php`** is also written to the project's `kiln.toml`, so the site and the shims keep
    that version.
  - **Checked first:**
    - The name must be a single site label.
    - An existing folder is refused before anything is downloaded.
  - **On failure:** Composer's exit code is passed on, any partial folder is named, and
    nothing is linked or deleted.
- **I-66 fixed on the way:** since 2026-10-29, nodejs.org's `index.json` begins with a Node
  27 alpha. Kiln refused the whole index, so `kiln node install` failed for everyone.
  Pre-releases are now skipped, with a test using the real entry.
- **Tests:**
  - Project names, Composer's arguments, `kiln.toml` round-trips, and `PATH` order.
  - The node index with the alpha entry.
  - 319 on Windows; clippy and tests are also clean on Linux.
- **VM scenarios:**
  - **Both:** `kiln new shop --secure --php 8.3`, outside the parked folder. Then `hosts
    sync`; the Laravel page answers over HTTPS; `kiln which` in the project names
    `kiln.toml` and PHP 8.3. A second `kiln new shop` is refused.
  - **Windows passed 63 checks, Ubuntu 62**, each leaving the machine as it was.
- **On the way:** the Ubuntu VM's address changed again. Its host key matched the known one
  before the new address was added.

### P2-09 delivered (2026-10-30)

[ADR 0032](adr/0032-proxy-sites.md).

- **`kiln proxy <name> <target> [--secure]`:** serves `<name>.test` from a local dev server
  (Vite, Next, a Node or Go server) through Kiln's Caddy.
  - **Target:** a port (`5173`), `host:port`, or `http(s)://host:port`. It must be on
    this machine (`localhost`, a `127.x` address or `[::1]`), so a `.test` name never
    forwards to another machine.
  - **Forwarding:**
    - The dev server sees its own host (`localhost:5173`) in `Host`, which Vite's host
      check accepts. The site's name arrives in `X-Forwarded-Host`.
    - WebSocket upgrades pass through, so live reload keeps working.
    - An HTTPS target is reached without checking its certificate, since it's on loopback.
  - **HTTPS** with `--secure`, the same way as for folder sites; `kiln secure` and `kiln
    unsecure` work on proxies too.
  - **Names:** a proxy takes precedence over a parked subfolder with the same name, and
    says so. A linked site's name is refused: unlink it first.
  - **Listed** in `kiln sites` with its target. `kiln hosts sync` covers it.
- **`kiln unproxy <name>`** removes it.
- **Stored in its own table** (state schema v6). Like links, proxies are Kiln's own
  settings, not journaled system changes.
- **I-67 fixed on the way:** replacing php.ini on Windows failed once with "Access is
  denied" while another process briefly held it. Kiln's rename now retries for up to about
  two seconds.
- **I-69 fixed on the way:** right after `--secure`, Caddy refused HTTPS for a moment while it
  issued the certificate in the background. `kiln` now waits for the certificate, up to 10 s,
  before it reports the change done. This affects `secure`, `link`, `new` and `proxy`.
- **I-68 (test harness):** in the Windows VM, the firewall prompt Vite set off covered the
  certificate dialog during uninstall. The test VM now has that prompt turned off. Kiln
  doesn't touch firewall rules.
- **Tests:**
  - Target parsing (loopback only, no paths), the Caddy route, and the proxy table.
  - Against a real Caddy: the dev server's `Host` and the forwarded headers, a WebSocket
    echo, and 502 once the dev server is gone.
  - 325 on Windows; clippy and tests are also clean on Linux.
- **VM scenarios:**
  - **Both:** `npm run dev` in the Laravel app starts Vite. `kiln proxy vite 5173
    --secure`, then `hosts sync`: Vite's HMR client answers over HTTPS at `vite.test`, and
    `kiln sites` lists the target. After `kiln unproxy vite` the name is no longer served.
  - **Windows passed 66 checks, Ubuntu 65**, each leaving the machine as it was.
  - Both also retry the Laravel build once and give Node longer to reach Bunny Fonts (I-70,
    a slow route, not Kiln). Windows' passing run came before that change to its build step;
    Ubuntu's ran with it.
- **Not done:** while the dev server is stopped, the name answers 502. Kiln doesn't start
  or watch dev servers.

### P2-10 delivered (2026-10-30)

[ADR 0033](adr/0033-configurable-tld.md), amending [ADR 0010](adr/0010-local-certificate-authority.md).

- **`kiln ca init --tld <name>`** chooses the development TLD when it creates the root.
  - **Allowed names:** `test` (the default), `internal`, `example` or `invalid`, the names
    set aside so that they never resolve publicly. Any other name is refused.
  - **Recorded** as the `tld` setting. Every command and the shims read it:
    - site names in `link`, `proxy`, `new`, `secure` and `isolate`;
    - parked folders and `hosts sync`;
    - the defaults of `hosts set`, `hosts clear` and `plan hosts`.
  - **The root decides:**
    - `ca init` records the existing root's own TLD.
    - Intermediates are always issued for the root's TLD, at `ca init`, at `ca renew` and
      at a start with a stale intermediate.
    - Another TLD over an existing root is refused, since it needs a new root. For now the
      way is `kiln uninstall`, then `ca init --tld`.
    - Links or proxies under another TLD are listed and refused when the root is created.
  - **Reading back:** `Tld::recorded` reads names that the earlier rules allowed (such as
    `wip`), so `kiln undo` and `kiln ca trust` still work for what Kiln made then.
- **I-71 fixed on the way:**
  - Serving a `.internal` site made Caddy set up its own default CA and install its root
    into the system and NSS trust stores. The test VM's passwordless sudo let that happen
    silently.
  - Kiln's Caddy config now declares that CA with `install_trust: false`.
  - Found by the new stage's before/after comparison.
- **I-69 extended:** `kiln web start` now also waits for new site certificates before it
  prints the URLs.
- **I-70:** the scenarios now give Node 10 s to reach Bunny Fonts. 2 s still failed once.
- **I-72 fixed:** the first `kiln web start` never recorded which intermediate issued the
  site certificates. So the first `link`, `secure` or `kiln new` after it took that for a
  changed CA and restarted the server. Now the record is written whenever Caddy gets the CA.
  A Windows run's `kiln new` also stalled once before linking, for reasons not found.
- **Tests:**
  - The allowed and recorded TLD rules, and the setting with a legacy or broken value.
  - Project names under another TLD, and Caddy's `local` CA without trust installation.
  - 327 on Windows, 297 on Linux; clippy clean on both.
- **VM scenarios:**
  - **Both:**
    - `ca init` refuses `--tld lan` (not reserved), and `--tld internal` over the `.test`
      root.
    - A new stage after the uninstall comparison: a fresh `kiln ca init --tld internal`,
      then a static site linked as `static-site.internal`, `hosts sync`, and the site
      served. Then `kiln uninstall` again, and the machine is compared with the start once
      more.
  - **Ubuntu:** the `.internal` site over HTTPS, checked against the new root. 72 checks
    passed.
  - **Windows:** the `.internal` site over HTTP, since trusting a root needs a dialog. 71
    checks passed.
  - **Windows `kiln new`:** it now runs with a 20-minute limit and logs where it stopped
    (I-72).
- **On the way:** the Ubuntu VM got a new address after a host restart. Its host key matched
  the known one before the new address was added.

### P2-11 delivered (2026-10-31)

[ADR 0034](adr/0034-phase-2-vm-scenarios.md), extending
[ADR 0024](adr/0024-integration-test-vms.md). **Phase 2's exit criteria are met in both
VMs.**

- **A reboot inside each scenario:**
  - The session part ends at `kiln daemon enable`, with `kilnd` left running.
  - The runner reboots the VM and waits for the automatic login.
  - The after-reboot part checks what the login brought back, then uninstalls and
    compares with the very first snapshot.
  - Shared helpers moved to `lib.ps1` and `lib.sh`. The Ubuntu work folder moved from
    `/tmp` to `~/kiln-test`, since a reboot can clear `/tmp`.
- **I-73 fixed:** the restart after a CA change left sites unanswered for 8.1 s on Windows,
  found by the new timed check.
  - It now restarts only Caddy (`WebServer::restart`). The PHP workers keep running, and
    Caddy validates the new config while the old server still serves.
  - Caddy's log shows about 0.12 s between the two servers. A client on Windows sees about
    0.6 s, because Windows retries a refused connection.
- **Tests:**
  - A real-Caddy test of `restart`, and of a rejected config leaving the server running.
  - 328 on Windows, 298 on Linux; clippy clean on both.
- **Exit criteria, and the checks that cover them** (both VMs unless noted):

  | Criterion | Checks |
  |---|---|
  | `kiln.toml` pins PHP and Node for `php`, `node`, `npm` and the site | `toml-app`: its site and the `php` shim on PHP 8.3; the `node` shim on its Node 20 (22 is the default); **new:** `npm version` names Node 20 there and 22 in `laravel-app` |
  | `herd.yml` imported to the same result | `kiln import herd`: HTTPS, PHP 8.3 and the alias, in the site and the shim. `herd.yml` has no Node setting |
  | `kilnd` starts at login | **new:** a real reboot; `kilnd` running, started by the login item; the web server and PHP back as new processes; the sites over trusted HTTPS; `daemon status` |
  | Its versioned API | `daemon call web.status`; the renewal through `daemon call ca.renew` |
  | HTTPS across a renewal forced through the API, a restart under a second | a new intermediate, a restarted server, HTTPS on the new chain; **new:** the longest time without an answer, measured |
  | `php update` then `php rollback` round-trips | a real update to the newest patch and back, live in a site |
  | Extensions on and off, undoable | Windows: `xsl` on and `curl` off, live, then undone byte for byte. Ubuntu: Kiln's static builds refuse, with `php.ini` unchanged, and a system PHP's are left to `phpdismod` |
  | `kiln new` over HTTPS | `kiln new shop --secure --php 8.3` |
  | A proxy serves a dev server | Vite over HTTPS at `vite.test`, with its HMR client; `unproxy` |
  | Uninstall leaves the machine as it was, autostart, Node and shims included | the comparison after a session whose `kilnd` the login had started, and again after the `.internal` stage |

- **VM scenarios:**
  - **Windows (with Herd):** 80 checks passed. The renewal's restart left sites without an
    answer for 672 ms. After the reboot the scenario opens Herd's app, as its user would,
    since the test VM's Herd has no login item.
  - **Ubuntu:** 80 checks passed. The renewal's restart left sites without an answer for
    66 ms (115 ms in an earlier run).
- **On the way:**
  - The first runs checked too early after the login: Windows starts the `Run` key's
    programs a while after the logon. Each after-reboot check now waits for its condition,
    up to a limit.
  - The Ubuntu keyring lists its items in another order after a login, so the snapshot now
    sorts them, keeping the unlocked and locked counts. The five items it lists are the
    orphans from earlier runs (open item), the same before and after.
  - The P2-02 reboot scripts (`autostart.ps1`, `autostart.sh`) stay, for quick checks and
    `-HardReset`.

## Phase 3: services, Windows first (in progress)

Tasks and exit criteria are in [plan.md](plan.md#phase-3-services-windows--linux). Planned
2026-10-31, with [ADR 0035](adr/0035-services.md) for instances, providers, ports, data and
`.env` wiring.

- **Scope:** Windows and Linux. macOS moved out to Phase 4b
  ([ADR 0007](adr/0007-platform-delivery-order.md), amended): there's no Apple hardware to
  test on, and no Apple Developer account for notarization yet.
- **Binary sources,** checked 2026-10-31:
  - MySQL and MariaDB come from the vendor on both platforms.
  - PostgreSQL has no Linux binaries since version 10, so Kiln builds its own (P3-05).
  - Valkey has no Windows build, so a spike (P3-02) checks Garnet.

| ID | Task | Status | Notes |
|---|---|---|---|
| P3-01 | Service framework, instances, second supervisor; Mailpit | **done** 2026-10-31 | [ADR 0036](adr/0036-service-framework.md); see below. VM-checked on Windows and Ubuntu |
| P3-02 | Spike: a Redis-compatible cache on Windows (Garnet) | **done** 2026-11-01 | [spike 03](spikes/results/03-windows-cache.md); see below. Garnet once it fixes Lua ignoring `SELECT` (I-75) |
| P3-03 | Ports and coexistence for services | **done** 2026-11-01 | [ADR 0037](adr/0037-service-ports-and-coexistence.md); see below. VM-checked on Windows and Ubuntu |
| P3-04 | MySQL and MariaDB | **done** 2026-11-02 (Windows) | [ADR 0038](adr/0038-mysql-and-mariadb.md); see below. VM-checked on Windows; Linux built and unit-tested, its VM run stopped (Linux on hold) |
| P3-05 | PostgreSQL on Windows (EDB) | **done** | ADR 0040; 18.6 and 17.11; services run without admin rights; CI scenario 24 checks |
| P3-06 | Redis on Windows | **done** | ADR 0041; redis-windows MSYS2 builds, 8.10.3 and 8.8.3; CI scenario 31 checks |
| P3-07 | Mailpit's UI at `mail.test`, `MAIL_*` wiring | next | the provider came with P3-01 |
| P3-08 | `.env` wiring with byte-exact undo | planned | ADR 0035 §9 |
| P3-09 | `[services]` in `kiln.toml`, `herd.yml` services | planned | |
| P3-10 | `kilnd` and services | planned | |
| P3-11 | Data safety: dump/restore, `--delete-data`/`--keep-data` | planned | ADR 0035 §7 |
| P3-12 | Phase 3 exit run (CI scenario + by-hand Herd check) | planned | ADR 0039 |
| P3-13 | Windows scenario on GitHub Actions | **done** | ADR 0039 §3; `scenario.yml`, `tests/ci/windows-services.ps1`; 17 checks + before/after, green in about 5 minutes |
| P3-14 | Meilisearch, RustFS, MongoDB | **done** | ADR 0042; mongosh as a companion; CI scenario 45 checks |
| P3-15 | phpMyAdmin and Adminer as Kiln sites | **done** | ADR 0043; `kiln tool add`; CI scenario 52 checks |
| P3-16 | Laravel Reverb per project | planned | |

### P3-01 delivered (2026-10-31)

[ADR 0036](adr/0036-service-framework.md), within [ADR 0035](adr/0035-services.md).

- **`kiln service add <kind> [--version] [--name] [--port role=N]`:**
  - downloads the pinned release and checks its SHA-256;
  - installs the executable under a Kiln name (`kiln-mailpit`);
  - sets up the data folder, records the instance (schema v7, `services` table) and starts
    it.

  Also `kiln service list | start | stop | restart | logs | remove`, and `kiln services up |
  down`.
- **A second `kiln-supervisor`** (`<local data>/services/supervisor`):
  - **One binary:** the same as for PHP, which now runs programs too, with their own
    arguments, environment, working folder and log.
  - **Ports:** a program can open several; start-up waits until all of them listen.
  - **Graceful stop:** a program's stop command first, then a wait, then SIGTERM on Linux,
    and only then a kill. The supervisor itself gets 45 s to stop, so it never kills a
    database mid-shutdown.
  - **Independent of the web stack:** `kiln web stop` doesn't touch it.
- **`kiln-services`,** a new crate:
  - the `ServiceProvider` trait, pinned releases and their install;
  - **contract tests** for every provider: unique kinds, port roles with usual numbers,
    well-formed pins, Kiln-named executables, and command lines that name only loopback
    addresses and use every port given.
- **Mailpit 1.31.6,** the first provider:
  - SMTP on 1025 and its UI and API on 8025, loopback only, with `--disable-version-check`;
  - pinned from GitHub's asset digests, re-checked by hand.
- **Ports:** the usual port if free and not another instance's, else the next free one
  above it, recorded with the instance. Naming who holds it comes with P3-03.
- **Data from the start:**
  - `kiln service remove` keeps the data folder unless `--delete-data`, and says where it
    is and how large.
  - `kiln uninstall` lists the services' data in its plan, and refuses before changing
    anything while there is some, unless `--delete-data` (or `--keep-files`). Its flags
    are now one `Options` struct.
- **Tests:** the supervisor's program pools, the `services` table, the installer (digest
  mismatch, missing file), the contract tests, Mailpit's command line, and port choice
  and parsing. 341 on Windows, 311 on Linux; clippy clean on both.
- **VM scenarios,** both, in a new "Services" stage:
  - `service add mailpit`, a mail sent over SMTP and caught (Mailpit's API), `service
    list`;
  - `stop` closing its ports and `start` bringing it back with its mail;
  - `restart` with a new process and the same data, and `logs`.

  After the reboot, uninstall refuses without `--delete-data` and changes nothing; then
  `--delete-data`, and the machine compares clean. **Windows 87 checks, Ubuntu 87.** The
  renewal restart left sites without an answer for 882 ms on Windows and 120 ms on Ubuntu.
- **On the way:** [I-74](issues.md#i-74), downloads that stall mid-file. Two Ubuntu runs
  failed on GitHub stalling the PHP 8.3 download. It worked again within the hour, so a
  retry with resume is noted for later.

### P3-02 delivered (2026-11-01)

[Spike 03](spikes/03-windows-cache.md) ([results](spikes/results/03-windows-cache.md)),
amending [ADR 0035](adr/0035-services.md) §8.

- **The check:** a Laravel artisan command with 22 checks of everything Laravel uses Redis
  for, over `predis`:
  - cache operations, locks, tags and sessions;
  - the queue (immediate, delayed, released and failed jobs);
  - the rate limiter, `Redis::throttle` and `funnel`;
  - pipelines, transactions and SCAN.

  The scripts are in `spikes/windows-cache/`.
- **Control:** Valkey 9.0.7 in the Ubuntu VM passed 22 of 22. Its first run caught three
  mistakes in the checks, which were fixed before Garnet was judged.
- **Garnet 2.2.3 on Windows** passed 19 of 22.
  - `Cache::add`, tag flushes and the rate limiter fail, because Garnet's Lua scripts
    ignore `SELECT` and act on database 0. Laravel's cache uses database 1.
  - Upstream issue: [microsoft/garnet#2258](https://github.com/microsoft/garnet/issues/2258),
    with its fix open in [microsoft/garnet#2311](https://github.com/microsoft/garnet/pull/2311).
  - With the cache in database 0, all 22 pass. But then `cache:clear` would delete queued
    jobs and sessions, so that isn't done.
- **Also found:** Garnet's Windows build needs .NET 10 and doesn't ship it.
  - With Microsoft's runtime zip unpacked privately and `DOTNET_ROOT` set for the
    process, it runs, and nothing is installed system-wide.
  - Size: 80 MB plus 77 MB of runtime.
  - It runs fine as `kiln-garnet.exe`, listens only on `127.0.0.1`, and takes 2.4–2.8 s to
    start.
- **Decision:** Garnet becomes Windows' cache once a release with the fix passes the spike
  unchanged ([I-75](issues.md#i-75)). Until then, Windows has no cache service and Kiln
  says so. Linux uses Valkey (P3-06).

### P3-03 delivered (2026-11-01)

[ADR 0037](adr/0037-service-ports-and-coexistence.md), extending
[ADR 0019](adr/0019-coexistence.md).

- **Who holds a port:** `kiln-coexist` now recognizes service programs by name: MySQL,
  MariaDB, PostgreSQL, Redis, Valkey, Memurai, Garnet, Mailpit, MailHog, Docker's port
  forwarders and WSL's relay. A busy usual port then reads, for example:
  - "Port 1025 (SMTP, where apps send mail) is used by Kiln's mailpit-1; using 1026.";
  - "… is used by XAMPP (mysqld.exe)".

  When the holder can't be inspected (a system service running as another user), Kiln
  names a running system service of that kind.
- **Conflicts at start:** before starting, each port of each instance that should run must
  be free or held by Kiln's own program. Otherwise the command stops, says who holds the
  port, and offers both ways out.
- **`kiln service port <name> <role>=<port>`** moves an instance. A running one restarts
  there, and its data stays.
- **Stop means stopped:** `stop`, `remove` and `restart` now wait until the stopped
  program's ports are released, up to the supervisor's 45 s.
  - The first Windows run showed `stop` returning while Mailpit was still exiting. A
    program that grabbed the port right after was refused, and `remove --delete-data` met
    files still in use. `remove` also retries the delete for 5 s.
- **"Running" means Kiln's program answers,** not just anything on the port: `service
  list` and doctor check who holds it.
- **`kiln doctor`:**
  - **A new Services area:** each instance running, stopped, or kept from running, with
    who holds its port and the fix.
  - **"Other tools"** lists other programs on the usual service ports (3306, 5432, 6379,
    1025, 8025). It also lists the system's database, cache and mail services, read from
    `sc.exe query` or `systemctl list-units` by name and state code only.
  - All of this is information, not warnings.
- **Tests:** service-program recognition, `sc.exe` and `systemctl` parsing (with real
  output shapes), the Services rules, and port choice naming another instance. 346 on
  Windows, 316 on Linux; clippy clean on both.
- **VM scenarios,** both:
  - a second Mailpit moves off the first one's ports and says whose they are;
  - with a stand-in program on 1025, `service start` refuses and names it, and doctor
    warns and lists it;
  - `service port … smtp=1027` works, mail arrives there, and the instance moves back.

  **Windows 92 checks, Ubuntu 92.**
- **On the way:**
  - **The renewal restart, timed directly on Windows** (ADR 0034 §2, amended). One run
    read 1,331 ms through HTTP, because Windows retries a refused connection after about
    0.5 s and again 1 s later. Bare TCP connects measured the server itself: 397 ms. Ubuntu
    measured 62 ms.
  - **One Windows run** saw `kilnd`'s restart of a killed web server not hold. The next run
    passed. The check now records `kilnd`'s and Caddy's logs if it fails again.
  - **The Linux stand-in** first failed to bind: the earlier SMTP checks leave sockets in
    TIME_WAIT. It now sets `SO_REUSEADDR`, as real servers do.
  - **Fixed** a doc comment in `kiln-coexist` that the Phase 3 planning commit had
    damaged.

### P3-04 delivered (2026-11-02, Windows)

[ADR 0038](adr/0038-mysql-and-mariadb.md).

- **The kinds `mysql` and `mariadb`:**
  - MySQL 8.4.13 and 9.7.4, and MariaDB 11.8.9 and 12.3.3: the vendors' archives, pinned by
    SHA-256.
  - MySQL's pins were made after checking Oracle's GPG signatures (MySQL Release
    Engineering's key, fingerprint as in Oracle's manual) and MD5s. MariaDB's come from its
    downloads API.
  - Default instance names follow the series, e.g. `mysql-8.4`.
- **Unpacking:**
  - Whole archives (`.zip`, `.tar.gz`, and `.tar.xz` through `lzma-rust2`), without debug
    symbols, static libraries, tests, docs or headers, and without MariaDB's plugins.
  - The server gets its Kiln name as a hard link: `kiln-mysqld`, `kiln-mariadbd`.
  - A path that would leave the archive's top folder is refused.
- **Running:** a config Kiln writes before every start (loopback only, `skip-name-resolve`,
  no X plugin for MySQL), plus `--no-monitor` for MySQL on Windows.
  - **First start:** creates `root@127.0.0.1` and `root@::1` without a password, through
    MySQL's init file or MariaDB's installers.
  - **Stop:** through `mysqladmin` / `mariadb-admin`, and Kiln waits until the server has
    removed its pid file, so a stopped database has finished writing.
- **`kiln db <instance> [database]`** opens the vendor's client. `kiln service logs` shows
  the server's own error log too.
- **After installing,** Kiln runs the server once:
  - On Windows, a missing DLL becomes the VC++ runtime message.
  - On Linux, a missing `libaio.so.1` is linked from Ubuntu 24.04's renamed `libaio.so.1t64`
    in Kiln's own folder, and any other missing library is named with its package.
- **I-74 fixed:** downloads resume after a stall or a broken connection, with a Range request,
  up to 3 times. MariaDB's 92 MB Windows archive had stalled for 25 minutes on a slow server.
- **Tests:** whole-tree unpacking (zip and tar.gz, filter, hard link, escape refused), the
  config, the stop command, `keep`, series names, missing-library parsing, and download
  resume, including a server that ignores the range. 356 on Windows, 326 on Linux; clippy
  clean on both.
- **Windows VM (with Herd): 99 checks passed.**
  - MySQL 8.4 on 3306 and MariaDB 11.8 beside it on 3307, only on loopback, no X plugin;
  - the Laravel app migrates against both, in databases `kiln db` made;
  - data survives a restart;
  - a stop ends in a clean shutdown.

  The renewal restart measured 656 ms.
- **On the way:**
  - The first Windows run caught `stop` returning while MySQL was still flushing: it
    closes its port first. Hence the pid-file wait.
  - The run's `Get` helper now has a 60 s limit, after a paused VM left one hanging.
- **Linux, on hold:** the code is in place and unit-tested, and the libaio handling was
  checked by hand in the Ubuntu VM. MySQL and MariaDB both ran there, with the private libaio
  link for MySQL. The Ubuntu scenario run was stopped when Linux was put on hold
  (2026-11-02). See the Phase 3 re-plan.

## Open items carried into Phase 1

| Item | Ref | Blocking? |
|---|---|---|
| Caddy admin over a Unix socket on Windows | [I-20](issues.md#i-20) | done (P1-07) |
| P1-07 live-reload check from a normal terminal | [I-45](issues.md#i-45) | done 2026-10-21 (user) |
| Dropped connection at reload, Linux recheck | [I-47](issues.md#i-47) | no (with P1-04) |
| Linux `replace_file`: copy SELinux labels / xattrs | [ADR 0008](adr/0008-unsafe-confined-to-kiln-sys.md) | part of P1-04 |
| Over-the-shoulder elevation verification | [I-41](issues.md#i-41) | P1-15 |
| Manual UAC launch check | [I-42](issues.md#i-42) | done 2026-10-21; repeat before 1.0 |
| Manual `Root` store trust check | [I-43](issues.md#i-43) | done 2026-10-21, repeated from a normal terminal with the browser check |
| Browser check of an HTTPS site from a normal terminal | [I-45](issues.md#i-45) | done 2026-10-21 (Chrome, user) |
| P1-09 PHP site check from a normal terminal | [I-45](issues.md#i-45) | done 2026-10-22 (user) |
| P1-10 park check from a normal terminal | [I-45](issues.md#i-45) | done 2026-10-22 (user) |
| P1-11 check from a normal terminal (shims + PATH) | [I-45](issues.md#i-45) | done 2026-10-22 (user) |
| P1-12 `kiln doctor` from a normal terminal | [I-45](issues.md#i-45) | done 2026-10-22 (user) |
| P1-13 history/undo/uninstall check from a normal terminal | [I-45](issues.md#i-45) | done 2026-10-22 (user), including a real uninstall |
| P1-14 coexistence check from a normal terminal (Herd running) | [I-45](issues.md#i-45) | done 2026-10-22 (user); found I-54, fixed |
| P1-04 part d (shims) | [ADR 0023](adr/0023-linux-shims-and-path.md) | done 2026-10-23 |
| P1-04b browser recheck after the I-57 fix (Firefox and Chrome windows in the VM) | [I-57](issues.md#i-57) | done 2026-10-23 (user) |
| Caddy reload vs a recycling worker (one request) | [I-50](issues.md#i-50) | no; report upstream |
| Automatic Caddy download | [ADR 0011](adr/0011-caddy-integration.md) §10 | done (P1-08) |
| Download stall detection (idle watchdog) | [I-48](issues.md#i-48) | done 2026-10-26 |
| Real-Caddy test in CI (`KILN_TEST_CADDY`) | P1-15 | done 2026-10-26 (Windows, Linux) |
| Firefox enterprise-roots / NSS import | [ADR 0010](adr/0010-local-certificate-authority.md) §10–11 | no (with `kiln doctor` / P1-04) |
| Reliable unattended VM build | [I-36](issues.md#i-36) | done 2026-10-23 (P1-15) |
| Edge DoH matrix + second WireGuard trial | [I-39](issues.md#i-39) | no |
| OpenVPN, Firefox/Chrome, WordPress and Unicode-path tests | spike results | no (Phase 1 integration suite) |
| rustup auto-install deprecation | [I-09](issues.md#i-09) | no |

## Log

| Date | What happened |
|---|---|
| 2026-10-19 | REA installed. Reconnaissance of an existing tool (architecture, feature inventory, system footprint) and a private planning package. Decisions approved: Rust + Tauri, Apache-2.0, Windows + Linux first, hosts-block DNS. |
| 2026-10-19 | Phase 0 scaffold pushed to `sameedkun/kiln` (`d599527`). CI green on 3 OSes. Bugs I-01 to I-03 fixed before the first commit. |
| 2026-10-19 | CI bumped to `actions/checkout@v5` (Node 24); later v7 via Dependabot PR #1. |
| 2026-10-20 | Spike 01: nginx vs Caddy, interleaved benchmarks plus robustness tests. **Caddy chosen**, 93 vs 80 (`dbf84c9`). Findings I-11 to I-20. |
| 2026-10-20 | Spike 02 Part 1 in Windows Sandbox: hosts block confirmed. Hosts editor made byte-oriented (I-04, I-05) (`3bcc1fa`). |
| 2026-10-20 | Spike 02 Part 2 in a Hyper-V Windows 11 VM: **NRPT rejected** (offline and VPN failures, I-37). ADR 0006 accepted (`49479cc`). VM moved from D: to E: after a disk-space scare (I-28). |
| 2026-10-20 | Docs set added: plan, progress, issues log, dev setup (`7040666`). |
| 2026-10-21 | **Phase 1 started.** I-38 decided (ADR 0008, `kiln-sys`). **P1-01 done:** `kiln-state` SQLite store, journal migration, shared journal contract tests, `kiln state info` (`f9c3f4b`). |
| 2026-10-21 | CI failed only on Windows: the ACL test's PowerShell `Get-Acl` check (I-40). Fixed by switching detection to `icacls /save`. |
| 2026-10-21 | **P1-02 done:** ADR 0009; Windows FFI in `kiln-sys::win`; new `kiln-helper` crate and binary (authenticated pipe protocol, fixed operations, hosts write policy); 28 new tests, 70 total. |
| 2026-10-21 | **P1-03 done:** `HostsBlockOp` (platform-neutral) + `HelperSession` (lazy UAC); helper protocol v2; `kiln hosts set/clear`, `kiln undo`; 12 new tests, 82 total. |
| 2026-10-21 | **I-42 manual end-to-end passed** on the dev machine: `kiln hosts set demo` (1 UAC prompt) → `demo.test` resolves → `kiln undo 1` (1 UAC prompt) → hosts file SHA-256 identical (`4387EA88…`), ACL identical, Herd's block untouched. |
| 2026-10-21 | **P1-05 done (Windows):** ADR 0010; `kiln-pki` (name-constrained root + 30-day intermediate, DPAPI-protected key); `kiln-sys::wincrypt`; `TrustCaOp`; `kiln ca init/info/trust`; 13 new tests, 95 total. |
| 2026-10-21 | **I-43 manual check passed:** `kiln ca init` → `kiln ca trust` (1 Windows dialog) → root in `CurrentUser\Root` (50 → 51) → `kiln undo 2` → back to the 50 baseline thumbprints. Fixed I-44 on the way: renames in `%APPDATA%` failed with os error 17; added `kiln_sys::rename_replace` and banned `std::fs::rename` via `clippy.toml`. |
| 2026-10-21 | **P1-06 done (Windows):** ADR 0011 (HTTP by default, HTTPS per site); new `kiln-web` crate (typed Caddy config, pinned release install, background process control); `sites` table; `kiln link/unlink/secure/unsecure/sites`, `kiln web …`, `kiln hosts sync`; 24 new tests, 119 total, plus a real-Caddy end-to-end test. Root cause of I-44 found: the Claude desktop app's MSIX AppData virtualization (I-45). Browser check left to the user from a normal terminal. |
| 2026-10-21 | **P1-06 browser check passed** (run by the user from a normal PowerShell): `http://hello.test:8080/` served over plain HTTP, `https://shop.test:8443/` shown as secure in Chrome with no warning, and `http://shop.test:8080/` redirected to HTTPS. Hosts block and trust undone afterwards. Confirms I-43 in Chrome and I-45 (fresh state outside the app container). |
| 2026-10-21 | **P1-07 done:** ADR 0012. A spike showed Caddy's admin endpoint works on a Unix socket on Windows. The admin endpoint now listens only on a per-user socket (I-20 resolved); `kiln-sys` gained `AF_UNIX` connect, and `kiln-web` an admin client with retry (I-46). Site changes reload live, and stop is graceful. 9 new tests, 128 total. Measured: a reload drops about 1 new connection per 3–7 reloads under saturating load, versus about 540 ms of refused connections per restart (I-47). The real-terminal check is left to the user (I-45: `AF_UNIX` is broken in the app container). |
| 2026-10-21 | **P1-07 live-reload check passed** (run by the user from a normal PowerShell): `kiln unsecure shop` and `kiln secure shop` each reloaded the running server, the PID was 21656 before and after, `admin.sock` existed, `netstat` showed nothing on :2019, `kiln web stop` stopped it, and hosts (#3) and trust (#4) were undone. |
| 2026-10-21 | **P1-08 done (Windows):** ADR 0013. New `kiln-fetch` (verified HTTPS downloads: digest first, OS trust store, cache) and `kiln-php` (windows.php.net index, latest-patch-only, safe extraction, php.ini defaults, start-up check); `php_versions` table; `kiln php available/install/list/remove`; `kiln web install` downloads Caddy. 20 new tests, 148 total. Real install of PHP 8.4.27 and download of Caddy 2.11.7 verified; fixed I-48 (body timeout) and I-49 (folder rename) on the way. |
| 2026-10-22 | **P1-09 done (Windows):** ADR 0014. New `kiln-supervisor` (separate binary; `pools.json` desired state; staggered recycle limits; respawn on the same port; crash = non-zero exit; kill-on-close job), `kiln-sys::background` shared helpers, PHP routes in the Caddy config (`php_fastcgi` expansion, `least_conn`, time-bounded retries, no passive checks). `kiln web start/stop/status` manage the pool; `kiln php default/workers` switch live. 19 new tests, 167 total; real end-to-end: 400 requests across about 30 recycling workers, 0 failures. Found I-50 (Caddy reload vs recycling, one request) and fixed I-51. |
| 2026-10-22 | **CI:** macOS moved to nightly + manual dispatch; Windows and Linux on every push (`c37bf8a`). The macOS job takes about 30 s but waited many minutes for a runner. |
| 2026-10-22 | **P1-10 done:** ADR 0015. New `kiln-sites` (document-root drivers for Laravel, Symfony, Drupal, Craft, Bedrock, WordPress, PHP, static; links + parked folders with precedence), `parked` table, `kiln park/unpark`, richer `kiln sites`, hosts sync covering parked sites. Valet considered, not used. 10 new tests, 177 total; the real Caddy+PHP test confirms a parked Laravel project never serves `.env`. |
| 2026-10-22 | **P1-08/09/10 checked by the user from a normal PowerShell:** `kiln php install 8.4` (33.7 MB, SHA-256 verified, `php -v` 8.4.27); `kiln park demo-sites` found `larademo` (laravel → `public`) and `phpdemo`; `hosts sync` (#5); `web start` brought up 4 php-cgi workers, all under `%LOCALAPPDATA%Kilnphp8.4.27`; in Chrome `phpdemo.test` served PHP 8.4.27 from different worker PIDs (16624, 2564), `/any/path` and `larademo.test/hello` went through the front controller, and `/artisan` wasn't served; `web stop` left no Kiln php-cgi running; unpark and `undo 5` cleaned up. |
| 2026-10-22 | **P1-11 done (Windows):** ADR 0016. `kiln isolate/unisolate` (pins per host, minor pins follow patches, fallback with warning), one php-cgi pool per version in use with untouched pools reused on change, new `kiln-runtime` (shared "which PHP" logic + `kiln-shim` binary as `php.exe`/`composer.exe`), `kiln which`, `kiln composer install` (SHA-256 verified), `kiln shims install` (journaled user-PATH prepend; undo keeps later edits). 15 new tests, 192 total; real checks with PHP 8.4.27 + 8.3.36 + Composer 2.10.3. |
| 2026-10-22 | **P1-11 checked by the user from a normal PowerShell:** installed PHP 8.3.36 + Composer 2.10.3 (verified); `larademo` pinned to 8.3 → browser and `php -v` showed 8.3.36, `phpdemo` 8.4.27; `web start` ran two pools; `unisolate`/`isolate` on the running server switched `larademo` without a restart while the 8.4 pool kept its ports (61726–61729); `shims install` (#7) showed Herd's `php.bat`/`composer.bat` being displaced, and in a new terminal `php`/`composer` followed each folder (`exit(7)` → 7); `undo 7` restored PATH (`where.exe php` → Herd's `php.bat` again); hosts `undo 6`. |
| 2026-10-22 | **P1-12 done:** ADR 0017. `kiln doctor` (read-only, fixes as commands, `--json`, exit 1 on failure); new `kiln-doctor` crate (pure rules); `kiln-sys` TCP listener owners, process parents, query-only process open. Detects the packaged-app container (I-45: the real path is `E:WindowsApps…` on this machine), ports held by other programs, PHP/pool/DNS/CA/shim problems, and explains Schannel revocation (I-12). 11 new tests, 203 total. |
| 2026-10-22 | **P1-12 checked by the user from a normal PowerShell:** idle: "normal terminal", ports 8080/8443 free, Caddy and PHP 8.3.36/8.4.27 found, and three accurate warnings (old demo links not in hosts, CA trust undone, `php` is Herd's). After `web start`: all four listeners reported as Kiln's web server (PID 15852), pool 8.4.27 with 4 workers listening. `--json` worked; exit code 0. |
| 2026-10-22 | **P1-13 done:** ADR 0018. `kiln history [--all]` (plain descriptions, times), `kiln undo` without a number (newest change in effect; clear refusal otherwise), `kiln uninstall [--dry-run|--keep-files|--yes]` and `kiln plan uninstall` (full plan: stop, undo newest first, delete Kiln's two folders with sizes, never-touched list; stops and deletes nothing if an undo fails; never deletes a folder holding a project; doesn't follow junctions). 9 new tests, 212 total, including a sandboxed real-binary uninstall. |
| 2026-10-22 | **P1-13 checked by the user from a normal PowerShell:** `hosts sync` (#8); `history` showed #8 in effect and `--all` all 8 operations (CA trust ×2, hosts ×5, PATH) with plain descriptions; `uninstall --dry-run` planned undo #8 and the deletion of `LocalKiln` (308.7 MB) and `RoamingKiln`; `undo` without a number undid #8 (1 UAC prompt), and a second `undo` reported nothing in effect; a **real `kiln uninstall`** deleted both Kiln folders and left the programs; `demo-sites` (projects) untouched. The user's machine now has no Kiln state; later manual checks start with `kiln php install` / `kiln web install`. |
| 2026-10-22 | **P1-14 done** (`2d1d787`, `0b7966e`): ADR 0019. New `kiln-coexist` crate (installed tools, port holders by program path, names other tools claim in the hosts file, free-port picking). `kiln web start` moves off ports held by others (80→8080, 443→8443) and says why, keeps chosen ports (`--auto-ports` to undo the choice); `kiln link`/`park` flag names shared with Herd; `kiln doctor` gains "Other tools". Found and fixed I-53 before release; the first Linux CI run failed on Windows path literals in one test (fixed with `Path::join`). 10 new tests, 222 total; CI green. Herd and everything of it left untouched. |
| 2026-10-22 | **P1-14 checked by the user from a normal PowerShell, with Herd running:** before any Kiln state, `doctor` showed only the missing database as a problem, plus "Other tools": Herd installed, its nginx (12 processes) on 127.0.0.1:80/:443, and `php` being Herd's `php.bat`. `web install` re-downloaded Caddy 2.11.7 (SHA-512 verified). `link herd-studio` printed the "also in the hosts file for Laravel Herd" note. `web start` said why and used 8080/8443. `doctor` then showed Kiln's server on 8080 and one warning for `herd-studio.test` with `http://herd-studio.test:8080/`. `web start --http-port 80` was refused, naming Herd and suggesting 8080 or `--auto-ports`. The next plain `web start` still used 8080. Herd was untouched. Found [I-54](issues.md#i-54): URLs printed by `link` before the first start said port 80. Fixed with `planned_port`; 223 tests. |
| 2026-10-22 | **P1-04 part a done** (`a80098e`, `16046fe`, `6eb9ebd`, and the `/proc` fix): ADR 0020. P1-04 split into parts a–d. Built the Ubuntu 24.04 Desktop test VM from Canonical's Hyper-V image (SHA-256 verified). Linux: `pkexec kiln-helper --stdio` with the shared protocol, `/etc/hosts` writes keeping xattrs, `hosts sync`/`undo`/`uninstall`, pinned Caddy `.tar.gz` releases, doctor resolver checks, port owners from `/proc`. Found I-55 (from the user's check) and I-56. 230 tests on Windows, 198 in the VM, CI green. |
| 2026-10-22 | **P1-04a checked by the user in the VM's own Terminal:** `kiln hosts sync` showed the plan, **one polkit password dialog**, recorded #2, and reloaded the server. `getent hosts hello.test` gave 127.0.0.1, and **Firefox in the VM loaded `http://hello.test:8080/`**. `kiln undo` used one more dialog, and the name was gone afterwards. `kiln doctor` showed nsswitch OK and systemd-resolved. Cancelling the dialog on a second `hosts sync` gave "approval was declined; nothing was changed". The doctor called Kiln's own port 8080 free: [I-55](issues.md#i-55), fixed and rechecked in the VM. |
| 2026-10-23 | **P1-04 part b done** (`7510533`, `1834c90`, `c2a30e2`, `c9720fe`): ADR 0021. Measured in the VM which stores Chrome, Firefox and curl read. Root key in the desktop keyring (Secret Service via zbus 6). `kiln ca trust` for every browser NSS database (`certutil`), `--system` for the system store through the helper (protocol v3, constrained-root check inside the helper). Doctor, uninstall and history cover it all. Found I-57 (from the user's check) and I-58. 243 tests on Windows, 213 in the VM, CI green. |
| 2026-10-23 | **P1-04b checked by the user in the VM's desktop:** `kiln ca init` put the root key **in the desktop keyring**. `hosts sync` (#6) and `ca trust --system` (#7–#9, Chrome, Firefox and the system store) each took **one polkit dialog**, and the doctor showed all three trusted. But curl, Chrome and Firefox rejected `https://hello.test:8443/`: [I-57](issues.md#i-57), stale site certificates from the replaced CA. Fixed and verified in the VM. `kiln uninstall` undid #9 → #6 with one dialog, **removed the key from the keyring**, and deleted both Kiln folders. |
| 2026-10-23 | **P1-04b rechecked by the user after the I-57 fix**, in the VM's desktop. `kiln ca info` showed the root key in the desktop keyring. `hosts sync` (#1) and `ca trust --system` (#2–#4) each took one polkit dialog. **curl** printed the page. The doctor showed Chrome's database, the Firefox snap profile and the system store all trusting the root. **Firefox** loaded `https://hello.test:8443/` with no warning, and **Chrome** showed "Connection is secure". |
| 2026-10-23 | **P1-04 part c done:** ADR 0022. Linux PHP: static-php-cli `common` builds of 8.2–8.5 (x86-64) with SHA-256 pinned in Kiln from two agreeing downloads, or the distribution's PHP via `--system`. Served by PHP-FPM masters under the supervisor (SIGTERM stop, parent-death signal), with per-site versions switched live. The `bulk` set was rejected for lacking `pdo_sqlite`. 247 tests on Windows, 218 in the VM. |
| 2026-10-23 | **P1-04 part d done, so P1-04 is complete:** ADR 0023. Linux shims are copies of `kiln-shim` (role from `argv[0]` first), put on PATH by Kiln's `env` script, sourced from a journaled marked block in each shell startup file (`env.shell_profile`). Verified in the VM: per-folder PHP, Composer, exit codes, one PATH entry, and byte-identical undo. 251 tests on Windows, 222 in the VM. **Linux now matches Windows for Phase 1; only P1-15 is left.** |
| 2026-10-23 | **P1-15 started** (`0d04c62`, `4103bd3`): ADR 0024. The Ubuntu scenario passed: 16 checks, 14 facts identical after uninstall. Found and fixed I-59 (uninstall with a locked keyring). The Windows test VM built unattended on the first try (I-36 fixed). Laravel Herd was installed in it and a `clean` checkpoint taken. |
| 2026-10-26 | **P1-15 done, so all 15 Phase 1 tasks are done.** The Windows scenario passed with Herd running: 20 checks, 14 facts identical after uninstall, and Herd restarted without affecting Kiln. Fixed I-60 (Windows: background processes inherited `kiln`'s output pipe). The test harness learned that certificate dialogs open behind focused windows, that a restored VM keeps a stale IP lease, and that the guest's resolution changes between boots. |
| 2026-10-26 | Phase 1 exit criterion reworded: uninstall is verified by scripted VM runs before each release, not by CI VMs (ADR 0024). Fixed I-61: `kiln php install` had failed on every version after windows.php.net started listing rebuilds as `8.4.27-1`. Stalled downloads now give up after a minute (I-48). CI runs the real-Caddy test on Windows and Linux (`a83c527`). |
| 2026-10-27 | **Phase 1 done.** The VM scenarios gained a real Laravel app, WordPress and a second PHP version. They found I-62: WordPress needs `mysqli`, so it is enabled on Windows. On Linux, no published static build had both `mysqli` and `pdo_sqlite`. ADR 0025: Kiln builds its own static PHP 8.2–8.5 in the new public repo `sameedkun/kiln-php` (release `2026-10-27`, licences included), pinned after two independent downloads and runs. Windows passed 29 checks, Ubuntu 28; uninstall left both machines as they were. |
| 2026-10-27 | **Phase 2 planned:** exit criteria and 11 tasks (P2-01 to P2-11) in plan.md. ADR 0026: `kilnd`, an optional per-user daemon that keeps the web stack running, renews the intermediate certificate and serves a JSON-RPC API over a per-user Unix socket. Autostart is journaled. Next: P2-01. |
| 2026-10-27 | **P2-01 done:** new `kiln-daemon` crate and `kilnd` binary (ADR 0026): JSON-RPC API over a per-user socket with `kiln.hello`, `daemon.status/shutdown`, `web.status` and events; a watchdog that restarts a web stack that exited on its own, with backoff; `kiln daemon start/stop/status/call`; a change lock for every changing `kiln` command; a non-inheritable `AF_UNIX` listener on Windows. VM scenarios: Windows 37 checks, Ubuntu 35, including killing Caddy and the daemon bringing it back. |
| 2026-10-27 | **P2-02 done:** `kiln daemon enable/disable`, a journaled autostart (Windows `Run` value, Linux XDG autostart entry; ADR 0026 §6 amended). Checked across real reboots in both VMs, Windows also after a hard reset. Fixed I-64 (flush `HKCU` after registry writes) and I-63 (piping `kiln` into `head` exits quietly). Scenarios: Windows 39 checks, Ubuntu 37. |
| 2026-10-28 | **P2-03 done:** `kiln ca renew [--force]`; `kilnd` checks the intermediate every 6 hours and renews it when fewer than 10 days are left. A running server restarts briefly to use it (ADR 0026 §2 amended: Caddy keeps old site certificates across a reload, I-57). API: `ca.status`, `ca.renew`; events `ca.renewed` and `ca.renewal_failed`. A renewal forced through the API passed in both VMs: Windows 42 checks, Ubuntu 40. |
| 2026-10-28 | **P2-04 done:** ADR 0027. A project's `kiln.toml` sets its name, HTTPS, aliases and PHP (and later Node) version, read live; an isolate pin still wins. `kiln import herd` turns Laravel Herd's `herd.yml` into `kiln.toml` and links the folder. Fixed I-65 (PHP limits now 512 MB, 60 s, 64 MB uploads on both systems). Scenarios: Windows 46 checks, Ubuntu 47. |
| 2026-10-28 | **P2-05 done:** ADR 0028. `kiln node available/install/list/remove/default` from nodejs.org, with `SHASUMS256.txt`'s OpenPGP signature checked against Node's release keys (new `kiln-node` crate; `ring` does the arithmetic, no new crates). `node`/`npm`/`npx`/`corepack` shims follow the nearest `kiln.toml`, `.nvmrc` or `.node-version`, and step aside to an existing Node while Kiln has none. Scenarios: Windows 52 checks (Herd's machine-wide Node stays first, as documented), Ubuntu 53, both building the Laravel app with `npm run build`. |
| 2026-10-29 | **P2-06 done:** ADR 0029. `kiln php update [minor]` installs the newest patch beside the current one and switches live; `kiln php rollback` goes back and removes it. Kiln now pins the previous patch of each line: on Linux from the new kiln-php release `2026-10-29` (the workflow builds exact patches on request), on Windows from windows.php.net's archives. Scenarios: Windows 56 checks, Ubuntu 57, each with a real update and rollback. |
| 2026-10-29 | **P2-07 done:** ADR 0030. `kiln php ext list/on/off [--php X]`: `php -m` says what is loaded; each toggle is one journaled line in the build's `php.ini` (`PhpExtOp`), undone byte for byte by `kiln undo`, checked after `on` and applied live by restarting that build's workers. `php update` carries toggles over; `rollback`/`remove` undo them first. Kiln's static Linux builds list everything as compiled in; a system PHP's `conf.d` modules are left to `phpdismod`. Scenarios: Windows 61 checks, Ubuntu 60. |
| 2026-10-30 | **P2-08 done:** ADR 0031. `kiln new <name> [--path] [--php] [--secure]` runs `composer create-project laravel/laravel` with Kiln's PHP and Composer (installing Composer if missing), then links the project as `<name>.test` (or secures it where a parked folder already serves it); `--php` is kept in the project's `kiln.toml`. Fixed I-66: nodejs.org's index now lists Node 27 alphas, which broke `kiln node install`; pre-releases are skipped. Scenarios: Windows 63 checks, Ubuntu 62. |
| 2026-10-30 | **P2-09 done:** ADR 0032. `kiln proxy <name> <target> [--secure]` serves `<name>.test` from a loopback dev server through Caddy, with the dev server's own `Host`, WebSocket upgrades for live reload, and HTTPS like any site; `kiln unproxy` removes it. Proxies have their own table (schema v6). Fixed I-67 (php.ini replace retried briefly on Windows) and I-69 (`kiln` now waits for Caddy's certificate before reporting a `--secure` change done); I-68 turned off the firewall prompt in the Windows test VM; I-70 logged (Node's 250 ms connect limit vs Bunny Fonts). Scenarios: Windows 66 checks, Ubuntu 65. |
| 2026-10-30 | **P2-10 done:** ADR 0033. `kiln ca init --tld <name>` chooses the development TLD (only `test`, `internal`, `example`, `invalid`), recorded as the `tld` setting that every command and the shims read; the root's own TLD always wins, and another TLD over an existing root is refused. Fixed I-71 (Caddy installed its own `local` root into the trust stores for a `.internal` site; now declared with `install_trust: false`) and I-72 (the first site change after `web start` restarted the server). Scenarios: Windows 71 checks, Ubuntu 72, each with a second stage under `.internal` and a second uninstall comparison. |
| 2026-10-31 | **P2-11 done, Phase 2 done:** ADR 0034. Each VM scenario now reboots after `kiln daemon enable` and checks that the login started `kilnd`, which brought the web server, PHP and the HTTPS sites back, before uninstall and the comparison; `npm` is checked on the project's Node; the renewal restart forced through the API is timed. That found I-73 (8.1 s on Windows): the restart after a CA change now restarts only Caddy, on a config validated while the old server serves (about 0.1 s; a client sees 0.67 s on Windows, 0.07 s on Ubuntu). Scenarios: Windows 80 checks, Ubuntu 80. Phase 2's exit criteria are met in both VMs. |
| 2026-10-31 | **Phase 3 planned:** exit criteria and 12 tasks (P3-01 to P3-12) in plan.md, for Windows and Linux. ADR 0035: services as per-user instances (several versions side by side) behind compiled-in providers, run by a second `kiln-supervisor`, loopback only, on their usual port or the next free one; development credentials; data never deleted without `--delete-data`; GPL services downloaded from the vendor; PostgreSQL built by Kiln for Linux; a spike decides Windows' cache (Garnet). macOS moves to a new Phase 4b (ADR 0007 amended): no Apple hardware to test on and no Developer account yet. Next: P3-01. |
| 2026-10-31 | **P3-01 done:** ADR 0036. `kiln service add/list/start/stop/restart/logs/remove` and `kiln services up/down`; instances in a `services` table (schema v7); a second `kiln-supervisor` that now runs programs too, stopped gracefully; the new `kiln-services` crate with contract tests for every provider; Mailpit 1.31.6 as the first provider, running as `kiln-mailpit` on loopback. Uninstall refuses while services hold data unless `--delete-data`. I-74 logged (GitHub downloads stalling mid-file). Scenarios: Windows 87 checks, Ubuntu 87. |
| 2026-11-01 | **P3-02 done:** spike 03. Garnet 2.2.3 on Windows passed 19 of 22 Laravel checks (Valkey control: 22 of 22). Its Lua scripts ignore `SELECT` (microsoft/garnet#2258, fix open), which breaks `Cache::add`, tag flushes and the rate limiter on Laravel's cache database. Using one database would pass but make `cache:clear` delete queued jobs, so it isn't done. Garnet also needs .NET 10, which Kiln would keep privately. ADR 0035 §8 amended: Windows gets Garnet once a release with the fix passes the spike; until then, no Windows cache (I-75). |
| 2026-11-01 | **P3-03 done:** ADR 0037. A busy service port names its holder (database, cache, mail and Docker programs recognized; a running system service of that kind when the holder can't be inspected); a start refuses a port another program took, and `kiln service port` moves the instance; stop and remove wait until the program has let go of its ports; doctor gains a Services area and lists other services and system services (`sc.exe`, `systemctl`). ADR 0034 §2 amended: Windows times the restart with bare TCP connects (397 ms; HTTP read 1,331 ms through Windows' connect retries). Scenarios: Windows 92 checks, Ubuntu 92. |
| 2026-11-02 | **P3-04 done (Windows):** ADR 0038. MySQL 8.4/9.7 and MariaDB 11.8/12.3 from their vendors (MySQL pins after checking Oracle's GPG signatures); whole archives unpacked without the extras, the server hard-linked under a Kiln name; a loopback-only config; root for 127.0.0.1 and ::1 at first start; stop waits for the pid file to go; `kiln db`. I-74 fixed: downloads resume. Windows VM: 99 checks. Linux unit-tested; its VM run stopped as Linux goes on hold. |
| 2026-11-02 | **Phase 3 re-planned:** ADR 0039. Windows first; Linux on hold (its code keeps building and passing unit tests; its services will run as rootless Podman containers, like Lerd, when it resumes). The Windows catalog follows Herd Pro: PostgreSQL, Redis, MongoDB, Meilisearch, RustFS, Reverb, plus phpMyAdmin and Adminer; MinIO publishes source only now. Integration tests move from local VMs to GitHub Actions' Windows runners (P3-13); Herd coexistence is checked by hand before releases. Tasks P3-13 to P3-16 added. Next: P3-13. |
| 2026-11-02 | **P3-13 done:** the Windows services scenario runs on `windows-latest` on every push to `main` (`.github/workflows/scenario.yml`, `tests/ci/windows-services.ps1`, reusing the VM's `lib.ps1` and `snapshot.ps1`). Mailpit with SMTP, a second instance moving ports, a squatter refused and named by `service start` and `doctor`, MySQL 8.4 and MariaDB 11.8 side by side on loopback with `kiln db`, restart and clean stop, uninstall refused then `--delete-data`, and the machine identical before and after. Green on the first run in about 5 minutes (Windows Server 2025). Ports are read from Kiln's output, since a runner may have its own services. The snapshot now tolerates a missing `Run` key. Next: P3-05 PostgreSQL. |
| 2026-11-02 | **P3-05 done:** PostgreSQL on Windows ([ADR 0040](adr/0040-postgresql.md)). EDB's 18.6 and 17.11 builds, pinned by Kiln's own SHA-256 since EDB publishes none, keeping about 140 MB of each 960 MB archive (pgAdmin, Stack Builder, docs and headers left out). `initdb` with `--auth=trust` (loopback lines only), UTF-8 and the C locale; superuser `postgres`; Kiln's `kiln.cnf` includes `initdb`'s config and sets `listen_addresses='127.0.0.1'` and the port; `pg_ctl stop -m fast`; `kiln db` opens `psql`. PostgreSQL refuses admin rights, so the services supervisor now starts with a restricted token (Administrators and Power Users deny-only, the user in its default DACL), as `pg_ctl` does: every service runs with least rights, elevated or not ([I-76](issues.md#i-76)). Providers gain `ready()`: PostgreSQL is ready when `postmaster.pid` says so, not when its port opens ([I-78](issues.md#i-78)). A daemon test race fixed ([I-77](issues.md#i-77)). The CI scenario (elevated runner) now has 24 checks, green. Next: P3-06 Redis. |
| 2026-11-02 | **P3-06 done:** Redis on Windows ([ADR 0041](adr/0041-redis.md)). `zkteco-home/redis-windows` was rejected: its binaries are committed with no source for the Windows port and no build workflow, and are labelled MIT though Redis 8 is RSALv2/SSPLv1/AGPLv3. Kiln uses `redis-windows/redis-windows`' MSYS2 builds instead: upstream tags built by a public workflow, digests matching GitHub's. Redis 8.10.3 and 8.8.3, downloaded at the user's request and not redistributed; relative paths in a Kiln-written config ([I-80](issues.md#i-80)); `redis-cli shutdown` saves; `kiln db redis-8.10 [n]`. The supervisor now opens program logs for writing at the end, not append-only, which MSYS2 programs couldn't write through ([I-79](issues.md#i-79)). CI checks databases 0 and 1, a Lua script in database 1 (Garnet's I-75 case), data across a restart and a clean shutdown: 31 checks, green. Laravel's own cache, session and queue checks move to P3-08 with `.env` wiring. Next: P3-14 (Meilisearch, RustFS, MongoDB). |
| 2026-11-02 | **P3-14 done:** Meilisearch, RustFS and MongoDB on Windows ([ADR 0042](adr/0042-meilisearch-rustfs-mongodb.md)). Meilisearch 1.56.1 (Community Edition, development mode, no analytics); RustFS 1.0.4 (both addresses pinned to 127.0.0.1, as its defaults listen everywhere; relative volume, as it splits paths at spaces; credentials `rustfsadmin`); MongoDB 9.0.4 and 8.0.34 (SSPL, from MongoDB at the user's request; debug symbols, mongos and installers left out; 256 MB WiredTiger cache). Providers gain companion archives: MongoDB brings `mongosh` for its clean shutdown (`mongod --shutdown` is Linux-only) and `kiln db`, kept inside the instance folder and with telemetry and update checks off. Single-file installs now stream to disk, and downloads may be up to 2 GB ([I-81](issues.md#i-81)). CI round trips: Meilisearch index and search, S3 bucket and object over signed requests, MongoDB through `kiln db` and a clean shutdown: 45 checks, green. Windows now has Herd Pro's services except Reverb (P3-16). Next: P3-15 (phpMyAdmin and Adminer). |
| 2026-11-03 | **P3-15 done:** phpMyAdmin and Adminer as Kiln sites ([ADR 0043](adr/0043-phpmyadmin-and-adminer.md)). `kiln tool add phpmyadmin|adminer` downloads phpMyAdmin 5.2.3 (its published SHA-256) or Adminer 6.2.0 (GitHub's digest) into `<local data>/tools`, writes Kiln's config beside it and links it as `pma.test` or `adminer.test`; `kiln tool remove` undoes it. Both read `tools/servers.json`, which the CLI rewrites after every service change, so they always list the current instances: phpMyAdmin logs in to each MySQL and MariaDB instance as root, and a small Adminer plugin offers MySQL, MariaDB and PostgreSQL instances by name and logs in without a password (Adminer's CSRF token stays). No PHP pin: CI runs both on PHP 8.5 and fails on any warning or deprecation. Fixed on the way: a failed `servers.json` write blocked service commands ([I-82](issues.md#i-82)); uninstall kept Kiln's folder because a tool's site looked like a project ([I-83](issues.md#i-83)). CI: 52 checks, green. Next: P3-07 (Mailpit at `mail.test`, `MAIL_*` wiring). |

## How to update this file

At the end of each working session: update the task table statuses, add a log row with the
commit hash, and move anything new and unexpected into [issues.md](issues.md).
