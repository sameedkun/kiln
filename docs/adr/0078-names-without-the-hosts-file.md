# 0078. Names without the hosts file

- **Status:** accepted: approved by the user 2026-11-15; built in P4D-02
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

## As built (P4D-02, 2026-11-15)

**More from the spike**, on a runner with a wildcard listener on UDP 0.0.0.0:53 beside Kiln's
(as Internet Connection Sharing has on the user's PC, where binding 127.0.0.1:53 beside it
was checked to work): Edge opened a `.test` name through the rule; AAAA questions got an
empty answer; with the rule on and the responder down, a name in the hosts file still
resolved, and any other `.test` name failed only after about 12 s. So kilnd must be running
while the rule is on; the app starts it at sign-in, and `kiln setup` says which mode is on.

**The helper's operation** (`NrptRule { tld, on }`): the TLD is validated again as a
`Tld`; 127.0.0.1 and the marker (`Kiln (ADR 0078)`) are constants in the helper. It runs
Windows PowerShell by its full System32 path, `-NoProfile -NonInteractive`, with a fixed
script: add the rule only if no rule exists for the namespace (a rule without Kiln's marker
is another program's, and the add is refused), remove only the marked rule, then clear the
DNS client's cache.

**The operation** (`nrpt_rule`): previewed (before and after), journaled, verified by
reading the rules back, and undone from History; uninstall removes it with the rest of the
journal, and the Scenario's snapshots list NRPT rules to prove it.

**`kiln dns on|off|status`**: `on` applies the operation, sets `dns.wildcard`, starts kilnd,
and waits up to 20 s for `kiln-check.<tld>` to resolve to loopback; if it doesn't, the rule
is undone and Kiln stays on the hosts file. kilnd answers on UDP 127.0.0.1:53 only while
`dns.wildcard` is on (a pure, bounds-checked parser with property tests: any bytes are
answered or dropped, never a panic).

**Everything that said "not in the hosts file"** now asks whether the name resolves: the sites'
`in_hosts`, `kiln sites`, the notes after link and park, `kiln setup`, the doctor's count, and
the app's "add the new names to the hosts file?" offer, which isn't made while the rule is on.
The app's Settings has "Site names", to turn it on (previewed, one administrator prompt) or
back to the hosts file.

**The setting follows the rule.** Undoing the rule from History sets `dns.wildcard` back, so
the hosts file is offered again; uninstall turns it off with the rest. `kiln ca init --tld`
won't change the TLD while the rule is on, as the rule names the old one (`kiln dns off`
first).
