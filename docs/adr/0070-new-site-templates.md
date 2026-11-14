# 0070. New sites from templates

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-06 · Builds on [ADR 0031](0031-kiln-new-laravel-projects.md)

## Context

`kiln new` makes a Laravel project from `laravel/laravel` with Kiln's PHP and Composer and
serves it (ADR 0031). The app had no way to start a project, and Laravel's own starter kits
(React, Vue, Livewire) and other frameworks weren't offered. The user asked for "a new site"
in the app with a framework to choose, Laravel's starter kits among them (2026-11-14).

## Decision

1. **`kiln new <name> --template <id>`**, `laravel` by default. Templates are a fixed table in
   the CLI; only the project's name comes from the caller:

   | id | Composer package | Notes |
   |---|---|---|
   | `laravel` | `laravel/laravel` | as before |
   | `react`, `vue`, `livewire` | `laravel/react-starter-kit`, `laravel/vue-starter-kit`, `laravel/livewire-starter-kit` | Laravel's starter kits; their front end is built once |
   | `symfony` | `symfony/skeleton` | |
   | `bedrock` | `roots/bedrock` | WordPress through Composer |
   | `empty` | none | a folder with `public/index.php` |

2. **`--db`** stays for the Laravel templates (it points `.env` at an instance and runs the
   migrations); with another template it's refused before anything is made.
3. **A starter kit's front end** is built once with Kiln's Node for the project (its
   `.nvmrc` or the default): `npm install`, then `npm run build`, through Node's own
   `npm-cli.js` rather than a `.cmd` file. Without Kiln's Node, or if npm fails, the project
   is still made and served, and Kiln says what to run.
4. **The app** has "New site from a template…" under "Add a site": name, template (with what
   each gives), folder (a parked folder by default), PHP, database for Laravel, HTTPS. The
   working toast follows the run's steps and Composer's lines; it can take minutes.

## Consequences

- Composer and npm download from Packagist and npm's registry, as `composer` and `npm` do;
  Kiln pins neither (they are the project's dependencies, not Kiln's).
- Plain WordPress (not Bedrock) and other CMSs can come as templates later, each a row.

## Amendment (P4D-07, 2026-11-15)

The user asked for WordPress as wordpress.org installs it, set up to run. **`--template
wordpress --db <MySQL or MariaDB instance>`** downloads WordPress 7.1.4 from
downloads.wordpress.org (pinned by SHA-256 computed on a runner; CI also checks
wordpress.org's SHA-1), unpacks it into the project folder, makes its database (the project's
name, `-` as `_`) on that instance, and writes `wp-config.php` from WordPress's own sample:
the database, its login and address, and fresh random keys and salts. The site then opens on
WordPress's installer (title and admin account). `--db` is required for it, and refused for
PostgreSQL. Bedrock stays a CLI template. The app chooses a framework, then (for Laravel
only) a starter kit, and for WordPress a MySQL or MariaDB instance.
