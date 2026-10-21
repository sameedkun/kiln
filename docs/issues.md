# Issues log

Every problem hit so far: code bugs, environment and tooling trouble, and design surprises found
by spikes. Each entry has a cause, a resolution and a status. IDs are stable; cite them in
commits and PRs.

**Status:** ✅ resolved · 🔶 open · 📌 design rule (resolved by a decision we must keep honouring)

## Summary

| ID | Area | Issue | Status |
|---|---|---|---|
| [I-01](#i-01) | code | Journal lost records appended after a torn (half-written) line | ✅ |
| [I-02](#i-02) | code | Journal repair failed on Windows: append-only handles can't truncate | ✅ |
| [I-03](#i-03) | code | Hosts block removal didn't round-trip for files without a trailing newline | ✅ |
| [I-04](#i-04) | code | Hosts editor couldn't edit non-UTF-8 hosts files | ✅ |
| [I-05](#i-05) | code | Hosts block flipped to CRLF when foreign content ended in a lone `\r` | ✅ |
| [I-06](#i-06) | tooling | No Rust toolchain; the portable GNU toolchain couldn't link Windows crates | ✅ |
| [I-07](#i-07) | tooling | Git Bash rewrote `openssl -subj "/CN=…"` into a Windows path | ✅ |
| [I-08](#i-08) | tooling | Git Bash quirks: `C:` in PATH, MSYS `whoami` shadowing Windows `whoami` | ✅ |
| [I-09](#i-09) | tooling | rustup warns that auto-installing the pinned toolchain is deprecated | 🔶 minor |
| [I-10](#i-10) | process | CI Node 20 deprecation; push rejected after a Dependabot merge | ✅ |
| [I-11](#i-11) | coexistence | Herd kills every `nginx.exe` on the machine when it starts | 📌 |
| [I-12](#i-12) | TLS | Windows Schannel clients hard-fail revocation checks against a local CA | 📌 |
| [I-13](#i-13) | benchmark | Laravel file sessions under synthetic load distorted throughput about 10× | ✅ |
| [I-14](#i-14) | runtime | Synchronized php-cgi recycling plus passive health checks caused a 10 s outage | 📌 |
| [I-15](#i-15) | Caddy | `lb_retries` capped retries and left ~9% errors during recycling | 📌 |
| [I-16](#i-16) | Caddy | Caddy binds all interfaces by default | 📌 |
| [I-17](#i-17) | web | Unknown hosts get a 200 (Caddy) or the first site (nginx) | 📌 |
| [I-18](#i-18) | Caddy | Caddyfile syntax gotchas (inline blocks, `persist`, TLS on HTTP sites) | ✅ |
| [I-19](#i-19) | nginx | nginx on Windows is 2–25× slower on static files | 📌 (→ Caddy) |
| [I-20](#i-20) | security | Caddy admin API listens on TCP `localhost:2019`, reachable by any local process | 🔶 |
| [I-21](#i-21) | sandbox | `wsb start` never runs `LogonCommand` (no interactive logon) | ✅ |
| [I-22](#i-22) | scripting | `R` is a built-in PowerShell alias (`Invoke-History`) | ✅ |
| [I-23](#i-23) | sandbox | Files in a mapped folder are locked while the Sandbox runs | ✅ |
| [I-24](#i-24) | safety | Recursive deletes via variables or top-level paths are blocked by a safety check | ✅ (by design) |
| [I-25](#i-25) | hosts | Rename-based file replacement drops explicit ACEs; `ReplaceFileW` keeps them | 📌 |
| [I-26](#i-26) | hosts | Read-only attribute, sharing violations and a missing file all break writes | 📌 |
| [I-27](#i-27) | hosts | Defender is off in Windows Sandbox, so detection tests there are inconclusive | ✅ (retested in VM) |
| [I-28](#i-28) | infra | D: nearly filled up during the VM install | ✅ |
| [I-29](#i-29) | infra | `Move-VMStorage` needs full admin, not just Hyper-V Administrators | ✅ (worked around) |
| [I-30](#i-30) | infra | Hyper-V Administrators membership needs a fresh sign-in | ✅ |
| [I-31](#i-31) | infra | Fresh Windows blocks scripts (execution policy Restricted) | ✅ |
| [I-32](#i-32) | scripting | `$p` overwrote `$P`: PowerShell variable names are case-insensitive | ✅ |
| [I-33](#i-33) | testing | Headless Edge prints nothing in session 0 and exits when elevated | ✅ |
| [I-34](#i-34) | infra | Checkpoint taken mid-setup → reboot → test credentials rejected | ✅ (lesson) |
| [I-35](#i-35) | infra | Checkpoint restore failed: not enough host memory | ✅ |
| [I-36](#i-36) | infra | Rebuilt VM installs (~20 GB written) but has no bootable OS | 🔶 |
| [I-37](#i-37) | DNS | NRPT + stub stops resolving when offline and under a full-tunnel VPN | 📌 (NRPT rejected) |
| [I-38](#i-38) | code | `ReplaceFileW` needs Win32 FFI, but `unsafe` is forbidden | ✅ (ADR 0008) |
| [I-39](#i-39) | testing | Edge × DNS-over-HTTPS results unreliable (cold session); recheck pending | 🔶 |
| [I-40](#i-40) | CI | ACL test passed locally but failed on the GitHub Windows runner (PowerShell `Get-Acl` check) | ✅ |
| [I-41](#i-41) | helper | Over-the-shoulder elevation (helper runs as a different admin) not yet verified | 🔶 |
| [I-42](#i-42) | helper | UAC launch path can only be tested manually | ✅ (manual run passed 2026-10-21) |

---

## Code

<a id="i-01"></a>
### I-01: Journal lost records after a torn line ✅
- **Found:** 2026-10-19, while writing `kiln-ops` journal tests.
- **Cause:** after a crash mid-write, the next append was glued onto the half-written line, so
  a valid record was silently dropped on read.
- **Fix:** `FileJournal::open` truncates an unterminated tail before appending. Safe, because
  that write was never acknowledged. Test `torn_final_line_is_dropped_on_open_so_later_events_are_not_lost`.

<a id="i-02"></a>
### I-02: Journal repair failed on Windows ✅
- **Found:** 2026-10-19, first local test run on Windows ("Access is denied", os error 5).
- **Cause:** a handle opened in append mode lacks `FILE_WRITE_DATA`, so `set_len` fails. Linux
  CI would have passed.
- **Fix:** open with `write(true)` (no append) for the repair. Takeaway: always run tests on
  Windows locally or in CI before trusting file-system code.

<a id="i-03"></a>
### I-03: Hosts block round-trip broken for files without a trailing newline ✅
- **Found:** 2026-10-19, self-review before the first build.
- **Cause:** appending added two line endings in one case, so removal couldn't know how many
  to strip.
- **Fix:** always add exactly one line ending, and strip exactly one on removal.

<a id="i-04"></a>
### I-04: Hosts editor couldn't edit non-UTF-8 files ✅
- **Found:** 2026-10-20, spike 02 Part 1 in Windows Sandbox (a Latin-1 `0xE9` comment).
- **Cause:** the API was `&str`-based. Real hosts files may contain legacy code-page bytes.
- **Fix:** `kiln_platform::hosts` works on `&[u8]` / `Vec<u8>`. A property test feeds arbitrary
  bytes (20,000 cases). Commit `3bcc1fa`.

<a id="i-05"></a>
### I-05: Block line endings flipped by a lone `\r` ✅
- **Found:** 2026-10-20, by the new arbitrary-bytes property test (minimal input `[13]`).
- **Cause:** the line ending was re-derived from the whole file on each update. Foreign `\r`
  plus our `\n` separator looked like CRLF next time.
- **Fix:** reuse the existing block's own line ending on update and removal. The regression
  case is committed in `crates/kiln-platform/proptest-regressions/hosts.txt`.

## Tooling and process

<a id="i-06"></a>
### I-06: No Rust toolchain / GNU toolchain couldn't link ✅
- **Cause:** Rust wasn't installed. A session-local `x86_64-pc-windows-gnu` toolchain was tried
  (MSVC Build Tools were absent). Its bundled `dlltool` needs an assembler (`as.exe`) that isn't
  shipped, so `windows-sys` and `getrandom` (raw-dylib) failed to link.
- **Fix:** the user installed VS 2022 Build Tools (C++ workload, Windows SDK 10.0.26100) and
  rustup with the MSVC host. The scratch toolchain was deleted. See [dev-setup.md](dev-setup.md).

<a id="i-07"></a>
### I-07: Git Bash path conversion broke OpenSSL ✅
- **Cause:** MSYS converts arguments that look like POSIX paths, so `-subj "/CN=blog.test"`
  became a Windows path and leaf certificates were silently not created.
- **Fix:** set `MSYS_NO_PATHCONV=1` for such commands.

<a id="i-08"></a>
### I-08: Git Bash environment quirks ✅
- Windows paths with `C:` can't go into `PATH` from bash; use `/c/...` form.
- From bash-launched PowerShell, `whoami` resolved to MSYS's `/usr/bin/whoami`. Call
  `%windir%\System32\whoami.exe` explicitly.

<a id="i-09"></a>
### I-09: rustup auto-install deprecation warning 🔶 (minor)
- rustup auto-installs the toolchain pinned in `rust-toolchain.toml` but warns that this is
  deprecated. **Action:** document `rustup toolchain install` in dev-setup (done) and in CI, if
  runners start failing.

<a id="i-10"></a>
### I-10: CI deprecation and push rejection ✅
- `actions/checkout@v4` ran on deprecated Node 20, so it was bumped to v5. Dependabot then opened
  v7, which was merged.
- A push was rejected after that merge. Fixed by fetching, checking the remote diff (CI only),
  and rebasing the local commit before pushing.

## Spike 01: web server

<a id="i-11"></a>
### I-11: Herd kills every `nginx.exe` when it starts 📌
- **Found:** 2026-10-20. The spike's nginx died twice while idle. Herd's log showed
  `[nginx] killed leftover nginx.exe processes` at each startup (Herd auto-updated
  2.0.0 → 2.0.2 during the spike). The kill is by image name, not path; our `php-cgi.exe` survived.
- **Rule:** Kiln ships server binaries under Kiln-specific names (e.g. `kiln-caddy.exe`).
  Coexistence tests must include "Herd restarts while Kiln runs". (Task P1-06, P1-14.)

<a id="i-12"></a>
### I-12: Schannel hard-fails revocation for local CAs 📌
- **Found:** spike 01. Git's curl and System32 curl failed with "revocation status unknown".
  Local CAs have no CRL/OCSP endpoint. Browsers soft-fail.
- **Rule:** `kiln doctor` and the docs explain it, and suggest `--ssl-revoke-best-effort` /
  equivalent for Schannel-based tools. (P1-12.)

<a id="i-13"></a>
### I-13: Laravel file sessions distorted benchmarks ✅
- **Cause:** every load-generator request created a session file (31,179 files), and Laravel's
  session GC scans the directory, cutting throughput about 10×.
- **Fix:** benchmarks use the `array` session driver. Possible `kiln doctor` hint for apps
  hammered by tests.

<a id="i-14"></a>
### I-14: Synchronized php-cgi recycling causes outages 📌
- **Cause:** equal `PHP_FCGI_MAX_REQUESTS` made all workers exit together. The proxy's passive
  health check (nginx `max_fails=1 fail_timeout=10s`, Caddy `fail_duration`) quarantined every
  worker for 10 s, so all requests failed even though the workers were back within milliseconds.
- **Rule:** the supervisor staggers recycling, and the proxy doesn't quarantine local workers
  (nginx `max_fails=0`; Caddy: no `fail_duration`). Verified 0 errors over about 100 recycles.
  (P1-09.)

<a id="i-15"></a>
### I-15: Caddy `lb_retries` cap left errors 📌
- **Rule:** bound retries by time (`lb_try_duration 2s`, `lb_try_interval 50ms`), never by
  `lb_retries`. That took errors from about 9% to 0. (P1-06.)

<a id="i-16"></a>
### I-16: Caddy binds all interfaces by default 📌
- **Rule:** generated config sets `default_bind 127.0.0.1` (plus `::1`). A snapshot test asserts
  every listener is loopback. (P1-06.)

<a id="i-17"></a>
### I-17: Unknown hosts aren't rejected 📌
- Caddy returns an empty 200, and nginx serves its first server block.
- **Rule:** an explicit catch-all returns 404 with a "run `kiln link`" page. (P1-06.)

<a id="i-18"></a>
### I-18: Caddyfile syntax gotchas ✅
- Blocks can't be inline (`x { y }` on one line). `persist off` is JSON-only (Caddyfile uses
  `persist_config off`). A site with plain-HTTP addresses can't carry `tls`, so split it and use
  a snippet. This is another reason Kiln generates **JSON**, not Caddyfiles ([ADR 0005](adr/0005-web-server.md)).

<a id="i-19"></a>
### I-19: nginx on Windows is slow on static files 📌 (→ Caddy)
- 1 MB file: ~68 req/s for nginx vs ~1,670 for Caddy. Known limits: `select()`, one effective
  worker, ~1,024 connections. Input to choosing Caddy.

<a id="i-20"></a>
### I-20: Caddy admin API reachable by any local process 🔶
- `localhost:2019` over TCP lets any local process reconfigure the server.
- **Next:** verify Caddy admin over a Unix socket on Windows. Otherwise use a random port plus
  `enforce_origin` plus an OS guard. (P1-07.)

## Spike 02: Windows DNS

<a id="i-21"></a>
### I-21: Sandbox `LogonCommand` never runs with `wsb start` ✅
- No interactive logon happens. **Fix:** run tests with `wsb exec -r System`, which is a superset
  of the elevated-admin context `kiln-helper` uses.

<a id="i-22"></a>
### I-22: `R` is a PowerShell alias ✅
- A helper named `R` resolved to `Invoke-History` (aliases beat functions). Renamed to `Rec`.

<a id="i-23"></a>
### I-23: Mapped Sandbox folders are locked ✅
- Scripts in a mapped folder can't be edited while the Sandbox runs. Stop it (`wsb stop`), edit,
  restart.

<a id="i-24"></a>
### I-24: Guarded deletes ✅ (by design)
- A safety check blocks `rm -f $VAR/*` (it could expand to `/`) and removal of top-level folders
  such as `D:\VMs`. Use literal, specific paths. The empty `D:\VMs` folder was left for the user
  to remove.

<a id="i-25"></a>
### I-25: Rename-based replacement drops explicit ACEs 📌
- **Evidence:** with an explicit `Deny Write` ACE on `hosts`, temp + `ReplaceFileW` kept it, while
  temp + rename (`std::fs::rename`) removed it.
- **Rule:** the hosts writer uses `ReplaceFileW` and never touches ACLs. ([ADR 0006](adr/0006-windows-dns-strategy.md), P1-03.)

<a id="i-26"></a>
### I-26: Read-only, locked or missing hosts file 📌
- Read-only: every strategy fails, even as SYSTEM. **Rule:** refuse with guidance; clearing the
  attribute is a journaled opt-in.
- Sharing violation (os error 32): **rule:** retry with backoff, never write partially.
- Missing: **rule:** create it, with a receipt that deletes it on revert. (P1-03.)

<a id="i-27"></a>
### I-27: Defender is off in Windows Sandbox ✅
- Detection tests there prove nothing. Retested in the Hyper-V VM with real-time protection on:
  0 detections for 51 names.

<a id="i-28"></a>
### I-28: D: nearly full during the VM install ✅
- The VM disk grew to 20 GB with 2.8 GB left on D:. The VM was paused immediately, deleted, and
  rebuilt on E: (192 GB free). D: is back to ~27 GB. `new-vm.ps1` now defaults to `E:\VMs`.

<a id="i-29"></a>
### I-29: `Move-VMStorage` needs full admin ✅ (worked around)
- Hyper-V Administrators can create and run VMs but can't storage-migrate them. Rebuilt instead.

<a id="i-30"></a>
### I-30: Group membership needs a new sign-in ✅
- Adding the user to Hyper-V Administrators only takes effect after signing out and in.

<a id="i-31"></a>
### I-31: Execution policy Restricted in a fresh VM ✅
- **Fix:** `Set-ExecutionPolicy -Scope Process Bypass` per remote session (not machine-wide).

<a id="i-32"></a>
### I-32: `$p` vs `$P` ✅
- PowerShell variable names are case-insensitive: `$p = Start-Process …` overwrote the
  directory variable `$P`, which broke the WireGuard phase. Renamed to `$msiProc`.

<a id="i-33"></a>
### I-33: Headless Edge needs a real user context ✅
- From PowerShell Direct (session 0) Edge prints nothing, and with an elevated token it exits
  ("Edge is running elevated"). **Fix:** a one-off scheduled task in the interactive session,
  `-RunLevel Limited`. Also, `<` / `>` in a data URL broke the `cmd` line; use real URLs.

<a id="i-34"></a>
### I-34: Checkpoint taken mid-setup ✅ (lesson)
- The `clean-install` checkpoint captured the temporary `defaultuser0` session. After a later
  guest reboot the test credentials were rejected.
- **Rule:** checkpoint only after the test user is signed in on the console and the boot time is
  stable across checks a few minutes apart.

<a id="i-35"></a>
### I-35: Not enough host memory to restore ✅
- A 4 GB checkpoint restore failed on the 16 GB host while Windows Update was busy. It succeeded
  on retry (5.4 GB free). Keep test VMs at ≤ 4 GB, or close heavy apps during VM tests.

<a id="i-36"></a>
### I-36: Rebuilt VM has no bootable OS 🔶
- The third and fourth builds wrote ~20 GB, but the firmware reports "boot loader did not load
  an operating system" (diagnosed with `vm/screen.ps1`). The first two builds with the same
  script worked. Suspects: host memory pressure during setup ([I-35](#i-35)), and the
  DVD → PXE → disk boot order (PXE removed in the script).
- **Impact:** only blocks the optional recheck ([I-39](#i-39)). Spike 02's decision doesn't depend on it.
- **Next:** one rebuild with the host idle, then inspect Setup logs from the VHDX if it fails
  again (needs an elevated mount).

<a id="i-37"></a>
### I-37: NRPT + stub fails offline and under a full-tunnel VPN 📌
- `.test` names served by a loopback stub stopped resolving entirely with the NIC unplugged
  (reproduced twice) and under a WireGuard full tunnel with pushed DNS. The hosts block kept
  working in both cases.
- **Decision:** NRPT rejected for Phase 2 ([ADR 0006](adr/0006-windows-dns-strategy.md)).

<a id="i-38"></a>
### I-38: `ReplaceFileW` needs `unsafe` ✅
- The workspace forbids `unsafe_code`, but the hosts writer needs `ReplaceFileW` (Win32 FFI).
- **Investigated (2026-10-21):** no safe wrapper exists. `atomicwrites` 0.4, `atomic-write-file`
  0.3 and `staged_file` 0.5 all replace by rename (source grep: 0 uses of `ReplaceFile`), which
  I-25 showed drops explicit ACEs.
- **Resolved by [ADR 0008](adr/0008-unsafe-confined-to-kiln-sys.md):** `unsafe` is allowed only
  in the new `kiln-sys` crate, under stricter lints (`undocumented_unsafe_blocks`,
  `multiple_unsafe_ops_per_block`, `unsafe_op_in_unsafe_fn` all deny). A CI step fails if any
  other crate drops the workspace lints. `kiln_sys::replace_file` / `write_replacing` ship with
  real-Windows tests showing an explicit deny ACE is kept, and that a plain rename drops it.
- **Side finding:** a workspace-level `forbid` can't be relaxed per item, so `kiln-sys` defines its
  own `[lints]` table instead of `workspace = true`.

<a id="i-39"></a>
### I-39: Edge × DNS-over-HTTPS matrix unreliable 🔶
- The main run's DoH rows came from a cold interactive session (first Edge launches failed). A
  3-trial matrix and a second WireGuard trial are scripted (`part2.ps1 -Phase matrix|wgcheck`)
  but waiting on a working VM ([I-36](#i-36)). Doesn't affect the hosts-block decision.

## Phase 1

<a id="i-40"></a>
### I-40: ACL test failed only on the GitHub Windows runner ✅
- **Found:** 2026-10-21, CI run for `f9c3f4b`. `windows_keeps_explicit_aces_where_rename_drops_them`
  failed at its *setup* assertion: `icacls /deny` reported success, but the test's check (SDDL
  from PowerShell `Get-Acl`) didn't see the deny ACE. It passed on the dev machine. The product
  code (`ReplaceFileW`) was never reached.
- **Cause:** the test's detection depended on PowerShell output in the runner environment, which
  was not reliable there.
- **Fix:** read the security descriptor with `icacls <file> /save <out>` (UTF-16 SDDL,
  locale-independent, no PowerShell), match an explicit `(D;;…;;;BG)` ACE, and include the SDDL
  in every assertion message. As a side effect the test runs in 0.1 s instead of 2.3 s.
- **Lesson:** OS-behaviour tests must not depend on shell output formats. Prefer tools with
  machine-readable output, and always print the observed value on failure.

<a id="i-41"></a>
### I-41: Over-the-shoulder elevation not yet verified 🔶
- When a standard user approves UAC with *another* account's admin credentials, `kiln-helper`
  runs as that admin. The helper builds the pipe DACL from the **client process's** token
  user (`process_user_sid(client_pid)`), so the right user gets access. But opening a
  different user's process token can be denied by that token's default DACL.
- **Next:** test on a VM with a standard user plus a separate admin account. If the token query
  is denied, fall back to the SID the client passes as an argument, validated against the
  pipe client's PID after connect. Tracked for P1-15.

<a id="i-42"></a>
### I-42: UAC launch is manual-test only ✅ (verified manually 2026-10-21)
- `kiln_sys::win::spawn_elevated` (`ShellExecuteExW` `runas`) shows a consent prompt, which CI
  can't click. Everything *after* launch (pipe, identity check, MAC, operations, the real
  binary) is covered automatically in `crates/kiln-helper/tests/session.rs`.
- **Manual check:** `cargo test -p kiln-sys -- --ignored elevation_runs_and_reports_the_exit_code`
  (approve the prompt; expects exit code 7). Run it once per Windows release and before 1.0.
- **Manual end-to-end (since P1-03):** `cargo build -p kiln-cli -p kiln-helper`, then
  `target\debug\kiln.exe hosts set demo` → one UAC prompt → `ping demo.test` resolves to
  127.0.0.1 → `target\debug\kiln.exe undo <n>` → one UAC prompt → the hosts file is
  byte-identical to before (compare `certutil -hashfile` output).
- **Result 2026-10-21 (dev machine, Windows 11 Pro, with Laravel Herd's hosts block present):**
  - Baseline: SHA-256 `4387EA88…49BE90`, 2,155 bytes, no Kiln block, `demo.test` unresolved.
  - `kiln hosts set demo --yes`: one UAC prompt, done in 3 s, recorded as operation #1. After
    it, `demo.test` resolved to 127.0.0.1 (.NET and `ping`). The original 2,155 bytes were an
    exact prefix with Kiln's block appended, Herd's block was untouched, and the helper process
    exited.
  - `kiln undo 1 --yes`: one UAC prompt. The hash is back to `4387EA88…49BE90`, 2,155 bytes,
    `demo.test` unresolved, the file's security descriptor (including an explicit SYSTEM ACE)
    is identical, and the journal shows operation #1 as `Reverted`.
  - Side effect: Kiln's own state database now exists at `%APPDATA%\Kiln\state.db` (one
    reverted operation).

