# 0049. Data safety: `kiln db dump` and `restore`, `kiln uninstall --keep-data`

- **Status:** accepted
- **Date:** 2026-11-11
- **Task:** P3-11 · Implements [ADR 0035](0035-services.md) §7 · Builds on [ADR 0018](0018-history-undo-uninstall.md), [0036](0036-service-framework.md) §9, [0038](0038-mysql-and-mariadb.md), [0040](0040-postgresql.md)

## Context

A service's data is the user's work. What Kiln already guarantees
([ADR 0036](0036-service-framework.md) §9):

- `kiln service remove` keeps an instance's data unless `--delete-data` is given.
- `kiln uninstall` refuses while any data exists, unless `--delete-data` is given.

ADR 0035 §7 asked for two more things:

- **A way out in a portable form:** `kiln db dump` and `kiln db restore` with the services'
  own tools.
- **`kiln uninstall --keep-data`:** moves the data out of Kiln's folder, as the one thing
  uninstall leaves.

The databases ship their dump tools, and Kiln already keeps them:

- MySQL's `mysqldump`, MariaDB's `mariadb-dump`, PostgreSQL's `pg_dump` and `pg_dumpall`;
- loading a dump back is the job of the clients `kiln db` already opens.

MongoDB's dump tools are a separate download. Redis keeps its data as a single snapshot
file.

## Decision

1. **`kiln db dump <instance> [database] [--output <file>]`** writes SQL with the
   service's own tool, via `dump()` on the provider:
   - **MySQL, MariaDB:** `mysqldump` / `mariadb-dump` with `--single-transaction`,
     `--routines`, `--events` and `--triggers`.
     - One database is dumped without `CREATE DATABASE`, so it can be restored under
       another name.
     - Without a database, `--all-databases`.
   - **PostgreSQL:** `pg_dump --no-owner --no-privileges` for one database, so it restores
     as `postgres` anywhere; `pg_dumpall` for the whole server.
   - **The file:** the default is `<instance>-<database or all>-<date-time>.sql` in the
     current folder. An existing file is never overwritten, and a failed dump leaves no
     file.
   - **Other kinds:** Kiln says where their data is and that `--keep-data` keeps it.
2. **`kiln db restore <instance> <file> [database]`** feeds the file to the instance's
   client, via `restore()` on the provider:
   - **The target:** a named database is made if it's missing. Without one, a full dump
     goes to the server.
   - **psql** runs with `ON_ERROR_STOP=1`, so it stops at the first error.
   - **Checks:** the instance must be running, and database names that look like options
     (`-…`) are refused, so nothing but a name reaches the tools.
   - `kiln db <instance> [database]` still opens the client, as before.
3. **`kiln uninstall --keep-data`** (not together with `--delete-data`):
   - It copies each instance's data folder to `Kiln data (<date>)/<instance>` in the home
     folder, with ` 2`, ` 3`… if that name exists. It's the one thing uninstall leaves.
   - **When:** after the services are stopped, so nothing writes, and before anything is
     undone or deleted. Each copy is checked against the folder's size, measured then: a
     database writes on its way down.
   - **A failed copy** stops uninstall with nothing undone or deleted.
   - **The refusal** without either flag now offers `--keep-data` first, and names
     `kiln db dump`.

## Consequences

- **Data can leave Kiln before an uninstall**, either as portable `.sql` files or as the
  services' own data folders. The data folders reopen only with the same service version.
- **Redis and MongoDB data** leaves through `--keep-data` (or a copy of the data folder
  while the instance is stopped); there's no dump command for them yet.
- **CI:**
  - dumps the Laravel app's MySQL database and restores it as a new database with the
    same rows;
  - does the same for PostgreSQL, and dumps the whole server;
  - refuses to overwrite a dump, and has Redis say how its data is kept;
  - uninstalls with `--keep-data`, checking the kept folders hold each instance's data
    (the MySQL one with the `shop` database);
  - compares the machine before and after, as always.

## Alternatives considered

- **Moving the data folders instead of copying them.** Rejected: a rename can't cross
  drives, and a copy that fails halfway leaves the originals intact. Uninstall deletes
  them anyway afterwards.
- **Dumping every database at uninstall.** Rejected: it needs each service running, and a
  data folder kept whole loses nothing a dump might leave out.
- **`mongodump` for MongoDB.** Not now: it's a separate download (MongoDB Database Tools).
  It can become a companion like `mongosh`
  ([ADR 0042](0042-meilisearch-rustfs-mongodb.md) §1) later.
