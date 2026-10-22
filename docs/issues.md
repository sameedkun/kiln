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
| [I-11](#i-11) | coexistence | Herd kills every `nginx.exe` on the machine when it starts | ✅ (Kiln runs `kiln-caddy.exe`; restart test in P1-15) |
| [I-12](#i-12) | TLS | Windows Schannel clients hard-fail revocation checks against a local CA | 📌 |
| [I-13](#i-13) | benchmark | Laravel file sessions under synthetic load distorted throughput about 10× | ✅ |
| [I-14](#i-14) | runtime | Synchronized php-cgi recycling plus passive health checks caused a 10 s outage | 📌 (staggered limits + no passive checks in P1-09) |
| [I-15](#i-15) | Caddy | `lb_retries` capped retries and left ~9% errors during recycling | 📌 (time-bounded retries in P1-09) |
| [I-16](#i-16) | Caddy | Caddy binds all interfaces by default | 📌 (enforced in `kiln-web`, P1-06) |
| [I-17](#i-17) | web | Unknown hosts get a 200 (Caddy) or the first site (nginx) | 📌 (catch-all 404 in `kiln-web`, P1-06) |
| [I-18](#i-18) | Caddy | Caddyfile syntax gotchas (inline blocks, `persist`, TLS on HTTP sites) | ✅ |
| [I-19](#i-19) | nginx | nginx on Windows is 2–25× slower on static files | 📌 (→ Caddy) |
| [I-20](#i-20) | security | Caddy admin API listens on TCP `localhost:2019`, reachable by any local process | ✅ (Unix socket, ADR 0012) |
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
| [I-43](#i-43) | CA | Trusting/untrusting in the real `CurrentUser\Root` store (Windows dialog) is manual-only | ✅ (manual run and browser check passed 2026-10-21) |
| [I-44](#i-44) | platform | Renames inside the user profile fail with os error 17 on the dev machine; `ReplaceFileW` gets Access denied there (cause: MSIX virtualization, I-45) | ✅ (workaround: `rename_replace`) |
| [I-45](#i-45) | environment | Commands run from the Claude desktop app see a virtualized AppData; Caddy can't store certificates there; manual checks must use a normal terminal | 📌 (confirmed; `kiln doctor` detects it) |
| [I-46](#i-46) | Caddy | On Windows, the first admin request after each config load gets an EOF (admin listener swap) | ✅ (retry in the admin client) |
| [I-47](#i-47) | Caddy | A live reload can close a brand-new connection without a response (about 1 per 3–7 reloads under saturating load, Windows) | 🔶 (accepted; recheck on Linux and newer Caddy) |
| [I-48](#i-48) | downloads | A 60 s body timeout failed a real 34 MB PHP download on a slow line (ureq's body timeout is total, not idle) | ✅ (1 h total; no stall detection yet) |
| [I-49](#i-49) | PHP | Renaming the extracted PHP folder into place failed with Access denied | ✅ (extract in place behind an incomplete-marker) |
| [I-50](#i-50) | Caddy | A request in flight during a Caddy reload can't be retried if its worker is recycling at that moment (502) | 🔶 (rare at real limits; accepted) |
| [I-51](#i-51) | supervisor | "Exited within 1 s" wrongly treated busy, healthy workers as crashed | ✅ (crash = non-zero exit) |
| [I-52](#i-52) | uninstall | A project inside Kiln's folder was deleted when LOCALAPPDATA used 8.3 short names (spelling compared, not paths) | ✅ (canonical paths; caught by a test before release) |
| [I-53](#i-53) | coexistence | An automatically picked port was saved like a chosen one, so it would never move again | ✅ (caught by a test before release) |
| [I-54](#i-54) | coexistence | Before the first `kiln web start`, `kiln link` printed URLs on port 80 although Kiln would pick 8080 | ✅ (found in the user's check) |

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
### I-11: Herd kills every `nginx.exe` when it starts ✅
- **Found:** 2026-10-20. The spike's nginx died twice while idle. Herd's log showed
  `[nginx] killed leftover nginx.exe processes` at each startup (Herd auto-updated
  2.0.0 → 2.0.2 during the spike). The kill is by image name, not path; our `php-cgi.exe` survived.
- **Rule:** Kiln ships server binaries under Kiln-specific names (e.g. `kiln-caddy.exe`).
  Coexistence tests must include "Herd restarts while Kiln runs". (Task P1-06, P1-14.)
- **Status (P1-14):** Kiln's server has been `kiln-caddy.exe` since P1-06; `kiln doctor` explains
  this when Herd is installed ([ADR 0019](adr/0019-coexistence.md)). The "Herd restarts while
  Kiln runs" test is part of P1-15.

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
### I-20: Caddy admin API reachable by any local process ✅
- `localhost:2019` over TCP lets any local process reconfigure the server.
- **Next:** verify Caddy admin over a Unix socket on Windows. Otherwise use a random port plus
  `enforce_origin` plus an OS guard. (P1-07.)
- **Resolved 2026-10-21 (P1-07, [ADR 0012](adr/0012-caddy-admin-channel.md)):** the admin endpoint
  listens only on a Unix socket in Kiln's per-user run directory (`admin.sock`), which works
  on Windows 10 1803+. There is no TCP admin port, and other users can't open the socket.

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


<a id="i-43"></a>
### I-43: Real `Root` store trust is a manual check ✅ (verified manually 2026-10-21)
- Adding to or removing from `Cert:\CurrentUser\Root` makes Windows show its own security
  dialog, which CI can't click. All logic is tested automatically against private per-test
  stores (`KilnTest-<pid>-<n>`, deleted afterwards): trust + undo from the journal, a
  pre-existing cert left alone, foreign roots refused by the `kiln-sys` wrapper.
- **Manual check:** `kiln ca init` → `kiln ca trust` (choose **Yes** in the Windows dialog) →
  `Get-ChildItem Cert:\CurrentUser\Root | ? Subject -like '*Kiln Local Development CA*'`
  shows it → `kiln undo <n>` (confirm the removal dialog) → gone. Also confirm Edge or Chrome
  shows a `.test` page as secure once P1-06 serves one.
- **Result 2026-10-21 (dev machine, Windows 11 Pro):**
  - Baseline: 50 certificates in `CurrentUser\Root`, no Kiln root (thumbprints saved).
  - `kiln ca init` created the `.test`-only root (SHA-1 `27CF1F33…05ECF1`, expires 2029-10-20)
    and a 30-day intermediate in `%APPDATA%\Kiln\ca`; the root key is DPAPI-encrypted. The
    first attempt failed with os error 17 ([I-44](#i-44)).
  - `kiln ca trust --yes`: one Windows security dialog (Yes), recorded as operation #2. The store
    held 51 certificates, including the Kiln root with the critical name constraint
    `Permitted: DNS Name=test`.
  - `kiln undo 2 --yes`: one removal dialog (Yes), no UAC. The store is back to 50 certificates,
    and the thumbprints are identical to the baseline.
  - Still to do: the browser check once P1-06 serves a `.test` page.
  - Caveat found later ([I-45](#i-45)): this run happened inside the Claude desktop app's MSIX
    container. The hosts file (System32) isn't virtualized, so I-42 stands. The trust check
    should be repeated once from a normal terminal, together with the browser check.
- **Repeated from a normal terminal by the user, 2026-10-21 (with P1-06):**
  - A fresh state: the first operation was #1, so nothing from the app container was visible.
  - `kiln ca init` created a new root (SHA-1 `e6201312…e744d2f4`).
  - `kiln ca trust`: one Windows dialog, then operation #1.
  - **Chrome showed `https://shop.test:8443/` as secure, with no warning.** The certificate was
    issued by Caddy from Kiln's intermediate under the name-constrained root.
  - `kiln undo 1`: removed, no UAC.

<a id="i-44"></a>
### I-44: Renames inside the user profile fail with os error 17 ✅ (worked around)
- **Symptom:** the first `kiln ca init` failed with `root.key.protected: The system cannot move
  the file to a different disk drive. (os error 17)`. The temp file and the target were in the
  same folder (`%APPDATA%\Kiln\ca`).
- **Findings:** `std::fs::rename` (plain `MoveFileExW`) fails this way in `%APPDATA%` and
  `%LOCALAPPDATA%`, inside and outside the agent sandbox, but works in `%TEMP%`.
  `MoveFileExW` with `MOVEFILE_COPY_ALLOWED` works, which is why .NET `File.Move` was fine.
  `ReplaceFileW` fails with "Access is denied" (os error 5) in the same folders.
- **Root cause (found during P1-06):** MSIX AppData virtualization, not Kiln and not Windows in
  general. The commands ran as children of the Claude desktop app
  (`C:\Program Files\WindowsApps\Claude_…\Claude.exe`), an MSIX-packaged app. Windows
  redirects such processes' AppData writes into
  `%LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\{Local,Roaming}` and merges that with the
  real folder. A rename between the two layers looks like a move to another volume. Both
  `…\LocalCache\Local\Kiln` and `…\LocalCache\Roaming\Kiln` exist on the dev machine. Kiln run
  from a normal terminal isn't affected. Anyone who runs Kiln from another packaged app's
  terminal would be, so the fix stays. Details and consequences: [I-45](#i-45).
- **Fix:**
  - New `kiln_sys::rename_replace`: `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`, retried
    with `COPY_ALLOWED` on `ERROR_NOT_SAME_DEVICE`. Used by `kiln-pki` key writes and
    `kiln-state`'s legacy-journal archive.
  - `clippy.toml` bans `std::fs::rename` (`disallowed-methods`), so new code can't reintroduce it.
  - A Windows regression test renames inside `%LOCALAPPDATA%`.
  - `write_replacing` (`ReplaceFileW`) is still used only for the system hosts file, where it
    works (I-42); its doc comment warns against using it in the profile.
- **Caveat:** the `COPY_ALLOWED` fallback is copy + delete, so it is not atomic. A crash in
  between leaves both files. That is acceptable for Kiln's write-once CA files and the one-time
  journal archive, but not for files rewritten in place.

<a id="i-45"></a>
### I-45: Commands run from the Claude desktop app live in an MSIX container 📌
- **Found:** 2026-10-21, P1-06 manual check. Over plain HTTP, `kiln web start` served the sites
  correctly. HTTPS failed because Caddy couldn't store the `shop.test` certificate:
  `failed storage check: failed to rename …\AppData\Local\Kiln\caddy\storage\…`. Caddy's Go
  file storage uses a plain rename, the same failure as [I-44](#i-44).
- **Cause:** see I-44. Every process started from the Claude desktop app inherits its MSIX
  AppData virtualization. That includes `kiln`, `kiln-caddy` and the PowerShell used to verify
  results. The automated real-Caddy test passed because it runs under `%TEMP%`, which isn't
  virtualized.
- **Consequences:**
  - The Kiln state, CA, sites and Caddy binary created during agent sessions are stored in
    `%LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\…`. A normal terminal sees none of them, and
    `kiln` run there starts fresh.
  - The hosts file (System32) isn't virtualized, so [I-42](#i-42) stands.
  - Whether `CurrentUser\Root` changes are virtualized is unverified. The dialog and store
    write are brokered by the system, so they probably aren't. [I-43](#i-43) gets repeated from
    a normal terminal.
  - Investigating from outside the container (starting a process through WMI, reading the
    package's `LocalCache`) was blocked in the agent session. That was the right call; the user
    runs the checks instead.
- **Confirmed 2026-10-21:** the user ran the P1-06 script from a normal PowerShell.
  - Kiln started with an empty journal (operation #1), so the container's state was invisible.
  - Caddy stored its certificates without error, and HTTPS worked in Chrome.
- **Rule:**
  - Manual checks that touch AppData run from a normal terminal, not from the Claude app.
  - `kiln doctor` (P1-12) should warn when Kiln runs in a packaged app's container, because
    its files would then be invisible to the rest of the system.
- **More from P1-07 (2026-10-21; the same steps passed from a normal terminal):**
  - `AF_UNIX` sockets don't work in the redirected AppData either. Caddy creates `admin.sock`,
    then fails with `unable to set permissions … The file cannot be accessed by the system`.
    The file can't be removed from inside the container. The same steps work in `%TEMP%`,
    so the P1-07 check also runs from a normal terminal.
  - `GetCurrentPackageFullName` returns no package for these processes, so Kiln can't detect
    the container through package identity. Detection stays an open question for
    `kiln doctor`.

## P1-07: Caddy admin channel

<a id="i-46"></a>
### I-46: First admin request after a config load gets an EOF on Windows ✅
- **Found:** P1-07 spike. On every `/load`, Caddy replaces its admin listener ("stopped
  previous server" → "admin endpoint started"). The next admin request then gets an EOF before
  any response; the one after it succeeds. `caddy reload` doesn't retry, so it failed every
  other time.
- **Fix:** Kiln's admin client retries a request up to 6 times, 50 ms apart, when the
  connection fails or closes before a response. Every request Kiln sends is safe to repeat.
  Verified across 20 reloads in a row.

<a id="i-47"></a>
### I-47: A live reload can drop a brand-new connection 🔶
- **Measured (Windows 11, Caddy 2.11.7):** a Rust client opened about 1,400 new connections
  per second while Kiln reloaded the config.
  - About one connection per 3–7 reloads was accepted and then closed with no response.
  - Changing a route and adding/removing the HTTPS server behaved the same: 3–7 per 20.
  - No reloads: 0 of 2,700. Nothing was ever served wrong (no 404/5xx, no wrong site).
  - For comparison, a restart is about 540 ms of refused connections per change.
- **Decision:** accepted for a local dev server. Reloads only happen when the user changes a
  site, so a person rarely hits it, and refreshing the page fixes it.
  - The end-to-end test allows at most one empty close per reload and nothing else.
- **Next:** measure on Linux (SO_REUSEPORT listener sharing) with P1-04, and recheck on newer
  Caddy releases. Report upstream if it still happens.

## P1-08: downloads and PHP runtimes

<a id="i-48"></a>
### I-48: Download body timeout was total, not idle ✅
- **Found:** the first real `kiln php install 8.4` stopped at 6.7 of 33.7 MB with `timeout:
  receive body` after 60 s. The connection was delivering about 110 KB/s.
  `timeout_recv_body` in ureq 3 limits the **whole** body, not the time between bytes.
- **Fix:** the body timeout is now 1 hour (`kiln_fetch::BODY_TIMEOUT`), enough for the 512 MB
  cap on a slow line. The retry downloaded the file in about 5 minutes. Connect (20 s) and
  response-header (60 s) timeouts are unchanged.
- **Open:** there's no stall detection, so a dead connection can wait out the hour. Ctrl+C is
  safe, because a partial file never enters the cache. Add an idle watchdog if users hit it.

<a id="i-49"></a>
### I-49: Renaming the finished PHP folder failed with "Access is denied" ✅
- **Found:** the same first install. Download, SHA-256 check, extraction, `php.ini` and
  `php -v` all passed. The final `rename(8.4.27.partial → 8.4.27)` then failed with os error 5.
- **Likely causes:**
  - Real world: antivirus (Defender) still holding the freshly written `.exe`/`.dll` files
    open for scanning, which blocks renaming the folder they're in.
  - In the agent session: the MSIX AppData virtualization from I-44/I-45. Directory renames
    across its layers fail, and `COPY_ALLOWED` can't help with folders.
- **Fix:** no folder rename. Kiln extracts straight into the final folder, which holds a
  `.kiln-install-incomplete` marker until the start-up check passes. `is_installed` requires
  `php.exe` **and** no marker, an interrupted install is redone, and a failure deletes the
  folder. The retry succeeded in 14 s from the verified cache.

## P1-09: php-cgi supervisor

<a id="i-50"></a>
### I-50: A Caddy reload can turn one request into a 502 if a worker recycles at that moment 🔶
- **Found:** the P1-09 end-to-end test switches pools live (old and new workers side by side,
  Caddy reloaded to the new upstreams, the old pool retired after a grace period). It saw
  exactly one 502 in most switches, although steady-state recycling had none.
- **Cause, from Caddy's log:** `dialing backend: … actively refused` after about 50 ms, or
  `wsarecv: … forcibly closed` after about 3 ms. In steady state Caddy retries both for up to
  2 s. But on a config reload Caddy cancels the previous config's context. A request still
  being handled by that config stops retrying after one `try_interval`, and fails if its
  worker happened to be restarting at that moment.
- **Not the cause:** retiring the old pool too early. A 5 s grace (`RETIRE_GRACE`) is in
  place and the 502s remained. The grace stays, because it protects long requests on the old
  workers.
- **Impact:** needs a reload *and* a recycle at the same instant.
  - The test recycles every 10–16 requests on purpose, so it hits this almost every time.
  - With the real limit (500–875 per worker), it's a fraction of a millisecond per reload.
  - Reloads only happen when the user changes sites or PHP settings.
- **Decision:** accepted. The test allows at most two failed requests (502 or empty close,
  I-47) for its one reload, and nothing else.
- **Next:** report upstream (letting a draining config finish its retries), and recheck on
  Linux with P1-04.

<a id="i-51"></a>
### I-51: Healthy workers recycling within a second were treated as crashed ✅
- **Found:** the first real run of the P1-09 test. With a recycle limit of 10, workers served
  their requests and exited cleanly within a second. The supervisor's rule ("exited within
  1 s of starting = crash") backed them off for 0.25–1 s, and the log showed "exited right
  after starting" for healthy workers.
- **Fix:** the exit code decides. `php-cgi` exits **0** at `PHP_FCGI_MAX_REQUESTS`, and that
  is restarted at once. Only a non-zero exit is a failure, and only a quick failure backs off.
  New unit tests cover a busy worker recycling within 200 ms and a late failure.
- **Result:** 400 requests across 25–34 different php-cgi processes (about 24–31 recycles)
  with 0 failures, in every one of 12 runs.

## P1-12: kiln doctor

- **I-45 detection (2026-10-22):** `kiln doctor` now flags the packaged-app container by walking
  the process's ancestors.
  - Windows reports a packaged app's **real** image path. On the dev machine apps are
    installed to drive E:, so the path is `E:\WindowsApps\Claude_…\app\claude.exe`, not the
    `C:\Program Files\WindowsApps\…` that WMI shows.
  - The Claude Code process itself runs from the redirected AppData,
    `…\AppData\Local\Packages\Claude_<id>\LocalCache\Roaming\…`.
  - The rule therefore matches a `WindowsApps` folder on any drive, or a
    `Packages\…\LocalCache` path.
  - Ancestors are opened for query only. Asking for terminate rights as well was refused by
    the packaged app's process, which first hid the match.

## P1-13: history, undo, uninstall

<a id="i-52"></a>
### I-52: Uninstall compared path spellings, so a project inside Kiln's folder could be deleted ✅
- **Found:** the first CI run of P1-13. On GitHub's Windows runner `LOCALAPPDATA` is spelled
  with 8.3 short names (`C:\Users\RUNNER~1\…`). `kiln link` stores a project under its real
  long path (`C:\Users\runneradmin\…`).
  - The safety rule "never delete a Kiln folder that holds a project" compared the two
    spellings with `starts_with`. It saw no match and **deleted the folder, project included**.
  - The sandbox test that guards this rule failed, so it was never released.
- **Fix:** Kiln's folders and project paths are both canonicalized (real long path, no `\?\`)
  before comparing. The state-folder check in the executor uses the same canonical form.
- **Test:** the sandbox now spells its root with 8.3 short names (via `cmd`'s `%~s`, passed
  with `raw_arg`, because `arg` escapes quotes in a way `cmd` doesn't understand). Without the
  fix it reproduces locally (the project is deleted); with it, the project survives.
- **Rule:** any destructive path check compares canonical paths, never spellings.

## P1-14: coexistence

<a id="i-53"></a>
### I-53: An automatically picked port was saved like a chosen one ✅
- **Found:** a unit test for P1-14, before the first commit. With Herd on port 80, the first
  `kiln web start` picked 8080 and saved it, but without the marker saying Kiln picked it.
  - A saved port with no marker means "chosen by the user before automatic ports existed".
  - So the next start treated 8080 as the user's choice: if 8080 became busy, it would fail
    instead of moving to the next free port.
- **Fix:** whenever a port is Kiln's to pick, the marker (`web.http_port_auto` /
  `web.https_port_auto` = `1`) is written along with it.
- **Rule:** a setting that is inferred when missing must be written explicitly the first time
  Kiln acts on the inference.

<a id="i-54"></a>
### I-54: URLs printed before the first start assumed port 80 ✅
- **Found:** the user's P1-14 check from a normal terminal, with Herd on port 80. Before the
  first `kiln web start`, `kiln link phpdemo` printed `http://phpdemo.test/`, and the note for
  `herd-studio.test` said Kiln's site was `http://herd-studio.test/`. That URL opens **Herd's**
  site. The start then picked 8080.
- **Cause:** with no saved port, URLs used the plain default (80) instead of the port the
  start would pick.
- **Fix:** `planned_port`: the saved port, or else the one `kiln web start` would pick now.
  Printed URLs (`link`, `park`, `sites`) and `kiln doctor`'s port checks use it.
- **Rule:** anything shown before an action must come from the same decision the action makes.
