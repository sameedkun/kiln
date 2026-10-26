# Progress

**Last updated:** 2026-10-26 · **Current phase:** **Phase 1 tasks done** (15 of 15) · **CI:** green on Windows, Linux, macOS

## At a glance

| Area | State |
|---|---|
| Research (how existing tools work, feature parity, risks) | **done**: private reconnaissance package, outside this repo |
| Architecture and key decisions | **done**: ADRs 0001–0024 accepted |
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
| Linux (Ubuntu 24.04 Desktop VM) | **part a done**: `kiln link`, `web install/start` (Caddy 2.11.7 for Linux, live reload over a Unix socket), `hosts sync` and `undo` through **one polkit password dialog per action**, `kiln doctor` (nsswitch, systemd-resolved, port owners from `/proc`). Checked in Firefox in the VM. **Part b done:** the CA root key in the desktop keyring; `kiln ca trust` for Chrome and Firefox (no password), `--system` for curl/PHP/Node (one dialog); `kiln uninstall` removes it all, keyring entry included. **Part c done:** `kiln php install 8.4` (static builds, SHA-256 pinned in Kiln) or `--system` (the distribution's PHP), served by PHP-FPM, per-site versions switched live. **Part d done:** `kiln shims install` puts `php`/`composer` shims on PATH through a block in the shell startup files, undone byte for byte. **Linux now matches Windows for Phase 1.** |
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
## Phase 1: MVP on Windows + Linux (in progress)

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
    - It passes locally against Caddy 2.11.7. CI skips it until P1-15.
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
- **VM build:** the Windows test VM now builds unattended ([I-36](issues.md#i-36) fixed).
- **How to run them:** in [dev-setup.md](dev-setup.md#integration-tests-in-vms-p1-15).
  They are manual, not CI.

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
| Download stall detection (idle watchdog) | [I-48](issues.md#i-48) | no |
| Real-Caddy test in CI (`KILN_TEST_CADDY`) | P1-15 | no |
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

## How to update this file

At the end of each working session: update the task table statuses, add a log row with the
commit hash, and move anything new and unexpected into [issues.md](issues.md).
