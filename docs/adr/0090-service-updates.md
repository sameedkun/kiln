# 0090. Service updates: a newer patch shown, one or all updated, the previous kept

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-11 · Builds on [ADR 0036](0036-service-framework.md), [0038](0038-database-services.md),
  [0064](0064-updates.md), [0086](0086-automatic-snapshots.md)

## Context

Kiln pins one release per series of each service: MySQL 8.4, PostgreSQL 18, Redis 8.10 and
so on, each with its SHA-256. A newer Kiln pins newer patches, but an instance keeps the
version it was added with. Its binaries live in `bin\<kind>\<version>`, shared by instances
of that version; its data lives in its own folder.

Patch releases carry security and bug fixes, and keep the data format within a series.
Lerd updates services from its UI and keeps the previous version; the user asked for what
Lerd has.

## Decision

1. **A newer patch** is the release Kiln pins for an instance's series, when its numbers are
   higher than the instance's version.
   - It is never a different series (MySQL 8.4 doesn't become 9.x), and never a downgrade.
   - Processes that run in a project (queue, Vite…) have none: their program is the
     project's.
   - **`kiln service updates`** (`--json`, the read `services.updates`) lists each instance
     with its version, the newer patch if any, and the version before its last update.
2. **`kiln service update <name>… | --all [--yes]`** shows each instance's old and new
   version, asks, then for each:
   1. downloads the new patch, checked against Kiln's pin, into its own `bin` folder, and
      proves it runs on this PC, as `service add` does;
   2. if it's a running database, snapshots each database a site uses, before anything
      changes (reason `before-update`, ADR 0086). If that fails, the instance isn't
      updated;
   3. stops the instance, records the new version, and starts it again if it was running;
   4. waits up to 60 seconds for its port. If it doesn't answer, the instance goes back to
      the old version and is started again, and the update fails with where its log is.

   Its data folder, ports, settings and wiring stay as they were. The previous version is
   remembered (`services.previous.<name>`). Its binaries stay where they are.
3. **`kiln service rollback <name> [--yes]`** puts an instance back on the version before its
   last update, if that version's binaries are still on this PC, checked the same way.
   - It's for a patch that misbehaves. A database server may already have upgraded its
     data on the newer patch. MySQL's long-term series and MariaDB allow going back within
     a series; the snapshot taken before the update is the safe way back for the data.
4. **The app:**
   - the Services page shows a notice when any instance has a newer patch, with
     "Update all";
   - a service's page has "Update to <version>", or after an update, "Back to <version>".

   Both go through the preview and confirmation. `service update` and `service rollback`
   are on the allow-list as asking first.

## Consequences

- A newer Kiln's patches reach existing instances with one click, their data untouched and
  a snapshot taken first.
- A bad patch is one step back, while its binaries are kept.
- Old versions' binaries stay until removed with the instance. Kiln doesn't clean them up.
