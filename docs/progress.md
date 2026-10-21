# Progress

**Last updated:** 2026-10-21 · **Current phase:** **Phase 1 in progress** (3 of 15 tasks done) · **CI:** green on Windows, Linux, macOS

## At a glance

| Area | State |
|---|---|
| Research (how existing tools work, feature parity, risks) | **done**: private reconnaissance package, outside this repo |
| Architecture and key decisions | **done**: ADRs 0001–0009 accepted |
| Privileged helper (`kiln-helper`, Windows) | **done**: UAC per batch, private pipe, HMAC-signed messages, fixed operations (protocol v2) |
| Hosts DNS on Windows (`HostsBlockOp`) | **done**: `kiln hosts set` / `clear` and `kiln undo`, journaled, one UAC prompt per change; real-machine run pending ([I-42](issues.md#i-42)) |
| Repository, CI, license and dependency policy | **done** (plus a CI guard keeping `unsafe` inside `kiln-sys`) |
| Operation engine (`kiln-ops`) | **done**: engine, rollback, undo, recovery, shared journal contract tests |
| Durable state (`kiln-state`) | **done**: SQLite store, journal with full history, one-time JSONL import |
| OS wrappers (`kiln-sys`) | **growing**: `replace_file` / `write_replacing`; Windows pipes, SIDs, system directory, UAC launch |
| Hosts-file editor (`kiln-platform::hosts`) | **done**: byte-safe, property-tested |
| Web server choice | **done**: Caddy ([spike 01](spikes/results/01-web-server.md)) |
| Windows DNS strategy | **done**: hosts block; NRPT rejected ([spike 02](spikes/results/02-windows-dns-part2.md)) |
| Serving a PHP site end to end | **not started** (Phase 1) |
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
| P1-02 | `kiln-helper` (Windows) | **done** 2026-10-21 | [ADR 0009](adr/0009-windows-privileged-helper.md); see below. Open: [I-41](issues.md#i-41), [I-42](issues.md#i-42) |
| P1-03 | `HostsBlock` provider (Windows) | **done** 2026-10-21 | see below |
| P1-04 | Linux DNS providers | **next** (or P1-05) | `HostsBlockOp` is platform-neutral; Linux needs a polkit `PrivilegedHosts` |
| P1-05 | Local CA + trust stores | later | |
| P1-06 | Caddy integration | later | |
| P1-07 | Caddy admin API lockdown | later | [I-20](issues.md#i-20) |
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
- **Not yet verified on the real machine:** the UAC-launched path changes the real hosts file;
  the steps are in [I-42](issues.md#i-42).

## Open items carried into Phase 1

| Item | Ref | Blocking? |
|---|---|---|
| Caddy admin over a Unix socket on Windows | [I-20](issues.md#i-20) | blocks P1-07 |
| Linux `replace_file`: copy SELinux labels / xattrs | [ADR 0008](adr/0008-unsafe-confined-to-kiln-sys.md) | part of P1-04 |
| Over-the-shoulder elevation verification | [I-41](issues.md#i-41) | P1-15 |
| Manual UAC launch check | [I-42](issues.md#i-42) | no (run before 1.0) |
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
| 2026-10-21 | **P1-03 done:** `HostsBlockOp` (platform-neutral) + `HelperSession` (lazy UAC); helper protocol v2; `kiln hosts set/clear`, `kiln undo`; 12 new tests, 82 total. Real-machine UAC run pending (I-42). |

## How to update this file

At the end of each working session: update the task table statuses, add a log row with the
commit hash, and move anything new and unexpected into [issues.md](issues.md).
