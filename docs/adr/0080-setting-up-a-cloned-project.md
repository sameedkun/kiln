# 0080. Setting up a cloned project (`kiln init`)

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-01 · Builds on [ADR 0027](0027-project-config-kiln-toml.md),
  [0045](0045-env-wiring.md), [0046](0046-project-services.md), [0070](0070-new-site-templates.md)

## Context

`kiln new` makes a project and leaves it running. A project cloned from Git needs a dozen
steps first: it's served, Composer installs its packages, `.env` comes from `.env.example`
with a fresh key, a database is wired and migrated, the storage link is made, and the
front end is built. Lerd has one command for this (`lerd setup`); the user asked for Kiln to
have what Lerd has (2026-11-15). The pieces exist in Kiln (`link`, `services up`, `env use`,
the steps of `kiln new`); nothing chains them.

A cloned repository may be someone else's code: Composer scripts, npm's lifecycle scripts
and `artisan` all run it. And its `.env`, if it has one, may point at any database.

## Decision

1. **`kiln init [<folder>] [--db <database>] [--php <version>] [--secure] [--seed]
   [--no-build] [--yes]`** sets up the project in the folder (the current one by default).
   It first lists the steps it will take, each as the exact command, and asks once
   (`--yes` skips the question). It never asks in the middle: Composer and `artisan` run
   with `--no-interaction`, and choices are flags, which the app's dialog passes.
2. **The steps**, each skipped when the project shows it done (so a second run picks up
   where the first stopped, with no state file of Kiln's):
   1. **Served**: linked as its folder's name, unless a site serves it (parked or linked);
      `--secure` makes it HTTPS.
   2. **PHP**: the one Kiln picks for the folder (`kiln.toml`, an isolated site, else the
      default); `--php` writes it to `kiln.toml` first, as `kiln new --php` does.
   3. **Composer**: `composer install --no-interaction` when `composer.json` is there and
      `vendor/` isn't (`install`, never `update`: the lock file decides).
   4. **`.env`** (where `artisan` is): copied from `.env.example` only if there is no `.env`;
      `php artisan key:generate` only if `APP_KEY` is empty.
   5. **Services**: those `kiln.toml` lists, as `kiln services up` does; `--db` wires a
      database as `kiln env use` does (and `kiln new --db`). An SQLite `.env` gets its
      database file made if it's missing.
   6. **Migrations** (`php artisan migrate --force`), only against a database this run
      wired (`--db`, `kiln.toml`'s services, or the SQLite file it made); otherwise the command
      is shown for the user to run. `--seed` adds `--seed`.
   7. **The storage link** (`php artisan storage:link`) when `public/storage` is missing.
   8. **The front end**, when `package.json` is there and `node_modules/` isn't: with
      Kiln's Node, `npm ci` when `package-lock.json` is there (else `npm install`), then
      `npm run build` if it has a `build` script. A pnpm, Yarn or Bun lock file isn't
      overridden by npm: Kiln says what to run. `--no-build` leaves the front end out.
3. **What stops it**: the folder isn't a project (no `composer.json`, `artisan`, `index.php`
   or `public/`), linking fails, Composer fails, or the migrations fail. The storage link
   and the front end only leave a note with what to run, as in `kiln new`.
4. **Nothing is overwritten**: an existing `.env`, `APP_KEY`, `vendor/`, `node_modules/` or
   database file is left as it is. Linking and `.env` wiring are journaled as before; the
   project's own commands change only its folder.
5. **The app**: "Set up a project…" (Sites) picks a folder, shows the steps, and runs
   `kiln init` as a job with its output. It is on the API's allow-list as asking first, and
   an assistant over MCP can't run it without the user's consent.
6. **Not yet**: WordPress's `wp-config.php` (P4E-03), and importing DDEV's or Lando's
   settings.

## Consequences

- A clone is one command (or one dialog) from a working site, and running it again is safe.
- Code from the repository runs, as it would by hand; the list shown first says which
  commands. Kiln doesn't sandbox it.
- A `.env` that came with the repository is never migrated against unless the user wires a
  database of Kiln's.
