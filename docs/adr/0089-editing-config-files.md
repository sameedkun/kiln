# 0089. Editing php.ini, a site's .env and a service's config, checked, with backups

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-10 · Builds on [ADR 0038](0038-database-services.md), [0045](0045-env-wiring.md),
  [0056](0056-services-page.md), [0067](0067-pecl-extensions.md)

## Context

Raising `memory_limit` or `upload_max_filesize`, changing a `.env` value, or tuning MySQL's
`max_connections` means finding the file and opening an editor. A typo then shows up later,
as PHP start-up warnings, a site that won't boot or a database that won't start.

The app shows `.env` read-only (ADR 0056) and php.ini's extensions as switches (ADR 0067),
but edits nothing whole. Lerd edits these files in its UI; the user asked for what Lerd has.

A service's config is rewritten by Kiln before every start (ADR 0038), so it can't be the file
the user edits.

## Decision

1. **Targets**, each a file:

   | Target | File |
   |---|---|
   | `php:<version>` (`8.4` or `8.4.27`) | that build's `php.ini` |
   | `env:<site>` | the site's `.env` |
   | `service:<instance>` | the instance's own file: `custom.cnf` beside it for MySQL and MariaDB, read before Kiln's settings; `initdb`'s `postgresql.conf` for PostgreSQL, which Kiln's config already includes first |

   - Kiln's port, address and folders stay after the user's file, so they win, as wiring and
     coexistence need.
   - Other services have no editable file yet.
2. **Commands:**
   - **`kiln config show <target>`** (`--json`, the read `config.show`) gives the path and
     text.
   - **`kiln config save <target> --from <file> [--yes]`** shows which lines change (at most
     20, each old and new), asks, checks, keeps a backup, writes, and applies.
   - **`kiln config backups <target>`** lists the backups.
   - **`kiln config restore <target> <backup> [--yes]`** is a save from a backup.
3. **Checked** before anything is written, or, for a service, by starting it:
   - **php.ini:** PHP runs once with the new file, `php -c <new> -r "echo 'ok';"`. Any
     start-up warning or error stops the save, with PHP's words. Afterwards that version's
     workers restart, as an extension change does.
   - **.env:** every line is a comment, blank, or `KEY=value` with a valid name and balanced
     quotes. Errors name their lines.
   - **service:** it restarts. If its port doesn't answer within 30 seconds, the previous
     file goes back, the service is restarted on it, and the save fails with the end of its
     log.
4. **Backups:**
   - Before each save, the file as it was goes to Kiln's `backups\<target>\<time>.bak`.
   - The newest 10 per target are kept.
   - A restore is itself a save, so the file it replaces is backed up too.
   - They are Kiln's whole-file undo for these edits. The journal keeps Kiln's own key
     changes (ADR 0045), not hand-written files.
5. **The app** never sends a file's text on a command line, where it may not fit:
   - The method `config.stage {text}` writes the text to Kiln's `staged` folder, at most
     1 MB, and answers a token.
   - The app runs `kiln config save <target> --staged <token>` through the usual preview and
     confirmation. The staged file is deleted when used.
   - Through the API, `--from <file>` is refused: a preview would show any readable file's
     lines.
   - A server Kiln doesn't run (ADR 0091) has no target.
   - An unsaved edit stays in the editor for the session, so a save the check refuses can
     be fixed and tried again.
   - Editors:
     - the PHP page: Edit php.ini, per version;
     - the site page's `.env` tab: Edit;
     - a service's page: Edit settings, where there's a file.

     Each is a plain text area with Save and its backups with Restore.

## Consequences

- A broken php.ini or `.env` is refused before it lands. A service that won't start goes
  back to its last good file by itself.
- Ten backups per file are always one click from the editor.
- MySQL and MariaDB read one more file at start: an empty `custom.cnf` Kiln makes.
