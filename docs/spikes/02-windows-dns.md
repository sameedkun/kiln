# Spike 02: Windows name resolution, hosts block vs NRPT + local DNS stub

**Feeds:** [ADR 0006](../adr/0006-windows-dns-strategy.md) (opt-in mode) · **Time box:** 3 days ·
**Runs in:** a disposable Windows 11 VM. **Not on the dev machine.**

## Why a VM

Both modes change machine-wide state: the hosts file, the NRPT table, and a listener on port 53.
The dev machine has Laravel Herd's live hosts block and is used for VPN product testing. A
mistake there would break real work. Read-only checks on the dev machine (2026-10-19) found
no NRPT rules, nothing on port 53, no active VPN adapters, and Windows 11 Pro, so Windows
Sandbox and Hyper-V are available but **not enabled**.

**VM options (pick one; needs your approval):**

| Option | Change to the dev machine | Notes |
|---|---|---|
| A. Windows Sandbox | Enable the optional feature (admin, reboot) | Fresh disposable VM each launch. Scripted with a `.wsb` file. Easiest |
| B. Hyper-V VM | Enable Hyper-V (admin, reboot), plus about 20 GB disk | Snapshots allow repeatable runs, and a WireGuard client can be installed inside |
| C. Cloud VM | None | e.g. an Azure/AWS Windows 11 or Server 2025 instance. Costs money, and RDP setup |

Recommendation: **B** if we also want repeatable VPN interop tests (we do), otherwise **A**.

## Part 1: Hosts block (validates the accepted default)

Uses `kiln_platform::hosts` through a throwaway `spikes/windows-dns/hosts-writer` binary.

| Test | Pass criterion |
|---|---|
| Write 1, 50 and 500 names | Each resolves via `Resolve-DnsName` and in Edge/Chrome/Firefox within 2 s, with or without `DnsFlushResolverCache` |
| Coexistence with another tool's block (simulated Herd-style block) | Foreign block byte-identical after 100 random updates |
| CRLF, UTF-8 BOM, read-only attribute set, file missing | Preserved or handled with a clear error; **no ACL changes** |
| Defender real-time protection | Note any detections (`Get-MpThreatDetection`) for `.test` loopback entries |
| File locked by another process | Retry with backoff, then a clear error. Never a partial write (temp + `ReplaceFile`) |
| UAC flow | One prompt per batch. Measure prompt-to-done latency |
| Revert | `remove_block` restores the original file byte-for-byte (hash compare) |

## Part 2: NRPT + local stub (candidate opt-in mode)

A throwaway stub (`spikes/windows-dns/stub`, `hickory-server`) answers `*.test` → 127.0.0.1 / ::1
on `127.0.0.1:53` and returns REFUSED for everything else. The rule is added with
`Add-DnsClientNrptRule -Namespace ".test" -NameServers 127.0.0.1` and removed by its GUID.

| Test | Pass criterion |
|---|---|
| Wildcards | `anything.blog.test` resolves without a per-name write |
| Port 53 availability | Detect and report owners (Docker Desktop, ICS/SharedAccess, WSL mirrored networking). Fall back cleanly |
| WireGuard client (full tunnel, DNS pushed) | `.test` still resolves; tunnel DNS for other names unaffected |
| OpenVPN (TAP and wintun, with `block-outside-dns`) | Record behavior. `block-outside-dns` is expected to break it; document it |
| Corporate-style NRPT policy present | Our rule coexists. Precedence documented |
| Browser secure DNS: Chrome/Edge "custom provider", Firefox TRR mode 2/3 | Record which bypass NRPT, and the guidance needed |
| Sleep/resume, network switch (Wi-Fi ↔ Ethernet) | Still resolves; the stub restarts if it was killed |
| IPv6-only resolution path | AAAA `::1` works |
| Revert | Rule removed, stub stopped, `Get-DnsClientNrptRule` back to the baseline snapshot |

## Decision rule for the opt-in mode

Ship NRPT + stub as an opt-in in Phase 2 only if it passes **all** of: wildcards, WireGuard
interop, revert, and port 53 fallback. It must also produce a documented, user-visible warning
for each failing browser or VPN case. Otherwise hosts mode stays the only Windows mode, and the
NRPT idea moves to the Phase 6 backlog.

## Deliverables

- `docs/spikes/results/02-windows-dns.md`, including a compatibility table (VPN × browser ×
  mode).
- An ADR 0006 update stating the opt-in decision.
- Hosts-writer findings folded into `kiln_platform::hosts` tests, such as BOM handling and the
  atomic replace strategy.

## Approvals needed

1. Choice of VM option A/B/C, and enabling the Windows feature for A or B (admin + reboot).
2. Installing a WireGuard and an OpenVPN client **inside the VM only**.
