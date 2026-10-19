# 0006. Windows name resolution: managed hosts block by default, NRPT stub as opt-in

- **Status:** accepted for the default. The opt-in mode is pending [spike 02](../spikes/02-windows-dns.md).
- **Date:** 2026-10-19

## Context

Windows has no `/etc/resolver` equivalent. The options are:

1. **Hosts file block.** It works with every browser and VPN client, and needs no listener on
   port 53. But it doesn't support wildcards, every site change is a privileged write, and some
   security products monitor hosts edits.
2. **NRPT rule (`.test` → 127.0.0.1) plus a local DNS stub.** It supports wildcards and needs
   only one privileged change. But it can conflict with VPN clients and corporate NRPT
   policies, it needs port 53 on loopback (Docker, ICS and WSL may hold it), and browsers using
   their own DNS-over-HTTPS may bypass it.

Our own users often run VPN clients that rewrite DNS, so compatibility outweighs wildcard
convenience for the default.

## Decision

- The default is a marker-delimited Kiln block in the hosts file
  (`kiln_platform::hosts`). It is written by `kiln-helper` with UAC elevation per batch, and
  there is no resident SYSTEM service.
- The helper accepts only names under the managed TLD and loopback addresses, preserves every
  byte outside its markers (including other tools' blocks), keeps the file's line endings,
  and refuses ambiguous marker layouts.
- The helper never changes the hosts file's ACLs. If it can't write, it fails with
  guidance instead.
- NRPT + stub becomes an opt-in mode only after spike 02 shows acceptable behavior with
  WireGuard, OpenVPN and browser secure-DNS settings.

## Consequences

- No wildcard subdomains in the default mode. `kiln link --alias` adds explicit names.
- One UAC prompt per site-set change. Batching keeps this to one prompt per user action.
- The pure block-editing logic is shared with the Linux hosts fallback.
