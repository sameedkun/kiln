# 0091. Database servers Kiln doesn't run, used as services

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-12 · Builds on [ADR 0037](0037-service-ports-and-coexistence.md), [0045](0045-env-wiring.md),
  [0049](0049-data-safety.md), [0079](0079-snapshots-and-tinker.md)

## Context

Many developers moving to Kiln already have a database server with their data in it:
- Herd's MySQL;
- XAMPP's MariaDB;
- MySQL or PostgreSQL installed as a Windows service.

Kiln names those servers (ADR 0037) but can't use them. Its wiring, dumps, snapshots and
database list only work for instances it runs, which log in as `root` or `postgres` with no
password. Lerd can use a server it doesn't run; the user asked for what Lerd has.

The constraint from the start holds: Kiln never changes another tool's installation or
configuration unasked.

## Decision

1. **`kiln service attach <mysql|mariadb|postgres> --port <port> [--name <name>]
   [--user <user>] [--password <password>] [--bin <folder>]`** records a server on
   127.0.0.1:`port` that Kiln doesn't run.
   - The name defaults to `<kind>-<port>`.
   - The login defaults to `root` with no password (`postgres` for PostgreSQL), as Herd and
     XAMPP ship.
   - `--bin` is the server's own install folder, the one with `bin\` in it, for its client
     and dump tools. Without it, Kiln uses one of its own installed versions of that kind,
     and says to add one if there's none.
   - Kiln checks it before keeping it: the port answers, and listing its databases with that
     login works.
   - It is kept in Kiln's state (`services.external`), with the password. Kiln's state lives
     in the user's own profile, and the password is the local server's.
   - **`kiln service detach <name>`** forgets it. Nothing on the server changes.
2. **What works on it,** each only when the user asks:
   - `kiln env use <name>` (wiring, with its login in `DB_USERNAME` and `DB_PASSWORD`, and
     WordPress's names);
   - `kiln db list|create|dump|restore|drop`;
   - `kiln db snapshot`, `snapshots` and `rollback`, kept in Kiln's own folder for it.

   The login reaches the tools as `--user`/`--username`, and the password through the
   environment (`MYSQL_PWD`, `PGPASSWORD`), never on a command line.
3. **What never happens to it:**
   - `kiln service start|stop|restart|port|remove|update` refuse, saying Kiln doesn't run it;
   - `kiln config` won't edit its settings;
   - the supervisor never sees it;
   - Kiln's automatic snapshots before a project's steps, and daily ones (ADR 0086), skip
     it. A drop or rollback the user asks for on it still keeps one first, as for Kiln's
     own;
   - Kiln writes no config of its own for it.

   **Detach** forgets the server. It keeps Kiln's folder for it (`services\<name>`, with
   its snapshots) until the user removes it.
   - Through the app's API, `--password` and `--bin` are refused. The password comes
     staged instead (`--password-staged`), so it never reaches a job's arguments or kilnd's
     log.
   - A folder of programs to run is for the terminal only.
4. **Listing:**
   - `kiln service list` (`--json`) shows it, marked `external`, running when its port
     answers.
   - The app's Services page lists it as "not run by Kiln", without start and stop, with
     the same database, dump and wiring actions.
   - `service attach` and `service detach` are on the allow-list; the attach dialog asks
     for kind, port and login.

## Consequences

- An existing Herd or XAMPP database is wired, dumped and snapshotted from Kiln, without
  moving it.
- Kiln stays read-only toward that server, except for the database operations the user asks
  for.
- The password of a local development server sits in Kiln's state database.
