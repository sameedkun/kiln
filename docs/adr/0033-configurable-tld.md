# 0033. Configurable development TLD

- **Status:** accepted
- **Date:** 2026-10-30
- **Task:** P2-10 · Amends [ADR 0010](0010-local-certificate-authority.md) · Builds on [ADR 0006](0006-windows-dns-strategy.md), [0015](0015-parked-folders-and-drivers.md)

## Context

Kiln serves sites as `<name>.test`. [ADR 0010](0010-local-certificate-authority.md) already
says the root CA's name constraint is "the configured development TLD (default `test`)",
and much of the code takes a TLD:

- the root and intermediate certificates;
- the hosts-file block, which is labelled with its TLD;
- name validation.

But nothing let a user choose one. The CLI, the shims and certificate renewal all assumed
`test`.

Some teams need another name: `.test` is taken by another tool, or a company standard says
`.internal`. Plan principle 3 rules out names that resolve on the public internet. A `.dev`
or `.app` site would send its traffic, and possibly its cookies, to the real domain the
moment Kiln's hosts entry is missing. `.local` belongs to mDNS, and `.localhost` to the
operating system.

The root's name constraint fixes its TLD for the root's whole life. A root for `.test` can't
vouch for `blog.internal`.

## Decision

1. **Chosen once, at `kiln ca init --tld <name>`:**
   - When `ca init` creates the root, the root is constrained to that TLD, and Kiln records
     it as the `tld` setting in its state.
   - Without `--tld`, the TLD is the recorded one, or `test` if none is recorded.
   - Every part of Kiln reads that setting:
     - site names (`link`, `proxy`, `new`, `secure`, `isolate`);
     - parked folders;
     - `hosts sync`, and the defaults of `hosts set`, `hosts clear` and `plan hosts`;
     - the shims, which find a folder's site.
2. **Only reserved, non-public names:** `test` (the default), `internal`, `example` and
   `invalid`.
   - These are the names that RFC 2606 and ICANN (2024, for `.internal`) set aside so that
     they never resolve publicly.
   - Anything else is refused, including names that are free today but could be delegated
     tomorrow.
3. **Changing it later means a new root, so `ca init` refuses:**
   - If a root exists for another TLD, `kiln ca init --tld <other>` stops and says so.
     Kiln can't rotate the root yet (the later `kiln ca rotate` of ADR 0010). Until then
     the way is `kiln uninstall`, then `kiln ca init --tld <other>`.
   - If links or proxies already use another TLD when the root is created, `ca init`
     refuses and lists them. Unlink them first.
4. **An existing root wins:** `ca init` reads the TLD back from the root's name constraint
   and records it. This covers a root created before this ADR with a TLD other than `test`.
   The web server's intermediate is always issued for the root's TLD, whether at
   `kiln ca init`, at `kiln ca renew`, or at a start with a stale intermediate.
5. **The hosts block follows:** `kiln hosts sync` writes Kiln's one block with the names
   under the current TLD, replacing what was there
   ([ADR 0006](0006-windows-dns-strategy.md)). Undo still restores the earlier names
   exactly.
6. **What Kiln recorded before stays readable:** journaled hosts changes and roots may
   carry a TLD that `--tld` would now refuse, such as `wip`, because the earlier rules
   only refused known public names.
   - `Tld::new` applies the new allow-list. It is used for every new choice: `ca init
     --tld`, and `--tld` on the `hosts` and `plan` commands.
   - `Tld::recorded` applies the earlier rules. It is used when reading back what Kiln
     wrote itself: journal entries, an existing root, the `tld` setting.
   - So `kiln undo` and `kiln ca trust` keep working for them.

## Consequences

- One setting decides the TLD for the whole of Kiln. The root certificate can't disagree
  with it, because `ca init` records the root's own TLD.
- `.test` users notice nothing.
- The allowed list is short on purpose. A request for another name must show it is
  reserved.
- Choosing the TLD after linking sites over HTTP means unlinking them first. That's rare,
  since HTTPS needs the root anyway.
- Doctor messages and help text say "Kiln's sites" rather than `.test`.
- Caddy treats some of these names specially. With a `.internal` site it set up its own
  default CA and installed that root into the trust stores
  ([I-71](../issues.md#i-71)). Kiln's config now switches that off for Caddy's default CA
  as well as its own, whatever the TLD. Only `test` and `internal` have been VM-tested
  with sites.

## Alternatives considered

- **Any label that isn't a known public TLD (a blocklist), as before.** Rejected. Kiln
  can't know offline which names ICANN will delegate next, and `.dev` showed what happens.
- **A separate `kiln tld <name>` command that rotates the root and re-trusts it.**
  Deferred to root rotation. Swapping trust touches the OS and browser stores, and needs its
  own preview and undo.
- **Reading the TLD from the root certificate every time.** Rejected. The shims run on every
  `php` and `node` call and shouldn't parse certificates. A root that doesn't exist yet has
  no TLD to read anyway.
