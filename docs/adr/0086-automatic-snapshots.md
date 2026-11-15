# 0086. Snapshots Kiln takes for you: before risky steps, daily, with retention

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-07 · Builds on [ADR 0049](0049-data-safety.md), [0075](0075-git-branches-and-worktrees.md),
  [0079](0079-snapshots-and-tinker.md), [0084](0084-site-commands.md)

## Context

`kiln db snapshot` keeps a database's state to roll back to (ADR 0079), but only when the user
remembers. Data is usually lost by a step that seemed routine:
- a migration;
- `migrate:fresh` or `db:wipe`;
- a branch switch whose migrations don't match the database;
- a `db drop`.

Lerd snapshots before such steps and on a schedule; Phase 5 planned "snapshots before
migrations". Snapshots pile up, so automatic ones need a limit, and the user needs a way to
keep one past it.

## Decision

1. **Kinds**, told apart by the file name in the instance's `snapshots` folder:

   | Kind | Name |
   |---|---|
   | taken by the user | `<db>--<time>.sql`, as before |
   | automatic | `<db>--<time>--<reason>.sql`; the reason is `before-switch`, `before-migrate`, `before-drop`, `before-rollback` or `daily` |
   | kept | `<db>--<time>--kept.sql` |

2. **Before risky steps**, while the setting `snapshots.auto` is on (the default), Kiln
   snapshots the site's database first, if it is on one of Kiln's running instances. The
   `.env`'s `DB_PORT` names the instance, as for wiring.

   | Step | If the snapshot fails |
   |---|---|
   | `kiln git switch` | a warning; the switch goes on |
   | `kiln run <site> artisan migrate…` (not `migrate:status`), `db:wipe` or `db:seed` | a warning; the command goes on |
   | `kiln db drop` | nothing is dropped, and Kiln says why |
   | `kiln db rollback` (it already kept one; now it's `before-rollback`) | nothing is rolled back |

   The command says which snapshot it took, so the user knows what to roll back to.
3. **Daily**, while `snapshots.daily` is on (off by default):
   - kilnd checks every hour;
   - for each database a site uses on a running instance of Kiln's, it takes a `daily`
     snapshot when the newest snapshot of any kind is a day old or more;
   - this is `kiln db snapshot-due`, run on kilnd's own thread, so a slow dump never
     delays watching the services.
4. **Retention:**
   - After each automatic snapshot, only the newest `snapshots.keep` automatic ones of that
     database are kept (10 by default, 1–100); older automatic ones are deleted.
   - Snapshots the user took, and kept ones, are never deleted by Kiln.
   - **`kiln db keep <instance> <file>`** renames an automatic snapshot to a kept
     one.
5. **`kiln db snapshot-settings [--auto on|off] [--daily on|off] [--keep <n>]`** shows or
   changes the three settings (`--json`). Only Kiln's state changes, so it isn't journaled.
6. **The app:**
   - The service page's snapshots list shows each one's kind and reason, with Keep on the
     automatic ones.
   - Settings has the three settings.
   - `db keep` and `db snapshot-settings` are on the allow-list, without asking; the settings are the read `snapshots.settings`.

## Consequences

- A bad migration or branch switch is one rollback away, without the user having planned
  for it.
- Disk use is bounded per database by the limit, except for what the user chose to keep.
- `git switch` and migrations take a little longer: the time to dump the database.
