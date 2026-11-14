# 0078. Names without the hosts file

- **Status:** proposed: the spike passed on CI (2026-11-14); building it waits for the user's
  approval, as it changes a Windows setting once
- **Date:** 2026-11-14
- **Task:** P4C-14 (from Phase 6's "Windows wildcard DNS") · Builds on [ADR 0015](0015-parked-folders-and-drivers.md) §9

## Context

Windows' hosts file has no wildcards (ADR 0015 §9), so each new site needs its name written
there, through Windows' administrator prompt. The user asked why adding a site doesn't just
work (2026-11-14). Since P4C-01 the app offers the hosts file right after a site is added;
that is still one prompt per change.

## The spike (`tests/ci/spike-dns.ps1`, on a throwaway runner only)

A minimal DNS responder on UDP 127.0.0.1:53 answered any `*.test` A question with 127.0.0.1
(anything else: "no such name"). With **one Name Resolution Policy Table rule**
(`Add-DnsClientNrptRule -Namespace .test -NameServers 127.0.0.1`):

- before the rule, `kiln-spike.test` didn't resolve; with it, `Resolve-DnsName`,
  getaddrinfo (.NET, as programs use it) and curl all reached the responder, for any name
  under `.test` (`shop.kiln-spike.test` too);
- other names (`github.com`) still went to the usual DNS;
- removing the rule made `.test` names stop resolving again.

## Proposed decision

1. **kilnd answers `.test` itself**: a responder like the spike's, in Rust, on 127.0.0.1:53
   (and `[::1]:53`), answering only names under Kiln's TLD with the loopback address, and
   "no such name" otherwise. It listens on loopback only (principle 3).
2. **One rule, once, with consent**: the NRPT rule for `.<tld>` pointing at 127.0.0.1, added
   through Kiln's privileged helper as a new fixed operation (principle 2), previewed and
   journaled like the hosts block, so History and `kiln uninstall` remove it. The onboarding
   offers it instead of the hosts file: "Names for every site, now and later (one
   administrator prompt)".
3. **The hosts file stays the fallback**: without the rule, or with kilnd stopped, Kiln
   keeps writing the hosts block as today. With the rule on, `hosts sync` isn't needed and
   the app stops offering it.
4. **Browsers**: Chromium-based browsers fall back to the system resolver when NRPT rules
   are present; this gets its own check in the CI app run (Edge resolving a `.test` name)
   before it ships.

## Consequences

- If kilnd isn't running, `.test` names don't resolve (the rule still points at 127.0.0.1):
  the app starts kilnd at sign-in, and the doctor says so.
- Port 53 on loopback may be taken by another local DNS tool; Kiln then says which, and stays
  on the hosts file.
