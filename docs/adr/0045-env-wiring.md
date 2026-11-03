# 0045. `.env` wiring: `kiln env use`, key-level edits with byte-exact undo

- **Status:** accepted
- **Date:** 2026-11-03
- **Task:** P3-08 · Implements [ADR 0035](0035-services.md) §9 · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0018](0018-history-undo-uninstall.md), [0031](0031-kiln-new-laravel-projects.md), [0041](0041-redis.md), [0044](0044-service-web-uis.md) §4

## Context

A Kiln service is only useful once a project points at it. For Laravel, that means keys in
the project's `.env`. ADR 0035 §9 set the rules:

- only the instance's keys change, and every other byte stays;
- the user sees the old and new values first;
- `kiln undo` restores the file byte for byte, unless it was edited since;
- the project's database is made if missing;
- Laravel's drivers change only when asked.

Things the design has to meet:

- **Laravel's own `.env`:** with SQLite as its default, Laravel ships the other databases'
  keys commented out (`# DB_HOST=127.0.0.1`).
- **The MariaDB connection:** it exists from Laravel 11 on. Older projects reach MariaDB
  through `mysql`.
- **phpredis:** Laravel's default Redis client, `phpredis`, is a PHP extension that Kiln's
  Windows PHP doesn't have. Spike 03 used `predis`, a Composer package.
- **Users edit `.env` all the time.** Uninstall undoes every journaled change and stops at
  the first one it can't
  ([ADR 0018](0018-history-undo-uninstall.md)). An edited `.env` would block it.

## Decision

1. **`kiln env use <instance> [--project <folder>] [--database <name>] [--cache]
   [--session] [--queue] [--yes]`** sets the instance's keys in the project's `.env`:
   - It shows each key's old and new value, then asks.
   - It is journaled (`project.env.keys`), so it appears in `kiln history` and
     `kiln undo` reverses it.
   - A project without `.env` is refused, with a hint to copy `.env.example`.
2. **Keys come from the provider** (`env_keys()`):

   | Kind | Keys |
   |---|---|
   | MySQL, MariaDB | `DB_CONNECTION` (`mysql` or `mariadb`; `mysql` if the project has no `mariadb` connection), `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME=root`, `DB_PASSWORD=` |
   | PostgreSQL | `DB_CONNECTION=pgsql`, the same keys, `DB_USERNAME=postgres` |
   | Redis | `REDIS_CLIENT=predis`, `REDIS_HOST`, `REDIS_PASSWORD=null`, `REDIS_PORT`; with `--cache`, `--session`, `--queue`: `CACHE_STORE`, `SESSION_DRIVER`, `QUEUE_CONNECTION=redis` |
   | Mailpit | `MAIL_MAILER=smtp`, `MAIL_HOST`, `MAIL_PORT` (its SMTP port) |
   | Meilisearch | `MEILISEARCH_HOST`, `MEILISEARCH_KEY=` |
   | RustFS | `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_DEFAULT_REGION`, `AWS_ENDPOINT`, `AWS_USE_PATH_STYLE_ENDPOINT=true`; the bucket stays the project's choice |
   | MongoDB | `MONGODB_URI` |

   - **Redis:** if `vendor/predis/predis` is missing, Kiln says to run `composer require
     predis/predis`. It doesn't run Composer in the user's project itself.
3. **Edits are key-level** (`kiln-platform`'s `env_file`). For each key, in order:
   - **an existing setting:** its line is rewritten in place, keeping its indentation and
     `export `. Dotenv loaders keep the first definition, so the first line is the one
     changed;
   - **a commented-out example** (`# DB_HOST=…`): it becomes the setting, in its place;
   - **neither:** a new line after the last key of the same group (`DB_`, `REDIS_`…), or
     at the end after a blank line.

   **Values** are written plain when they are plain; otherwise double-quoted, with `"`,
   `\` and `$` escaped. **Line endings** follow the file (CRLF stays CRLF).
4. **Undo:**
   - The receipt keeps the whole file before and after, and each key's old value.
   - If the file is as Kiln left it, undo writes the old bytes back exactly.
   - If it was edited since, undo stops and says so. `kiln undo --force` then sets back
     only the keys that still hold Kiln's values, and leaves the user's edits.
5. **Databases:** for MySQL, MariaDB and PostgreSQL (`has_databases()`), Kiln makes the
   database through the instance's own client if it's missing.
   - Its name is the project folder's, in lowercase letters, digits and `_` (`my-shop`
     becomes `my_shop`), or `--database`.
   - Names are checked before they go into SQL.
   - A database is data, so undo doesn't drop it.
   - The instance must be running.
6. **`kiln new <name> --db <mysql|mariadb|pgsql|instance>`:**
   - The database is chosen before Composer runs: a kind means its first running
     instance, otherwise Kiln stops and says to add one.
   - After the project is created and served, it is wired as above and
     `php artisan migrate --force` runs with the project's PHP.
7. **Uninstall leaves projects' `.env` files as they are.** They are the projects' own
   data, not system changes.
   - The plan lists them separately, with their operation numbers, so a user can
     `kiln undo` them first.
   - They are not counted as unfinished operations, so they don't keep the state folder.

## Consequences

- One command points a project at any Kiln service, and the change can be seen in
  `kiln history` and undone exactly.
- After uninstall, a wired project still points at services that are gone. That's what
  the user's own edits would also leave, and the plan says so before anything happens.
- **CI** creates a Laravel app with `kiln new --db`. It wires the app to Redis (cache,
  session, queue) and Mailpit, then runs spike 03's Laravel checks on Kiln's Redis with
  the queue's `failed_jobs` in MySQL. It also sends a mail through Mailpit, undoes the
  wiring and compares the file's bytes, and checks uninstall left the `.env` alone.

## Alternatives considered

- **Rewriting `.env` from a template.** Rejected: it loses the user's comments, order and
  other keys.
- **Undo by setting keys back, always.** Rejected as the default: it can't restore
  removed comments or the exact layout. Kept as the `--force` path for an edited file.
- **phpredis.** Not available in Kiln's Windows PHP builds; `predis` is pure PHP and
  what spike 03 checked.
- **Running `composer require predis/predis` automatically.** Rejected: it changes the
  project's `composer.json` and lock file, which is the user's decision.
- **Uninstall reverting the wiring too.** Rejected: an edited `.env` would stop uninstall,
  and the project's settings are the user's.
