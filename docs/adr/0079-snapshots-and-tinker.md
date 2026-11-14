# 0079. Database snapshots and Tinker

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-15 (from Phase 5) · Builds on [ADR 0049](0049-data-safety.md), [0068](0068-service-pages.md)

## Context

Lerd offers database snapshots and a Tinker console per site, and Phase 5 had them planned:
snapshots before migrations, and Tinker. The user asked for Kiln to be as complete
(2026-11-14).

## Decision

1. **`kiln db snapshot <instance> <database>`** keeps a dump by the server's own tool (as
   `db dump`) in the instance's `snapshots` folder, named `<database>--<UTC time>.sql`.
   **`kiln db snapshots <instance>`** lists them, newest first (`services.snapshots` in kilnd).
2. **`kiln db rollback <instance> <snapshot>`** puts the database back as the snapshot has
   it, after asking: the database is emptied (dropped and made again) and the snapshot loaded.
   A snapshot of it as it was is kept first, so a rollback can be rolled back. Only a name
   from the list is taken: no path, nothing outside the instance's folder.
3. **`kiln tinker <site> <code>`** runs `php artisan tinker --execute <code>` in a Laravel
   site's folder with the PHP Kiln picks there, without a window, and gives its output (cut
   at 256 KB). The code is the user's own, in their own project; the app asks nothing more
   than the terminal would.
4. **The app**: a camera button on each database's row (Databases tab) and a Snapshots list
   with "Roll back"; a Tinker tab on a Laravel site's page (Ctrl+Enter runs).

## Consequences

- Snapshots take disk space in Kiln's folder; they show in the footprint (ADR 0063) with the
  instance, and go with it when it's removed with its data.
- Tinker runs one statement block at a time; an interactive console session is for a
  terminal (`php artisan tinker`).
