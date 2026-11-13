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
| [I-11](#i-11) | coexistence | Herd kills every `nginx.exe` on the machine when it starts | ✅ (Kiln runs `kiln-caddy.exe`; Herd restart verified in P1-15) |
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
| [I-36](#i-36) | infra | Rebuilt VM installs (~20 GB written) but has no bootable OS | ✅ (DVD prompt and boot order fixed in P1-15) |
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
| [I-48](#i-48) | downloads | A 60 s body timeout failed a real 34 MB PHP download on a slow line (ureq's body timeout is total, not idle) | ✅ (1 h total; gives up after 1 min without data) |
| [I-49](#i-49) | PHP | Renaming the extracted PHP folder into place failed with Access denied | ✅ (extract in place behind an incomplete-marker) |
| [I-50](#i-50) | Caddy | A request in flight during a Caddy reload can't be retried if its worker is recycling at that moment (502) | 🔶 (rare at real limits; accepted) |
| [I-51](#i-51) | supervisor | "Exited within 1 s" wrongly treated busy, healthy workers as crashed | ✅ (crash = non-zero exit) |
| [I-52](#i-52) | uninstall | A project inside Kiln's folder was deleted when LOCALAPPDATA used 8.3 short names (spelling compared, not paths) | ✅ (canonical paths; caught by a test before release) |
| [I-53](#i-53) | coexistence | An automatically picked port was saved like a chosen one, so it would never move again | ✅ (caught by a test before release) |
| [I-54](#i-54) | coexistence | Before the first `kiln web start`, `kiln link` printed URLs on port 80 although Kiln would pick 8080 | ✅ (found in the user's check) |
| [I-55](#i-55) | doctor | On Linux, `kiln doctor` called Kiln's own busy port 8080 free | ✅ (found in the user's VM check) |
| [I-56](#i-56) | process | Shell-quoted substitutions turned `\e` in a Windows path into an escape byte | ✅ (caught before commit) |
| [I-57](#i-57) | TLS | After the CA changed, Caddy kept serving site certificates from the old intermediate; browsers rejected the site though every store trusted the new root | ✅ (found in the user's VM check) |
| [I-58](#i-58) | dependencies | Two advisories during P1-04b: `ctutils` 0.4.3, and zbus 5's `zvariant` crates declared unmaintained | ✅ (ctutils 0.4.5; zbus 6) |
| [I-59](#i-59) | uninstall | With the keyring locked, `kiln uninstall` deleted the state folder and orphaned the CA key's keyring item | ✅ (state folder kept until the key is removed) |
| [I-60](#i-60) | web | On Windows, reading `kiln web start`'s output waited until the web server stopped (it inherited the output pipe) | ✅ (std handles kept from background processes) |
| [I-61](#i-61) | php | windows.php.net started listing rebuilds as `8.4.27-1`; the index parser rejected the whole index, so no PHP could be installed | ✅ (rebuild suffix accepted) |
| [I-62](#i-62) | php | WordPress needs `mysqli`: not enabled on Windows, missing from the static Linux builds | ✅ (enabled on Windows; Kiln's own Linux builds, ADR 0025) |
| [I-63](#i-63) | cli | Piping `kiln` into `head` panicked with "Broken pipe" once the reader exited | ✅ (quiet exit 141) |
| [I-64](#i-64) | durability | A registry value written just before a hard reset was lost (Windows writes the registry lazily) | ✅ (`RegFlushKey` after each write) |
| [I-65](#i-65) | php | Windows PHP kept PHP's 30 s / 128 MB development limits; a first WordPress load under Defender ran past 30 s | ✅ (same 60 s / 512 MB limits as Linux) |
| [I-66](#i-66) | node | nodejs.org started listing Node 27 alphas (`v27.0.0-alpha.0.0.0`) in `index.json`; the parser rejected the whole index, so no Node could be installed | ✅ (pre-releases skipped) |
| [I-67](#i-67) | sys | Replacing php.ini sometimes failed with "Access is denied" while another process briefly held it open | ✅ (brief retry) |
| [I-68](#i-68) | tests | In the Windows VM, the firewall prompt Vite set off covered the certificate dialog, and uninstall waited forever | ✅ (prompt off in the test VM) |
| [I-69](#i-69) | web | Right after `--secure`, the printed HTTPS URL failed for a moment while Caddy issued the certificate in the background | ✅ (kiln waits for it) |
| [I-70](#i-70) | node | The Laravel build failed when Bunny Fonts took longer than Node's 250 ms per-address connect limit | 📌 (Node's default; test step raises it) |
| [I-71](#i-71) | web | Caddy set up its own default CA for a `.internal` site and installed its root into the system and NSS trust stores | ✅ (`local` CA declared with `install_trust: false`) |
| [I-72](#i-72) | web | The first `kiln web start` never recorded which intermediate issued the site certificates, so the next site change restarted the server | ✅ (recorded whenever Caddy gets the CA) |
| [I-73](#i-73) | web | The restart after a CA change left sites unanswered for 8 s on Windows: it also restarted PHP and validated the new config while nothing was serving | ✅ (`WebServer::restart`: only Caddy, validated first; about 0.1 s) |
| [I-74](#i-74) | fetch | Downloads from GitHub stalled mid-file twice in a row in the Ubuntu VM; Kiln gave up after 60 s without data and had no retry or resume | ✅ (P3-04: a stalled or broken download resumes with a Range request, up to 3 times) |
| [I-75](#i-75) | services | Garnet's Lua scripts ignore `SELECT` and act on database 0, so Laravel's cache (database 1) breaks for `add`, tag flushes and the rate limiter: no Windows cache service until Garnet fixes it | open (upstream: microsoft/garnet#2258, fix in #2311) |
| [I-76](#i-76) | services | Services started without admin rights failed with `STATUS_DLL_INIT_FAILED` on an elevated machine: the restricted token kept a default DACL that granted only Administrators | fixed (P3-05) |
| [I-77](#i-77) | daemon | `bad_input_gets_an_error_not_a_hang` failed once on Linux CI: the daemon refused the oversized line and closed before the test finished writing it (broken pipe) | fixed (P3-05, test only) |
| [I-78](#i-78) | services | PostgreSQL answered "the database system is starting up" right after a restart: its port opens before it accepts sessions | fixed (P3-05) |
| [I-79](#i-79) | supervisor | Redis (an MSYS2 program) wrote nothing to its log: the supervisor gave programs append-only handles, which the MSYS2 runtime can't write through | fixed (P3-06) |
| [I-80](#i-80) | services | The MSYS2 Redis read `C:/…` config paths as relative and failed to start ("can't open config file '/cygdrive/…/C:/…'") | fixed (P3-06) |
| [I-81](#i-81) | fetch | MongoDB's Windows archive (989 MB) was refused: downloads were capped at 512 MB | fixed (P3-14) |
| [I-82](#i-82) | tools | A failure to write the web tools' `servers.json` made every service command fail | fixed (P3-15) |
| [I-83](#i-83) | uninstall | Uninstall kept Kiln's whole local folder because the phpMyAdmin site inside it counted as a project | fixed (P3-15) |
| [I-84](#i-84) | reverb | `composer require laravel/reverb` fails on a new Laravel 13 app: the app locks `guzzlehttp/psr7` 3.x, and Reverb 1.12 needs 2.x | worked around (P3-16): Kiln says to add `-W` |
| [I-85](#i-85) | dependencies | RUSTSEC-2026-0459 (CVE-2026-119375): the SQLite bundled by `rusqlite` 0.40 could return stale page-cache memory with a crafted `-wal` file; cargo-deny failed CI | fixed (P3-10): `rusqlite` 0.41, SQLite 3.54.1 |
| [I-86](#i-86) | design | With transparency off, the selected page was the same colour as the sidebar | ✅ |
| [I-87](#i-87) | design | A fade on the page's scroller also faded the page header inside it | 📌 (header beside the scroller) |
| [I-88](#i-88) | design | axe can't measure text contrast on translucent glass: it reports it as incomplete, not failed | 📌 (contrast measured from the design values) |
| [I-89](#i-89) | design | The tray popup's glass over a dark desktop left its quieter text near 3:1 in light mode | 📌 (Kiln's backdrop under the popup) |
| [I-90](#i-90) | design | An outlined stop square next to "Running" read as an empty checkbox | ✅ |
| [I-91](#i-91) | CI | GitHub Actions stopped starting jobs: the account's payments failed or its spending limit was reached | 🔶 (the user's account) |

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
### I-36: Rebuilt VM has no bootable OS ✅
- The third and fourth builds wrote ~20 GB, but the firmware reports "boot loader did not load
  an operating system" (diagnosed with `vm/screen.ps1`). The first two builds with the same
  script worked. Suspects: host memory pressure during setup ([I-35](#i-35)), and the
  DVD → PXE → disk boot order (PXE removed in the script).
- **Impact:** only blocks the optional recheck ([I-39](#i-39)). Spike 02's decision doesn't depend on it.
- **Fixed in P1-15** (`tests/vm/windows/new-vm.ps1`, [ADR 0024](adr/0024-integration-test-vms.md) §6):
  - "Press any key to boot from DVD" is answered only right after power-on.
  - Both DVDs are ejected once the VM's heartbeat is OK, so later reboots can only come from
    the disk.
  - Dynamic memory (2–4 GB).
  - `kiln-win11` then built unattended on the first try (2026-10-23).
- **Later lessons from the same VM:**
  - A restored checkpoint keeps its old IP lease. After a host reboot the Default Switch
    moves to another subnet, so `run.ps1` releases and renews the lease and checks the
    connection first.
  - The guest's resolution changes between boots (1920×1200, then 1024×768). Click
    positions come from the guest in screen pixels and are never scaled from a screenshot.

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
- **Linux (2026-10-27, GitHub's Ubuntu runner):** the same event appeared there as a
  connection reset while connecting, which the test hadn't expected: a connection queued on
  the old listener at the handoff is reset. It passed in the two CI runs before.
  - The test now counts a failed connect or send like an empty close, within the same limit
    of one per reload.
- **Next:** measure the Linux rate, and recheck on newer Caddy releases. Report upstream if
  it still happens.

## P1-08: downloads and PHP runtimes

<a id="i-48"></a>
### I-48: Download body timeout was total, not idle ✅
- **Found:** the first real `kiln php install 8.4` stopped at 6.7 of 33.7 MB with `timeout:
  receive body` after 60 s. The connection was delivering about 110 KB/s.
  `timeout_recv_body` in ureq 3 limits the **whole** body, not the time between bytes.
- **Fix:** the body timeout is now 1 hour (`kiln_fetch::BODY_TIMEOUT`), enough for the 512 MB
  cap on a slow line. The retry downloaded the file in about 5 minutes. Connect (20 s) and
  response-header (60 s) timeouts are unchanged.
- **Stall detection added (2026-10-26):** `Fetcher::fetch` reads the body on a helper thread
  and gives up when no data arrives for a minute (`kiln_fetch::STALL_TIMEOUT`). It deletes the
  partial file and says the download stalled.
  - Small files fetched with `get_text` (release indexes) still rely on the 60 s
    response-header timeout and the body timeout.

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

## P1-04a: Linux

<a id="i-55"></a>
### I-55: `kiln doctor` on Linux called Kiln's own busy port free ✅
- **Found:** the user's check in the Ubuntu 24.04 VM. With Kiln's server running on 8080
  (PID 15289), `kiln doctor` reported `HTTP 127.0.0.1:8080 is free`.
- **Cause:** the list of listening sockets and their processes was only read on Windows
  (`GetExtendedTcpTable`). On Linux it was empty, so every port looked free, and automatic
  ports couldn't name who held one either.
- **Fix:** read `/proc/net/tcp{,6}` (listening sockets and their inodes), map inodes to PIDs
  through `/proc/<pid>/fd`, and name the program from `/proc/<pid>/exe`. Checked in the VM:
  the doctor reports Kiln's server, and with Python on 8080, `kiln web start` says
  `port 8080 is used by /usr/bin/python3.12; Kiln uses 8000`.
- **Rule:** a fact the doctor can't gather on a platform must be reported as unknown, never
  as the healthy answer.

<a id="i-56"></a>
### I-56: Shell-quoted substitutions turned `\e` into an escape byte ✅
- **Found:** while making `apply.rs` portable. A perl substitution run through Git Bash
  rewrote `r"drivers\etc\hosts"` as `r"drivers<ESC>tchosts"`. It still compiled and passed
  clippy; reading the result back caught it. Two more attempts with `sed` and `perl` lost the
  backslashes again. An earlier session lost `\|` the same way in `pool.rs` and `process.rs`.
- **Fix:** the line was corrected with the editor tool, and the tree was scanned for escape
  bytes (`grep -P '\x1b'`): none left.
- **Rule:** never pass text containing backslashes through shell-quoted `sed`/`perl`
  replacements; edit such lines with the editor tool.

## P1-04b: Linux CA

<a id="i-57"></a>
### I-57: After the CA changed, browsers got certificates from the old one ✅
- **Found:** the user's P1-04b check in the Ubuntu VM. After `rm -rf …/ca && kiln ca init`,
  `kiln web restart` and `kiln ca trust --system`, `kiln doctor` said Chrome's database,
  Firefox's profile and the system store all trusted the new root. Yet:
  - curl reported `unable to get local issuer certificate`;
  - Chrome reported `NET::ERR_CERT_AUTHORITY_INVALID`;
  - Firefox reported `SEC_ERROR_BAD_SIGNATURE`.
- **Cause:** Caddy keeps the site certificates it issued in
  `storage/certificates/kiln/<site>/` and in memory, **across config reloads and
  restarts**, until each nears expiry. It went on serving `hello.test`'s certificate from the
  old intermediate, which chains to the old root.
  - Both intermediates were named `Kiln Intermediate CA (2026-10-23)`. Firefox matched the
    leaf to the new intermediate by name, and its signature didn't verify.
  - On Windows the same happens whenever the CA is replaced.
- **Fix:**
  - Caddy's storage records which intermediate its certificates came from
    (`kiln-web::issued`). `kiln web start` clears Kiln's CA folder there when it changed.
  - A running server restarts instead of reloading when the CA changed, on site changes and
    on `kiln web reload`, since a reload keeps the copies in memory.
  - Intermediates are named with time and root id:
    `Kiln Intermediate CA (2026-10-23 11:17, root eb9e00f8)`.
  - `kiln doctor` warns while stale certificates are stored.
- **Verified in the VM:** reproduced first (rejected). Then trusted after a restart, after
  replacing the CA while running followed by a site change (Kiln restarts and says why), and
  after a plain reload with the same CA. The doctor warns while the stale certificates are
  there, and not after.
- **Rule:** when Kiln changes something a running server caches, decide explicitly whether a
  reload is enough.

<a id="i-58"></a>
### I-58: Two dependency advisories during P1-04b ✅
- `cargo deny` reported **RUSTSEC-2026-0385** in `ctutils` 0.4.3, through RustCrypto's
  `digest`: non-constant-time comparisons. Updated to 0.4.5 with `cargo update -p ctutils`.
- The same day (2026-10-23), **RUSTSEC-2026-0370 to 0372** declared `zvariant`,
  `zvariant_derive` and `zbus_names` unmaintained: they were merged into `zbus` 6. P1-04b had
  just added `zbus` 5 for the Secret Service. Ported to `zbus` 6, with client features only
  and driven by `zbus::block_on`. That is 35 crates instead of about 60.
- A bare `cargo update` during the port bumped the whole lockfile; it was reset, and only
  the zbus swap was kept.
- **Rule:** update single dependencies (`cargo update -p <crate>`), never the whole
  lockfile as a side effect of other work.

## P1-15: integration test VMs

<a id="i-59"></a>
### I-59: Uninstall with a locked keyring orphaned the CA key's keyring item ✅
- **Found:** preparing the Linux VM for P1-15. Over SSH, with nobody logged in to the
  desktop, the keyring was locked.
  - `kiln uninstall` reported "Couldn't remove the CA root key from your keyring" and then
    deleted the state folder anyway.
  - That folder held the only reference to the item (`root.key.protected`), so nothing
    could find the item afterwards.
  - The keyring had five such items, from earlier test runs.
- **Fix:** when the keyring item can't be removed, uninstall keeps the state folder and says:
  "Run `kiln uninstall` again from a terminal on the desktop to finish." The second run
  removes the item, then the folder.
- **Rule:** never delete the last reference to something Kiln created outside its folders
  before that thing is gone.
- **Left over:** the five orphaned items in the test VM's keyring are removed by hand
  (Passwords and Keys, label "Kiln local CA root key") when someone is at its desktop.

<a id="i-60"></a>
### I-60: On Windows, reading `kiln web start`'s output waited forever ✅
- **Found:** the Windows VM scenario hung after `kiln web start`, although `kiln` had exited
  and the sites answered.
  - Windows hands a child process every inheritable handle of its parent, not only the
    three standard handles it is given.
  - A shell makes `kiln`'s stdout and stderr inheritable, so `kiln-caddy` and
    `kiln-supervisor` kept `kiln`'s output pipe open after `kiln` exited.
  - Anything reading that output (`kiln web start | ...`, a script or CI step capturing
    it) waited until the web server stopped.
  - Stopping both processes in the VM let the scenario continue at once, which confirmed it.
  - Linux is unaffected: Rust opens descriptors close-on-exec, and the child's 0–2 are
    replaced.
- **Fix:** `spawn_background` takes `kiln`'s standard handles out of inheritance while it
  starts the process (`kiln_sys::win::keep_std_handles`), then restores them.
- **Rule:** a process meant to outlive `kiln` gets only the handles it is given.

<a id="i-61"></a>
### I-61: `kiln php install` failed on every version after windows.php.net listed rebuilds ✅
- **Found:** the extended Windows VM scenario, 2026-10-26.
  - Every `kiln php install` stopped with "the PHP release index has an unexpected shape:
    8.2: odd version \"8.2.35-1\"".
  - windows.php.net now lists rebuilt packages of 8.2–8.5 as `8.4.27-1`, with archives
    named `php-8.4.27-1-nts-Win32-vs17-x64.zip`.
  - The parser required exactly `X.Y.Z` and rejected the whole index over one entry, so no
    version could be installed.
- **Fix:** a numeric `-N` suffix is read as a rebuild of the same PHP. The release is still
  PHP 8.4.27, the version `php -v` reports and Kiln names it by. The archive named in the
  index is downloaded and checked against its SHA-256 as before. Any other suffix is still
  refused.
- **Rule:** an upstream index can change shape at any time; keep a test with its real shape
  and fail with the entry named, as here.

<a id="i-62"></a>
### I-62: WordPress couldn't run on Kiln's PHP (no `mysqli`) ✅
- **Found:** the extended P1-15 scenarios, 2026-10-26. WordPress answered "Requirements Not
  Met: missing the MySQL extension".
  - **Linux:** the static-php-cli `common` set has `mysqlnd` and `pdo_mysql` but no `mysqli`.
    No published set has both `mysqli` and `pdo_sqlite` (`bulk` lacks the second).
  - **Windows:** the build ships `php_mysqli.dll`, but Kiln's default `php.ini` didn't
    enable it.
  - The Windows scenario's first check looked only for "WordPress", which the error page
    also contains. It now looks for the setup pages and fails on "Requirements Not Met".
- **Fix:**
  - **Windows:** `mysqli` is in `kiln_php::ini::DEFAULT_EXTENSIONS` (2026-10-26). The
    Windows scenario then passed with WordPress's real setup page (2026-10-27, 29 checks).
  - **Linux:** Kiln builds its own static PHP with `mysqli`, `intl`, `sodium` and `opcache`
    added to `common`. The builds are published from the public `sameedkun/kiln-php`
    repository, release `2026-10-27` ([ADR 0025](adr/0025-own-static-linux-php-builds.md)).
    - They are PHP 8.2.35, 8.3.36, 8.4.27 and 8.5.12, pinned after the release's
      checksums, two independent downloads and runs in the VM agreed.
    - The Ubuntu scenario then passed with WordPress, Laravel and PHP 8.3 (2026-10-27, 28
      checks).
- **Rule:** a check that a page works must look for something only the working page shows.

## P2-02: autostart

<a id="i-63"></a>
### I-63: `kiln … | head` panicked with "failed printing to stdout: Broken pipe" ✅
- **Found:** the Ubuntu autostart test piped `kiln web start` into `head -2`.
  - Once `head` exited, kiln's next `println!` panicked and printed a Rust backtrace hint.
  - The work was already done, so it only looked broken.
  - Rust ignores SIGPIPE and panics on a failed print instead. Windows reports the same
    case as "The pipe is being closed" (os error 232).
- **Fix:** a panic hook in `kiln` turns exactly that panic (a failed print to stdout or
  stderr with error 32 or 232) into a quiet exit with code 141 (128 + SIGPIPE), as Unix
  tools do. Other panics are reported as before.
- **Checked:** 20 runs of `kiln php available | true` in the Ubuntu VM: no panic, exit
  code 141. Not yet seen on Windows.

<a id="i-64"></a>
### I-64: A registry value written just before a hard reset was lost ✅
- **Found:** the first Windows autostart test hard-reset the VM seconds after
  `kiln daemon enable`.
  - Files survived, but the `Run` value didn't: Windows writes registry changes to disk
    lazily.
  - After a crash or power loss, the journal would say autostart was on while it wasn't.
    The user PATH (`kiln shims install`) had the same exposure.
- **Fix:** `kiln-sys` flushes the `HKCU` key (`RegFlushKey`) after every value it writes
  or deletes.
  - The autostart test then passed with a hard reset (`autostart.ps1 -HardReset`) as well
    as with a normal reboot.
- **Rule:** a journaled change is durable before the journal says it is applied: files
  are fsynced, and registry keys are flushed.

## P2-04: project config

<a id="i-65"></a>
### I-65: Windows PHP kept PHP's 30 s / 128 MB development limits ✅
- **Found:** WordPress's setup page in the Windows VM scenario took 22–31 s on its first
  request. When it went past 30 s, WordPress showed "There has been a critical error".
  - On Windows, Kiln's `php.ini` was PHP's bundled `php.ini-development` (30 s, 128 MB)
    plus Kiln's extension lines.
  - On Linux, Kiln's `php.ini` already set 60 s and 512 MB, and the same page was fast.
  - **Likely cause:** Windows Defender scans each of WordPress's files on its first load.
    That is the slowdown Herd's installer offers a Defender exclusion for; it was declined
    in the test VM.
- **Fix:** one `kiln_php::ini::LIMITS` constant (512 MB, 60 s, 64 MB uploads) used on both
  platforms. On Windows it is appended to Kiln's section, where it overrides the
  development defaults.
  - **Applies to new PHP installs.** An existing `php.ini` is the user's. P2-07 added
    extension toggles only ([ADR 0030](adr/0030-php-extension-toggles.md) §7); limits stay
    hand edits, at the path `kiln php ext list` prints.
- **Not done:** Kiln doesn't offer a Defender exclusion. That would be an admin-level change
  to Windows security, a decision for a later ADR, if ever.

## P2-08: kiln new

<a id="i-66"></a>
### I-66: nodejs.org's index listed a Node 27 alpha, and no Node could be installed ✅
- **Found:** the P2-08 Windows VM run, 2026-10-30. `kiln node install 22` failed with
  "unexpected nodejs.org release index: bad version in {…v27.0.0-alpha.0.0.0…}".
  - Since 2026-10-29, `index.json` starts with `v27.0.0-alpha.0.0.0`, a pre-release, as
    Node's new release schedule publishes alphas there.
  - Kiln's parser required every entry to be `vX.Y.Z` and refused the whole index, like
    I-61 for PHP.
- **Fix:** entries whose version has a pre-release suffix (`-…`) are skipped. Any other
  version Kiln can't read still fails, naming the entry. A test uses the index's real first
  entry.
- **Rule:** the same as I-61. Keep a test with the upstream's real shape, and skip only the
  shape that was seen.

## P2-09: proxy sites

<a id="i-67"></a>
### I-67: php.ini sometimes couldn't be replaced: "Access is denied" ✅
- **Found:** the P2-09 Windows VM run, 2026-10-30. `kiln php ext off curl` failed once with
  "Access is denied. (os error 5)". The same check had passed in the P2-07 and P2-08 runs.
  - Kiln writes php.ini to a temporary file and moves it over the old one with `MoveFileExW`.
    The move fails while another process has the old file open without delete sharing.
  - The likely holders are Defender scanning the file Kiln has just written, or a PHP
    process starting after the previous toggle restarted the pools. Both let go within
    moments.
- **Fix:** `kiln_sys::rename_replace` retries "Access is denied" and sharing violations on
  Windows. It waits 20 ms at first, doubling each time, for about two seconds in all. Any
  other error, or one that lasts longer, is reported as before.
- **Rule:** a Windows move over a file that other programs read can fail for a moment.
  Retry briefly, never forever.

<a id="i-68"></a>
### I-68: Windows' firewall prompt for Node covered the certificate dialog in the VM ✅
- **Found:** the same P2-09 Windows VM runs. Twice `kiln uninstall` waited until the run timed
  out.
  - Its "Root Certificate Store" dialog was open, and the scenario clicked its Yes button.
  - A screenshot of the VM showed Windows Firewall asking whether Node.js may use public and
    private networks. That prompt sat over the dialog, so the click never reached Yes.
  - `npm run dev` had started Vite. Vite listens on `::1` only, but Node still set off the
    prompt (probably while checking that the port was free). Windows also added "Block"
    rules for Kiln's `node.exe` while the prompt waited.
- **Fix (test harness):** `run.ps1` turns off the firewall's notify-on-listen setting in
  the test VM, next to its other test settings. Inbound connections stay blocked, as when
  nobody answers the prompt.
- **For users:** the same prompt appears for any dev server on Windows, with or without
  Kiln. A proxy target is on loopback, so "Cancel" is fine. Kiln doesn't change firewall
  rules.

<a id="i-69"></a>
### I-69: a newly secured site refused HTTPS for a moment after Kiln printed its URL ✅
- **Found:** a later P2-09 Windows VM run, 2026-10-30. Straight after `kiln proxy vite 5173
  --secure`, curl to the printed `https://vite.test` URL failed with "The Local Security
  Authority cannot be contacted". That is how Schannel reports a TLS `internal_error` alert.
  - Caddy's internal issuer creates a site's certificate in the background after a config
    load. Until it has one, Caddy answers that name's handshakes with that alert.
  - `kiln secure`, `kiln link --secure`, `kiln new --secure` and `kiln proxy --secure` all
    printed the URL as soon as the reload returned. Earlier runs had passed only because the
    certificate happened to be ready by the first request.
- **Fix:** after a reload, `kiln` waits until Caddy has stored a certificate for every
  secure site, polling every 100 ms for up to 10 s. Since P2-10, `kiln web start` waits
  the same way. If that runs out, it says the
  certificate is still being issued. The real-Caddy test checks that the certificate is
  where Kiln looks for it.
- **Rule:** reloading Caddy doesn't mean it's ready. Before printing a URL, wait for what
  the URL needs.

<a id="i-70"></a>
### I-70: the Laravel skeleton's build failed: Node gave up on Bunny Fonts after 250 ms 📌
- **Found:** later P2-09 runs, on both VMs, 2026-10-30, after the host's network changed.
  `npm run build` in the Laravel app failed in under a second, twice in a row.
  - The error was `[plugin laravel:fonts] TypeError: fetch failed`, caused by
    `AggregateError [ETIMEDOUT]`. The skeleton's `vite.config.js` downloads its fonts from
    Bunny Fonts at build time.
  - Node 20 and later give each of a host's addresses only 250 ms to connect
    (`--network-family-autoselection-attempt-timeout`). From this network, Bunny Fonts took
    about 260–270 ms to connect, and sometimes seconds. So every attempt timed out.
  - With the limit at 2000 ms, the same build succeeded in 5 s. curl, which waits longer,
    got there every time.
- **Not Kiln's:** this is Node's default meeting a slow route. It happens with any Node, and
  with Herd's too. Kiln doesn't change Node's defaults for everyone.
- **Test harness:** both scenarios raise the limit for the build step only, and retry it once.
  - In the P2-10 Ubuntu run, 2000 ms still failed once. Connects to Bunny Fonts then ranged
    from 0.25 s to over 1.3 s. The scenarios now use 10000 ms.
- **For users:** if a Laravel build fails with `fetch failed` and `ETIMEDOUT`, set
  `NODE_OPTIONS=--network-family-autoselection-attempt-timeout=10000`. A later `kiln doctor`
  could recognise it.

## P2-10: configurable TLD

<a id="i-71"></a>
### I-71: Caddy installed its own root CA into the system and browser trust stores ✅
- **Found:** the P2-10 Ubuntu VM run, 2026-10-30. After the new stage served a `.internal`
  site and Kiln was uninstalled, the comparison showed a root Kiln had never made:
  "Caddy Local Authority - 2026 ECC Root".
  - It was in the system anchors (`/usr/local/share/ca-certificates`) and in the user's NSS
    database.
  - Caddy's log said `pki.ca.local`: "installing root certificate (you might be prompted for
    password)", then "certificate installed properly in linux trusts".
- **Cause:** besides Kiln's CA (`kiln`, `install_trust: false`), Caddy set up its own
  default CA, `local`, and installed its root, as it does by default.
  - It did this for the `.internal` site. Recent Caddy versions treat `.internal` as a
    non-public name. It hadn't happened for `.test` sites in any run.
  - The site's certificate still came from Kiln's CA (`"issuer":"kiln"`). The `local` CA
    was set up and trusted, but never used.
  - The test VM has passwordless sudo, so it succeeded silently. On a real machine Caddy
    would have asked for a password, or used cached sudo credentials. Either way it's a
    system-wide trust change that Kiln didn't announce or journal, and uninstall didn't
    remove.
  - Before P2-10, `kiln ca init --tld internal` was also accepted, so this was possible
    then too.
- **Fix:** Kiln's Caddy config also declares the `local` CA, with `install_trust: false`
  and no keys of its own.
  - Caddy now logs "trust store installation disabled" for both CAs, and nothing is
    installed.
  - A unit test checks the config, and the Ubuntu scenario's second stage (`.internal`,
    then uninstall and compare) covers the real thing.
  - The test VM's stray root was removed by hand, after checking its bundle hash against
    the baseline.
- **Rule:** with Caddy, turn off every default that can reach outside Kiln's folders, not
  only the ones Kiln uses. The before/after comparison is what found this. Keep running
  it after every stage that starts the server.

### I-72: the first site change after `kiln web start` restarted the server ✅
- **Found:** the P2-10 Windows VM run, 2026-10-30. `kiln new shop --secure` never finished,
  and the run timed out an hour later. Linking the project again by hand worked, but said
  "Kiln's CA changed since the web server started" and restarted the server, though nothing
  had changed the CA.
- **Cause:** Kiln records which intermediate Caddy's site certificates came from in
  `kiln-intermediate.sha256` (I-57), but only wrote it when it found certificates from
  another one.
  - At the first `kiln web start` there are no certificates yet, so nothing was written.
  - Caddy then issued them. The next site change found certificates with no record beside
    them, took that for a changed CA, and restarted the server.
  - So every first HTTPS session restarted the server once, at the first `link`, `secure`
    or `kiln new`.
- **Fix:** the record is written whenever Kiln gives Caddy its CA: at every start, and
  before every reload that doesn't need a restart.
- **Not explained:** why the scenario's `kiln new` stalled. It had written `kiln.toml` but
  not yet saved the link, which is before that restart would happen. The same restart inside
  a piped command finished in seconds when run by hand. The scenario now runs `kiln new`
  with a 20-minute limit, and logs its output so far and the running processes if it hits it.

## P2-11: Phase 2 VM scenarios

### I-73: the restart after a CA change left sites unanswered for seconds ✅
- **Found:** the P2-11 Windows VM run, 2026-10-31, by the new timed check
  ([ADR 0034](adr/0034-phase-2-vm-scenarios.md) §2). During a renewal forced through
  `kilnd`'s API, the static site went 8.1 s without an answer. The Phase 2 exit criterion is
  under a second.
- **Cause:** the restart after a CA change (I-57) was `kiln web stop` then `kiln web start`.
  - It stopped and restarted the PHP workers too, though they don't depend on the CA. With
    two PHP versions and four workers each, that took most of the time.
  - Caddy validated the new config with a separate run only after the old server had
    stopped.
  - On a small machine (one PHP version, eight sites) the same restart measured 1.1 s:
    about 0.2 s stopping PHP, about 0.5 s starting it, and the validation.
- **Fix:** `WebServer::restart` writes the new config beside the running one and has Caddy
  validate it while the old server still serves.
  - A config Caddy rejects leaves the running server as it was.
  - Then it stops the old server, moves the config into place and starts Caddy at once.
  - The PHP workers keep running.
  - Caddy's log shows about 0.12 s between the old server exiting and the new one
    serving. A real-Caddy test covers the restart and the rejected config.
- **What a client sees on Windows:** a connection to a port with no listener isn't refused
  at once; Windows retries for about 2 s. So a request that arrives in the window is
  answered after one retry, about 0.6 s later, rather than failing. That is what the
  scenario's check measures.

## P3-01: the service framework

### I-74: downloads that stall mid-file fail the whole step ✅
- **Found:** the P3-01 Ubuntu VM runs, 2026-10-31. Two runs in a row, `kiln php install 8.3`
  stopped part-way through `php-8.3.36-fpm-linux-x86_64.tar.gz` from Kiln's GitHub release.
  - It stopped at 23.2 MB of 29 MB, then at 17.4 MB, each time after the scenario's retry
    too, with "no data arrived for 60 s".
  - The 8.4 download in the same runs completed. So did everything after it in the
    scenario that needed no PHP 8.3.
- **Checked:** within the hour, `curl` fetched the same file in about 4 s from the host and
  from the VM. `kiln php install 8.3` then succeeded three times in a row, in 25–30 s each.
  The third full run passed. It was GitHub's download servers, not the file or Kiln.
- **Gap:** Kiln's fetcher gives up after 60 s without data and starts again from zero on
  the next try. A resumed download (an HTTP `Range` request on the partial file) with a
  couple of retries would ride out such stalls. The pinned digest still checks the whole
  file, so resuming is safe.
- **For now:** the scenarios treat such a failure as the network. Rerun.
- **Fixed in P3-04:** the MariaDB Windows archive (92 MB) then stalled in the Windows VM. It
  hung 25 minutes on a slow server before the 60 s idle limit gave up, and nothing was kept.
  - **The fix:** `kiln-fetch` now keeps the partial file and its running digest when a
    download stalls or breaks, and asks for the rest with `Range: bytes=<received>-`, up to
    3 times.
  - **Only an exact continuation** is used: a `206` whose `Content-Range` starts at the
    received length. A server that sends the whole file again is started over from zero.
  - **The pinned digest** still covers the whole file.
  - **Servers:** GitHub, `cdn.mysql.com` and `archive.mariadb.org` all answered Range
    requests with `206`.
  - **Tests:** a resumed download, and a server that ignores the range.

## P3-02: the Windows cache spike

### I-75: Garnet's Lua scripts ignore `SELECT` (open, upstream)
- **Found:** spike 03, 2026-11-01 ([results](spikes/results/03-windows-cache.md)). Garnet
  2.2.3 passed 19 of the 22 Laravel checks that Valkey passes.
  - `Cache::add` stored nothing, a tag flush left its entry, and the rate limiter never
    limited.
  - Raw commands showed why: a script run on a connection that selected database 1 reads
    and writes database 0. Laravel's cache store uses database 1 (`REDIS_CACHE_DB`), and
    those three operations are Lua scripts.
- **Upstream:** [microsoft/garnet#2258](https://github.com/microsoft/garnet/issues/2258)
  (open since 2026-10-18). The fix is
  [microsoft/garnet#2311](https://github.com/microsoft/garnet/pull/2311), open and not
  merged on 2026-11-01. v2.2.5 doesn't have it.
- **Not worked around:** with the cache in database 0, all 22 checks pass. But
  `cache:clear` (`FLUSHDB`) would then also delete queued jobs and sessions.
- **Until it is fixed:** Garnet isn't used. Since P3-06, Windows has Redis from
  `redis-windows/redis-windows` ([ADR 0041](adr/0041-redis.md)), which runs Lua in the
  selected database (checked in CI). Linux uses Valkey.
- **To close:** when a Garnet release contains the fix, pin it, rerun
  `spikes/windows-cache/run-windows.ps1`, and require 22 of 22 with Laravel's defaults.

## P3-05: PostgreSQL

### I-76: services without admin rights couldn't start on an elevated machine (fixed)

- **Seen:** the first CI scenario with PostgreSQL. Every service failed to start with exit
  code `0xC0000142` (`STATUS_DLL_INIT_FAILED`), Mailpit included. The `kiln-sys` unit test
  for the restricted spawn failed the same way on the runner, and passed on the dev machine.
- **Why:** the runner is elevated and the dev machine isn't. An elevated token's default
  DACL, which new processes and threads get, grants Administrators, not the user. With
  Administrators deny-only, the new process couldn't open its own objects while loading its
  DLLs.
- **Fix:** the restricted token's default DACL now grants the user and SYSTEM full access,
  as `pg_ctl` does (`AddUserToTokenDacl`). The unit test checks the elevated case on every
  Windows CI run.

### I-77: a daemon API test raced the daemon's refusal (fixed, test only)

- **Seen:** once, on Linux CI: `bad_input_gets_an_error_not_a_hang` panicked with a broken
  pipe while writing a line over the size limit.
- **Why:** the daemon reads past the limit, answers "line too long" and closes, sometimes
  before the test has written the whole line.
- **Fix:** the test ignores that write's error and checks the answer, which is still there
  to read. The daemon's behaviour was right.

### I-78: PostgreSQL wasn't ready when its port was (fixed)

- **Seen:** `kiln service restart postgres-18` returned, and the next `kiln db` got "FATAL:
  the database system is starting up".
- **Why:** Kiln counted a service as started once its port answered. PostgreSQL listens
  before it has finished starting up.
- **Fix:** providers gain `ready()`. PostgreSQL's reads the status line of
  `postmaster.pid`, which says `ready` once it accepts sessions, as `pg_ctl start -w` does.
  Kiln waits for it after the port answers, within the same time limit
  ([ADR 0040](adr/0040-postgresql.md) §6).
- **Also seen:** the runner had no `HKCU\…\Run` key before the scenario and an empty one
  after. Kiln only writes that key with its `Kiln` value, and the run before had passed
  with the same steps. The snapshot now compares the key's values, which are what start
  programs, so an absent key and an empty one look the same.

## P3-06: Redis

### I-79: an MSYS2 program wrote nothing to its log (fixed)

- **Seen:** in CI, Redis ran and answered, but `kiln service logs` said it hadn't written
  anything, so the clean-shutdown check failed.
- **Why:** the supervisor opened each program's log in append mode. On Windows that grants
  `FILE_APPEND_DATA` without `FILE_WRITE_DATA`. Native and Go programs write through such a
  handle; the MSYS2 runtime doesn't.
- **Fix:** the log is opened for writing and positioned at its end. The program and its stop
  command get clones of that one handle, so they share one position and don't overwrite
  each other.

### I-80: the MSYS2 Redis read Windows paths as relative (fixed)

- **Seen:** the first CI run with Redis: neither instance started within the time limit.
  By hand, Redis said "can't open config file
  '/cygdrive/d/…/C:/Users/…/kiln.cnf'".
- **Why:** Redis makes a config path absolute by putting its current folder in front of
  anything that doesn't start with `/`, and `C:/…` doesn't.
- **Fix:** every path Redis sees is relative to the instance folder, where it runs: the
  config is `kiln.cnf`, with `dir data` and `pidfile server.pid`
  ([ADR 0041](adr/0041-redis.md) §3).
- **Also:** Windows PowerShell 5.1 puts a byte-order mark in front of text it pipes to a
  program, even with `$OutputEncoding` set to UTF-8 without one. `redis-cli` read it as
  part of the first command ("unknown command '﻿GET'"). `mysql` and `psql` skip it. The CI
  script passes Redis commands through a file instead. A user piping into `kiln db` from
  Windows PowerShell can hit the same; PowerShell 7 doesn't add the mark.

## P3-14: Meilisearch, RustFS and MongoDB

### I-81: MongoDB's archive was over the download limit (fixed)

- **Seen:** the first CI run with MongoDB: `kiln service add mongodb` stopped with "… is
  larger than the 536870912 bytes allowed".
- **Why:** `kiln-fetch` refused single downloads over 512 MB. MongoDB's Windows zips are
  808 MB (8.0) and 989 MB (9.0), mostly debug symbols, and there is no smaller official
  archive.
- **Fix:** the limit is 2 GB. The pinned digest still checks every byte, and the one-hour
  body limit allows down to about 280 KB/s for 1 GB.
- **Also:** two test-script faults, not Kiln's.
  - Windows PowerShell gives an S3 object served as text as a string, not bytes.
  - `Check` read a stale `$LASTEXITCODE` from an earlier failed check when its own block
    ran no program. It now resets the code first.

## P3-15: phpMyAdmin and Adminer

### I-82: the tools' server list could block service commands (fixed)

- **Seen:** the first CI run with the tools. `kiln tool add` failed to write its files, but
  had already created the tools folder. From then on, every `kiln service` command failed
  with "cannot write …\tools\servers.json".
- **Why:** two faults.
  - The files were written with `kiln_sys::write_replacing`, which replaces an existing
    file and fails on a new one. Its own comment says Kiln's own files go through
    `rename_replace`.
  - Service changes returned that error instead of carrying on.
- **Fix:** the tools' files are written to a temporary file and moved into place. A failure
  to rewrite `servers.json` is now printed as a note; the service command still succeeds
  ([ADR 0043](adr/0043-phpmyadmin-and-adminer.md) §2).

### I-83: uninstall kept Kiln's folder because of a tool's site (fixed)

- **Seen:** after `kiln uninstall --delete-data`, `%LOCALAPPDATA%\Kiln` was still there in
  full. The plan said "keep … (the project …\tools\phpmyadmin\phpMyAdmin-5.2.3-english is
  inside it)".
- **Why:** uninstall keeps any Kiln folder that holds a linked or parked project, so a
  user's project is never deleted ([ADR 0018](adr/0018-history-undo-uninstall.md) §5). The
  tools' sites are linked folders inside Kiln's folder.
- **Fix:** sites inside Kiln's tools folder are Kiln's own files and don't count as
  projects. The check compares canonical paths, like the rest of the folder check.
- **Also:** two test-script faults, not Kiln's.
  - `kiln web start` needs `kiln web install` first.
  - Adminer 6 checks a CSRF token on its login form, which a browser sends with the form;
    the test's POST now does too.

## P3-16: Laravel Reverb

### I-84: Reverb doesn't install on a new Laravel 13 app without `-W` (worked around)

- **Seen:** in CI, `composer require laravel/reverb` in an app from `kiln new` stopped with
  a dependency conflict, and Composer reverted `composer.json`.
- **Why:** Laravel 13's skeleton locks `guzzlehttp/psr7` 3.x. Reverb 1.12 (the newest, from
  2026-09-22) requires `^2.6`. Laravel and the Pusher client both accept 2.x, but Composer
  doesn't change a locked package unless asked.
- **Workaround:** `composer require laravel/reverb -W` (`--with-all-dependencies`) moves
  psr7 back to 2.x and installs. Kiln's "install Reverb first" message says so, and CI does
  it.
- **To close:** when a Reverb release accepts psr7 3.x, drop `-W` from the message and from
  CI.
- **Also:** the CI check for a broadcast passed once without Reverb installed. It looked
  for the word "broadcast", which the error message also contained. It now looks for a
  marker only the successful script prints.

## P3-10: `kilnd` and services

### I-85: a SQLite advisory against the bundled copy (fixed)

- **Seen:** CI's `cargo-deny` job failed on 2026-11-11 with RUSTSEC-2026-0459
  (CVE-2026-119375), against `libsqlite3-sys` 0.38.2 through `rusqlite` 0.40.
- **What it is:** SQLite before 3.54.0 can return uninitialised page-cache memory when it
  opens a database together with a sibling `-wal` file whose page size differs. A crafted
  pair of files makes ordinary queries return stale bytes.
- **Exposure:** small. Kiln only opens its own `state.db` in the user's folders, which
  another party would have to replace first.
- **Fix:** `rusqlite` 0.41.1, which bundles SQLite 3.54.1 (`libsqlite3-sys` 0.39.1). No code
  change; the state tests and `cargo-deny` pass.
- **Also:** the CI check for the daemon's restart read `kiln daemon status` while
  `kiln services resume` was still waiting for the slower databases. The daemon counts a
  restart once the command returns, so the check now waits for it.

## P4-02: the visual design

### I-86: the selected page disappeared with transparency off (fixed)

- **Seen:** the user's review of the mockup (2026-11-12). With transparency off, the selected
  **Home** pill had no visible background.
- **Cause:** "transparency off" mapped both the sidebar's glass and the selected item's
  glass to the same solid colour.
- **Fix:** the selected item has a solid colour of its own (`--selected-solid`). The contrast
  test checks that the selected item stands out from the sidebar in every appearance, and
  the gallery's checks compare the two colours in the browser. The light glass also moved,
  since a near-white pill on near-white glass was too close.

### I-87: the fade under the page header faded the header (design rule)

- **Seen:** the user saw content peek round the floating header's rounded corners. The first
  fix, a fade at the top of the scrolling content, also faded the header's own top edge,
  because the header was inside the scroller and a mask covers everything inside.
- **Rule:** a page's header sits beside its scroller, not inside it (`Page.tsx`). The
  scroller fades out under the header; the header stays sharp.

### I-88: axe can't measure contrast on glass (design rule)

- **What:** for text over translucent layers, gradients or very short text, axe's
  `color-contrast` rule returns "incomplete" rather than a result. A check that only looked
  for violations would pass without measuring anything.
- **Rule:** `src/styles/contrast.test.ts` measures contrast from the design values. It
  blends each fill over Kiln's backdrop, with and without the ember glow, and over the
  smoke behind a dialog. It checks WCAG AA for each text colour in light, dark and
  transparency off. The gallery's axe run allows "incomplete" only for those reasons. The
  first run moved five colours from the mockup: faint text, amber, the "stopped" ring, the
  light sidebar glass and dark red.

### I-89: the tray popup's glass over the desktop (design rule)

- **Seen:** the gallery's light screenshots. The tray popup is a window of its own, so its
  glass shows the desktop. Over a dark wallpaper, its fill came out mid-grey in light mode,
  and the quieter text measured 3.2 to 3.6:1.
- **Rule:** the popup's surface is Kiln's backdrop (`backdrop pop-frame`), which the
  contrast test covers. Windows' Acrylic (ADR 0051 §6) may replace it in P4-03 only with a
  fill opaque enough to pass the same check over both a light and a dark desktop.

### I-90: the stop icon read as a checkbox (fixed)

- **Seen:** the gallery's screenshots. An outlined square next to "Running", and before
  "Stop all", looked like an unticked checkbox.
- **Fix:** stop and start use the filled icons (`Stop16Filled`, `Play16Filled`).

## P4-03: the desktop app's shell

### I-91: CI jobs not started (open)

- **Seen:** 2026-11-13, on the first P4-03 push. Every job of the CI workflow ended at once,
  without starting: "The job was not started because recent account payments have failed or
  your spending limit needs to be increased."
- **Effect:** nothing pushed since is checked on CI or by the Scenario. Until it is fixed,
  each push is checked locally (the workspace's fmt, clippy and tests; the app's fmt,
  clippy, tests, build and `cargo deny`; the frontend's types, tests, build and browser
  checks), and the commits say so.
- **Fix:** the account's billing settings on GitHub (the user's). Then rerun the workflows for
  the newest commit, and the Scenario.
