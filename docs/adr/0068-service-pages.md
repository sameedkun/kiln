# 0068. A page per service, and its databases

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-02 · Builds on [ADR 0056](0056-services-page.md), [0049](0049-data-safety.md), [0045](0045-env-wiring.md)

## Context

The Services page lists instances with a menu of actions; logs, ports, dumps and the data
tools each open a dialog. The user asked for a page per service, as Lerd has: what it is,
its databases with their sizes and the sites that use each, its logs, the values a site's
`.env` gets, and its tools, all in one place (2026-11-14). Kiln can't list a server's
databases today, nor make or drop one outside a restore.

## Decision

1. **`kiln db list <instance>`** lists the instance's databases with their size on disk, for
   MySQL, MariaDB and PostgreSQL, through the server's own client (as `db dump` does). The
   servers' own databases (`mysql`, `sys`, `information_schema`, `performance_schema`;
   PostgreSQL's templates and `postgres`) are left out. With `--json`, each database also
   names the sites whose `.env` points at the instance with that `DB_DATABASE`. kilnd reads
   it as `services.databases`.
2. **`kiln db create <instance> <database>`** makes an empty database; **`kiln db drop
   <instance> <database>`** deletes one, after a confirmation that names it and says it
   can't be undone (a dump first is the way back, and the app offers one). Names are
   checked as `db dump` checks them: letters, digits, `_`, `$`, `-`, at most 64. Neither is
   journaled: a database's contents aren't Kiln's state, as with `db restore`.
3. **`kiln service list --json`** gives each instance's `.env` keys (`env`), the values
   `kiln env use` writes, for the page's `.env` tab.
4. **The app** opens a service's page from its row: a header with its version, port and
   state and Start/Stop; tabs for Overview (ports, connection, the sites that use it),
   Databases (where it has any: size, sites, create, dump, drop), Logs (followed live, as on
   the Logs page), `.env` and Tools (dump, restore, phpMyAdmin or Adminer, remove). The
   Services page keeps its menu for quick actions.

## Consequences

- Dropping a database is the first change in the app that deletes data with no undo; it
  asks in words, with the name typed back, and offers a dump first.
- Redis and MongoDB get no Databases tab yet: their pages show logs, ports and `.env`.
- Per-kind extras (pgAdmin, P4C-07) join the Tools tab.
