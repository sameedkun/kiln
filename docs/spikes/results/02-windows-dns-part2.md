# Spike 02 results, Part 2: NRPT + local stub vs hosts block, in a Hyper-V VM

**Date:** 2026-10-20 · **Verdict:** **do not ship NRPT + stub as a Windows mode yet.** It fails when the machine is offline and under a full-tunnel VPN, both cases where the hosts block keeps working. The hosts block stays the only Windows mode ([ADR 0006](../../adr/0006-windows-dns-strategy.md)). NRPT moves to the Phase 6 backlog.

## Setup

- Disposable Hyper-V Gen2 VM: Windows 11 Pro 25H2 (build 26200), 4 vCPU, 4 GB. Unattended install from
  the user's ISO with [`new-vm.ps1`](../../../spikes/windows-dns/vm/new-vm.ps1), generic (unactivated)
  Pro key, NAT via "Default Switch". Defender real-time protection **on**.
- Driven from the host over PowerShell Direct ([`run-part2.ps1`](../../../spikes/windows-dns/vm/run-part2.ps1)).
  Tests ran as the local admin `kiln` with an elevated remote token, matching `kiln-helper`.
- Edge (Chromium) was launched like a real user's browser: headless, in the interactive console session,
  **not elevated**. Edge refuses to run with an elevated token. Each check loaded
  `http://<name>:8099/` from a tiny local HTTP responder.
- NRPT mode: [`dns-stub`](../../../spikes/windows-dns/stub) on `127.0.0.1:53` / `[::1]:53` answering
  `*.test` → loopback and `REFUSED` otherwise, plus
  `Add-DnsClientNrptRule -Namespace .test -NameServers 127.0.0.1`.
- Hosts mode: Kiln's block written by [`hosts-writer`](../../../spikes/windows-dns/hosts-writer) via `ReplaceFileW`.
- WireGuard for Windows 1.1.1 (Authenticode-verified, WireGuard LLC), full tunnel (`0.0.0.0/0, ::/0`) with
  `DNS = 10.64.0.1`, to an unreachable TEST-NET peer. The client still applies its routes, DNS settings and
  DNS leak protection, which is what interop depends on. No traffic flows.
- Raw output: `spikes/windows-dns/.cache/part2-main.txt` (git-ignored). Key lines are quoted below.

## Results (main run)

OS resolver = `[System.Net.Dns]::GetHostAddresses`, the same path most apps use. ✅ resolves, ❌ does not.

| Scenario | Hosts block, OS | Hosts block, Edge | NRPT wildcard, OS | NRPT wildcard, Edge | External names |
|---|---|---|---|---|---|
| Baseline | ✅ 0 ms | ✅ | ✅ 0–78 ms (`any.thing.nrpt.test`) | ✅ | ✅ |
| After VM save → restore (sleep/resume) | ✅ 76 ms | ✅ | ✅ 2 ms | ❌ (1 trial; recheck pending) | ✅ |
| **Network adapter unplugged** | ✅ 2 ms | ✅ | **❌ never** | **❌** | ❌ (expected) |
| Adapter replugged | ✅ | ✅ | ✅ | ✅ | ✅ |
| **WireGuard full tunnel, DNS pushed** | ✅ 0 ms | ✅ | **❌ never** | **❌** | ❌ (no real server) |
| After WireGuard tunnel removed | ✅ | ✅ | ✅ | ✅ | ✅ |

The unplugged-adapter failure for NRPT reproduced in **both** complete runs. The WireGuard row comes from the
one run in which the tunnel came up (the other run hit a harness bug).

### Other checks

| Check | Result |
|---|---|
| Defender real-time protection on, 51 names written by an elevated admin | **0 detections**. Block intact after 20 s |
| Hosts write → first resolution | 102–157 ms (50 names), no cache flush |
| NRPT rule add → wildcard resolves | 2–78 ms (after `Clear-DnsClientCache`) |
| External names while NRPT rule active | unaffected (`example.com` resolves via the normal DNS server) |
| Port 53 already taken | stub exits with code 2: `Only one usage of each socket address…` (os error 10048). Restarts cleanly once the port is free |
| Revert | NRPT rules back to 0, no port 53 listeners, hosts SHA-256 identical to baseline, wildcard stops resolving |

## Why NRPT fails in exactly these cases (interpretation)

- **Offline:** with no connected interface, the Windows DNS Client service has no interface to send the
  query on, so it never consults the NRPT name server, even though that server is loopback. The hosts file
  is read before any network lookup, so it works offline.
- **Full-tunnel VPN:** WireGuard (like most VPN clients, including the ones we ship) installs DNS leak
  protection. Name resolution is pinned to the tunnel's DNS server, and queries to other resolvers, ours
  included, don't get through. Hosts-file lookups happen before DNS and are unaffected.

Both cases are routine for developers: working on a plane or train, or on a client's VPN. A local dev
environment that loses every `.test` site in those situations is worse than one without wildcards.

## Decision input for ADR 0006

The spike plan's rule was to ship NRPT + stub as opt-in only if it passes wildcards, WireGuard interop,
revert and port 53 fallback.

| Criterion | Result |
|---|---|
| Wildcards | ✅ pass |
| WireGuard interop | ❌ **fail** |
| Revert | ✅ pass |
| Port 53 fallback | ✅ pass (clear error) |
| *(new)* Offline | ❌ **fail** |

→ **NRPT + stub is not shipped in Phase 2.** It goes to the Phase 6 backlog, together with a possible
"hosts block + local wildcard expansion" approach: Kiln writes explicit names for the subdomains a
project's config declares.

## Harness lessons (for Phase 1 integration tests)

1. **Checkpoint only a settled guest.** A checkpoint taken while Windows was still finishing setup
   restored to a half-finished state: the temporary `defaultuser0` session, then a reboot after which the
   test account's credentials were rejected.
2. **Boot order:** DVD → disk only. With a network (PXE) entry, a Gen2 VM can stall at the firmware screen
   after Setup's reboot. One install also left a disk with no bootable OS. That was likely host memory
   pressure: a checkpoint restore had failed with "not enough memory" on this 16 GB host.
3. **PowerShell variable names are case-insensitive:** `$p` overwrote `$P`. Use distinct names.
4. **Edge must run non-elevated in the interactive session.** From PowerShell Direct (session 0) it
   prints nothing, and elevated it exits immediately.
5. A no-console VM screenshot (`vm/screen.ps1`, via `GetVirtualSystemThumbnailImage`) is what finally
   diagnosed the boot failure. Keep it in the toolbox.

## Not covered / follow-ups

- **Edge × DNS-over-HTTPS (off / automatic / secure) matrix, 3 trials each.** The main run's DoH rows came
  from a cold interactive session and are unreliable. A recheck is pending on a rebuilt VM.
- A second WireGuard trial, plus a direct query to the stub (`Resolve-DnsName -Server 127.0.0.1`) to confirm the
  leak-protection explanation. Pending in the same recheck.
- OpenVPN (`block-outside-dns`) and corporate NRPT policies need a real server or domain. Not tested.
- Firefox and Chrome (only Edge is preinstalled).
