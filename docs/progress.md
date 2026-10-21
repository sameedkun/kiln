# Progress

**Last updated:** 2026-10-21 · **Current phase:** **Phase 1 in progress** (5 of 15 tasks done) · **CI:** green on Windows, Linux, macOS

## At a glance

| Area | State |
|---|---|
| Research (how existing tools work, feature parity, risks) | **done**: private reconnaissance package, outside this repo |
| Architecture and key decisions | **done**: ADRs 0001–0011 accepted |
| Local CA (`kiln-pki`) | **done** (Windows): `.test`-only root, DPAPI-protected key, 30-day intermediate; `kiln ca init/info/trust`; verified on the dev machine: trust → in `CurrentUser\Root` → undo, store identical to baseline ([I-43](issues.md#i-43)) |
| Privileged helper (`kiln-helper`, Windows) | **done**: UAC per batch, private pipe, HMAC-signed messages, fixed operations (protocol v2) |
| Hosts DNS on Windows (`HostsBlockOp`) | **done**: `kiln hosts set` / `clear` and `kiln undo`, journaled, one UAC prompt per change; verified on the dev machine: set → resolve → undo, byte-identical ([I-42](issues.md#i-42)) |
| Repository, CI, license and dependency policy | **done** (plus a CI guard keeping `unsafe` inside `kiln-sys`) |
| Operation engine (`kiln-ops`) | **done**: engine, rollback, undo, recovery, shared journal contract tests |
| Durable state (`kiln-state`) | **done**: SQLite store, journal with full history, one-time JSONL import |
| OS wrappers (`kiln-sys`) | **growing**: `replace_file` / `write_replacing` / `rename_replace`; Windows pipes, SIDs, system directory, UAC launch, process handles |
| Hosts-file editor (`kiln-platform::hosts`) | **done**: byte-safe, property-tested |
| Web server choice | **done**: Caddy ([spike 01](spikes/results/01-web-server.md)) |
| Windows DNS strategy | **done**: hosts block; NRPT rejected ([spike 02](spikes/results/02-windows-dns-part2.md)) |
| Web server (`kiln-web`, Caddy) | **done** (Windows): `kiln link`, `secure` / `unsecure`, `sites`, `web install/start/stop/status`, `hosts sync`. Sites are **HTTP by default; HTTPS per site**. Verified against real Caddy 2.11.7; browser check pending ([I-45](issues.md#i-45)) |
| Serving a PHP site end to end | **static sites work**; PHP next (P1-08, P1-09) |
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
| P1-07 | Caddy admin API lockdown + live reload | **next** (or P1-08) | [I-20](issues.md#i-20); P1-06 runs with the admin endpoint disabled and restarts on change |
| P1-08 | PHP runtime provider | later | |
| P1-09 | Process supervisor | later | [I-14](issues.md#i-14) |
| P1-10 | Sites + drivers | later | |
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

**Browser check to run from a normal terminal** (Windows Terminal or PowerShell from the Start
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

## Open items carried into Phase 1

| Item | Ref | Blocking? |
|---|---|---|
| Caddy admin over a Unix socket on Windows | [I-20](issues.md#i-20) | blocks P1-07 |
| Linux `replace_file`: copy SELinux labels / xattrs | [ADR 0008](adr/0008-unsafe-confined-to-kiln-sys.md) | part of P1-04 |
| Over-the-shoulder elevation verification | [I-41](issues.md#i-41) | P1-15 |
| Manual UAC launch check | [I-42](issues.md#i-42) | done 2026-10-21; repeat before 1.0 |
| Manual `Root` store trust check | [I-43](issues.md#i-43) | done 2026-10-21 (inside the app container); repeat with the browser check |
| Browser check of an HTTPS site from a normal terminal | [I-45](issues.md#i-45) | no; user runs it ([script](#p1-06)) |
| Automatic Caddy download | [ADR 0011](adr/0011-caddy-integration.md) §10 | with P1-08 |
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

## How to update this file

At the end of each working session: update the task table statuses, add a log row with the
commit hash, and move anything new and unexpected into [issues.md](issues.md).
