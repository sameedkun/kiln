# 0074. Site groups

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-10

## Context

With many sites, the user wants those that belong together (an app, its admin, its blog;
one client's sites) shown together, as Lerd's sites page groups them (2026-11-14).

## Decision

1. **A group is a label Kiln keeps per site**: the setting `sites.groups`, a JSON map from
   host to group name. `kiln group <site> <name>` puts a site in a group (made by naming
   it); `kiln group <site>` takes it out. Names are 1 to 40 letters, digits, spaces, `-`,
   `_` or `.`.
2. **Not journaled.** Nothing of the project's or the system's changes, so it doesn't belong
   in History; it can be changed back the same way it was made.
3. **`kiln --json sites`** gives each site's `group`. The app's Sites page shows a section
   per group, above the parked and linked ones, and each site's menu has "Put in a group…"
   (a name, with the groups already used offered). Finding a site matches its group too.

## Consequences

- A group that loses its last site disappears; there's nothing else to clean up.
- Sharing one database across a group's sites is already `kiln env use <instance>` with the
  same database for each; a one-step "share a database" can come later on this.
