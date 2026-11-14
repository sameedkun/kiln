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
