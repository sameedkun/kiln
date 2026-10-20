# Spike 02 results, Part 1: hosts-block mode in Windows Sandbox

**Date:** 2026-10-20 · **Verdict:** the hosts-block default ([ADR 0006](../../adr/0006-windows-dns-strategy.md)) is confirmed, with the implementation requirements below · **Part 2** (NRPT + stub, VPN interop) needs the Hyper-V VM and is pending.

## Setup

- Windows Sandbox (Windows 11 build 10.0.26100), started with `wsb start` using
  [`part1.wsb`](../../../spikes/windows-dns/sandbox/part1.wsb). Test tools were mapped read-only
  and a results folder writable.
- Tests ran via `wsb exec -r System` because no interactive logon happens with `wsb start`, so
  `LogonCommand` never fired. They therefore ran as `NT AUTHORITY\SYSTEM`, a superset of the
  elevated-admin context `kiln-helper` will use.
- Tool: [`hosts-writer`](../../../spikes/windows-dns/hosts-writer), a thin wrapper over
  `kiln_platform::hosts` with three write strategies. Harness:
  [`part1.ps1`](../../../spikes/windows-dns/sandbox/tools/part1.ps1).
- Resolution was checked with `[System.Net.Dns]::GetHostAddresses`, polling every 50 ms with no
  DNS cache flush.

## Results

| Test | Result |
|---|---|
| Resolution after write: 1 / 50 / 500 names | First name 0–1 ms. Last name 71 / 25–33 / 295–332 ms. **No `DnsFlushResolverCache` needed** |
| Removed names stop resolving | Yes, immediately (not resolvable within 1.5 s) |
| Write time (500 names, 18.8 KB file) | 1–11 ms |
| Another tool's block, 100 random updates | Byte-identical throughout. Removing Kiln's block restored the exact previous file (SHA-256 match) |
| UTF-8 BOM | Preserved. Names resolve |
| Non-UTF-8 byte (Latin-1 `0xE9` comment) | **Failed on the first run** (the API was `&str`-based). Fixed by making `kiln_platform::hosts` byte-based. Rerun: byte preserved, names resolve |
| CRLF | Preserved (the Windows default file is CRLF) |
| Read-only attribute set | All strategies fail with *Access is denied*, even as SYSTEM |
| File held open without sharing | *Sharing violation* (os error 32) on read |
| File missing | *Not found* on read |
| Defender detections | 0, **but Defender real-time protection is off in Sandbox**, so this is inconclusive. Re-test in the Hyper-V VM |
| Final state | SHA-256 identical to baseline after every run |

### ACL preservation (the deciding test)

An explicit `Deny Write` entry for `BUILTIN\Guests` was added to `hosts`, then one name was
written with each strategy:

| Strategy | Explicit ACE afterwards |
|---|---|
| Temp file + `ReplaceFileW` | **kept** |
| Temp file + rename (`MoveFileEx` `REPLACE_EXISTING`, i.e. `std::fs::rename`) | **dropped**: the file only has inherited ACEs |

On a default hosts file, which has inherited ACEs only, both look identical. Only the
`SE_DACL_AUTO_INHERITED` control flag differed between runs. Any explicit hardening, whether by an
admin, an endpoint-protection product or another tool's helper, is silently removed by the
rename strategy.

## Requirements for the `HostsBlock` DNS provider (Phase 1)

1. **Byte-oriented editing.** Done: `kiln_platform::hosts` now takes and returns `&[u8]` /
   `Vec<u8>`. A property test feeds arbitrary bytes (20,000 cases) and checks foreign content
   survives and round-trips exactly. The block's own line ending is reused on update and
   removal (a regression case found by the property test: foreign content ending in a lone
   `\r`).
2. **Write with `ReplaceFileW`** (temp file in the same directory), never rename or truncate.
   This needs Win32 FFI. `unsafe_code` is forbidden workspace-wide, so the helper needs an ADR
   exception scoped to one audited module, or a vetted crate that wraps `ReplaceFileW`.
3. **Read-only attribute:** refuse by default with guidance ("the hosts file is marked read-only;
   another tool or an admin may have locked it on purpose"). Clearing and restoring the
   attribute is an explicit, journaled opt-in. **Never** touch ACLs.
4. **Sharing violations:** retry with backoff (e.g. 50 ms → 1 s, 5 attempts), then fail clearly.
   Never write partial content.
5. **Missing file:** create it containing only Kiln's block. The receipt records "absent" so
   revert deletes it rather than leaving an empty file.
6. **No DNS cache flush needed** for additions or removals on current Windows.
7. **Verify** by re-reading the block, plus a `GetHostAddresses` probe for one name.

## Not covered (moved to Part 2, Hyper-V VM)

- Defender real-time protection behavior (disabled in Sandbox).
- The elevated-admin user context with UAC, rather than SYSTEM.
- Browser resolution (Edge, Chrome, Firefox, including secure-DNS settings).
- NRPT + stub mode and VPN client interop.
