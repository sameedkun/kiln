# Progress

**Last updated:** 2026-10-22 · **Current phase:** **Phase 1 in progress** (8 of 15 tasks done) · **CI:** green on Windows, Linux, macOS

## At a glance

| Area | State |
|---|---|
| Research (how existing tools work, feature parity, risks) | **done**: private reconnaissance package, outside this repo |
| Architecture and key decisions | **done**: ADRs 0001–0014 accepted |
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
| Serving a PHP site end to end | **works** (Windows): linked sites run PHP through supervised php-cgi workers behind Caddy; framework drivers next (P1-10) |
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
| P1-04 | Linux DNS providers | later | `HostsBlockOp` is platform-neutral; Linux needs a polkit `PrivilegedHosts` |
| P1-05 | Local CA + trust stores | **done** 2026-10-21 (Windows) | see below; Linux key encryption + NSS with P1-04 |
| P1-06 | Caddy integration | **done** 2026-10-21 (Windows) | [ADR 0011](adr/0011-caddy-integration.md); see below. HTTP by default, HTTPS per site |
| P1-07 | Caddy admin API lockdown + live reload | **done** 2026-10-21 | [ADR 0012](adr/0012-caddy-admin-channel.md); see below. Admin on a per-user Unix socket, live reload; verified from a normal terminal |
| P1-08 | PHP runtime provider | **done** 2026-10-21 (Windows) | [ADR 0013](adr/0013-downloads-and-php-runtimes.md); see below. Shared verified downloader; `kiln web install` downloads Caddy |
| P1-09 | Process supervisor | **done** 2026-10-22 (Windows) | [ADR 0014](adr/0014-php-cgi-supervisor.md); see below. php-cgi pools behind Caddy, staggered recycling |
| P1-10 | Sites + drivers | **next** | park; Laravel `public/` and other framework drivers |
| P1-11 | Per-site PHP + shims | later | |
| P1-12 | `kiln doctor` | later | |
| P1-13 | `kiln plan` / `undo` / `uninstall --dry-run` | later | |
| P1-14 | Coexistence detection | later | [I-11](issues.md#i-11) |
| P1-15 | Integration test VMs | later | needs a reliable VM build ([I-36](issues.md#i-36)) |

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
- **Not checkable from the agent session:** `kiln web start` (I-45). Script below.

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
| P1-09 PHP site check from a normal terminal | [I-45](issues.md#i-45) | no; user runs it ([script](#p1-09)) |
| Caddy reload vs a recycling worker (one request) | [I-50](issues.md#i-50) | no; report upstream |
| Automatic Caddy download | [ADR 0011](adr/0011-caddy-integration.md) §10 | done (P1-08) |
| Download stall detection (idle watchdog) | [I-48](issues.md#i-48) | no |
| Real-Caddy test in CI (`KILN_TEST_CADDY`) | P1-15 | no |
| Firefox enterprise-roots / NSS import | [ADR 0010](adr/0010-local-certificate-authority.md) §10–11 | no (with `kiln doctor` / P1-04) |
| Reliable unattended VM build | [I-36](issues.md#i-36) | blocks P1-15 |
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

## How to update this file

At the end of each working session: update the task table statuses, add a log row with the
commit hash, and move anything new and unexpected into [issues.md](issues.md).
