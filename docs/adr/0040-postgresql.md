# 0040. PostgreSQL on Windows: EDB's binaries, `initdb` with loopback trust, services without admin rights

- **Status:** accepted
- **Date:** 2026-11-02
- **Task:** P3-05 · Builds on [ADR 0035](0035-services.md), [0036](0036-service-framework.md), [0037](0037-service-ports-and-coexistence.md), [0038](0038-mysql-and-mariadb.md), [0039](0039-windows-first-and-services-catalog.md)

## Context

The PostgreSQL project publishes no Windows binaries of its own. Its download page points to
EDB's builds, the same ones the Windows installer unpacks.

| | PostgreSQL 18.6 | PostgreSQL 17.11 |
|---|---|---|
| Archive | `postgresql-18.6-5-windows-x64-binaries.zip`, 344 MB | `postgresql-17.11-5-windows-x64-binaries.zip`, 341 MB |
| Unpacked | about 960 MB, of which pgAdmin 4 is about 780 MB | about the same |
| Published checks | none: EDB's page shows no digests or signatures | none |
| Needs | Microsoft's Visual C++ runtime, which the archive doesn't carry (only pgAdmin's Python does) | the same |

The `-5` is EDB's build number. The plain `-1` URLs also answer, but EDB's download page
points to `-5`, so Kiln pins that.

Two rules shape how PostgreSQL runs:

- **It refuses to run with administrator rights** on Windows: `postgres.exe` exits with
  "Execution of PostgreSQL by a user with administrative permissions is not permitted".
  `initdb` and `pg_ctl` get round this by restarting themselves with a token whose
  Administrators and Power Users groups are deny-only. A Kiln started from an elevated
  terminal, or on a CI runner (always elevated), would hit the refusal.
- **`initdb` picks the system locale** unless told otherwise. On Windows that's a name
  like `English_United States.1252` with a `WIN1252` encoding, which Laravel apps don't
  expect.

## Decision

1. **Kind `postgres`,** with port role `postgres` (usual port 5432):
   - Two pinned lines, 18 and 17, from EDB, Windows only. Linux waits for containers
     ([ADR 0039](0039-windows-first-and-services-catalog.md)).
   - Instances are named by major version (`postgres-18`).
   - The license line says the PostgreSQL License, binaries built by EDB, downloaded from
     EDB at the user's request.
   - EDB publishes no digests, so Kiln pins the SHA-256 of its own download over HTTPS.
2. **A whole-tree install** ([ADR 0038](0038-mysql-and-mariadb.md) §2), keeping about
   140 MB:
   - Kept: `bin`, `lib` and `share`.
   - Left out: pgAdmin 4, Stack Builder and its wxWidgets libraries, `doc`, `include`, and
     `.lib`, `.a` and `.pdb` files.
   - The server gets the Kiln name `kiln-postgres.exe`, a hard link. It finds `share` and
     `lib` relative to its own path, so the link works as the original does.
3. **Services run without administrator rights.** On Windows, the supervisor that runs
   services ([ADR 0036](0036-service-framework.md) §1) is started with the restricted token
   `pg_ctl` uses: the user's own token with Administrators and Power Users set to deny-only.
   - Everything it starts inherits that token: PostgreSQL, MySQL, Mailpit, and their stop
     commands.
   - This is what PostgreSQL requires, and a sound rule for every local service anyway.
   - Nothing changes for a user who isn't elevated: those groups are already deny-only in
     their token.
   - The restricted token's default DACL grants the user and SYSTEM, as `pg_ctl` does.
     An elevated token's default DACL grants Administrators instead of the user. With that
     group deny-only, a process couldn't open its own objects and failed to start with
     `STATUS_DLL_INIT_FAILED` ([I-76](../issues.md#i-76)).
   - The PHP supervisor is unchanged.
   - `kiln-sys` gains `spawn_without_admin` (`CreateRestrictedToken` and
     `CreateProcessAsUserW`). A restricted copy of the caller's own token needs no extra
     privilege.
4. **The data folder** comes from
   `initdb -D <data> --username=postgres --auth=trust --encoding=UTF8 --locale=C`:
   - `trust` applies to the lines `initdb` writes for `127.0.0.1/32` and `::1/128`. The
     server listens on 127.0.0.1 only, so trust never reaches beyond this computer. This
     matches MySQL's empty root password ([ADR 0038](0038-mysql-and-mariadb.md) §4).
   - The superuser is `postgres`, PostgreSQL's convention. A project's `.env` gets
     `DB_USERNAME=postgres` (P3-08).
   - UTF-8 with the C locale sorts the same on every machine and doesn't depend on
     Windows' code page.
5. **The config:** Kiln writes `<instance>/kiln.cnf` before every start:
   - First `include '<data>/postgresql.conf'`, which is `initdb`'s, with its memory, time
     zone and locale settings.
   - Then `listen_addresses='127.0.0.1'` and `port=<port>`, which win.
   - The server runs as `kiln-postgres -D <data> -c config_file=<instance>/kiln.cnf`.
   - `pg_hba.conf` and `postgresql.auto.conf` stay in the data folder, so `ALTER SYSTEM`
     works.
   - Its log goes to stderr, which is the instance's log.
6. **Ready and stopped:**
   - The server opens its port while it is still starting up, and refuses sessions ("the
     database system is starting up") until it has finished. It then writes `ready` as the
     status line of `postmaster.pid`, which `pg_ctl start -w` waits for. Providers gain
     `ready()`, true by default; Kiln waits for it after the port answers, within the same
     time limit.
   - `pg_ctl stop -D <data> -m fast -w`: open sessions end, the data is flushed, then the
     server exits.
   - An instance counts as stopped once `postmaster.pid` is gone, as with MySQL's pid file
     ([ADR 0038](0038-mysql-and-mariadb.md) §5).
7. **Client:**
   - `kiln db postgres-18 [database]` runs `psql --host=127.0.0.1 --port=<port>
     --username=postgres [--dbname=<database>]`.
   - The check after install runs `kiln-postgres --version`, and names the missing Visual
     C++ runtime as for MySQL.

## Consequences

- PostgreSQL works the same whether Kiln was started elevated or not, and the CI scenario
  (always elevated) shows it.
- All of a user's Kiln services run with least rights, including MySQL and Mailpit, which
  didn't need it.
- Each PostgreSQL version costs a 340 MB download for 140 MB kept: pgAdmin comes with the
  archive whether it's wanted or not. Downloads resume ([I-74](../issues.md#i-74)).
- The digests are only as trustworthy as Kiln's own download of them; EDB publishes nothing
  to check against.

## Alternatives considered

- **`pg_ctl start` instead of running the server directly.** Rejected. `pg_ctl` starts the
  server through `cmd.exe` and exits, so the supervisor would lose the process it watches
  and restarts.
- **Refusing to run services from an elevated Kiln.** Rejected. It would break the CI
  scenario and users who work in an administrator terminal. Dropping rights is what
  PostgreSQL's own tools do.
- **Restricting only the PostgreSQL process.** Rejected. The supervisor starts programs
  through the standard library, which can't take a token. Restricting the supervisor is
  one change, and a better default for every service.
- **A `root` role for Laravel's default `DB_USERNAME`.** Not now. Creating it needs the
  server running, or single-user mode, which also refuses admin rights. `.env` wiring
  (P3-08) sets `DB_USERNAME=postgres` instead.
- **The `-1` builds.** EDB's download page points to the newer build of the same version.
