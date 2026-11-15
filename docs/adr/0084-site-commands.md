# 0084. A site's own commands from Kiln (`kiln run`), streamed and stoppable

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-05 · Builds on [ADR 0053](0053-api-v2-and-json-output.md),
  [0060](0060-dev-processes.md), [0079](0079-snapshots-and-tinker.md)

## Context

The commands a Laravel developer runs most, such as `php artisan migrate:status`, `composer test`,
`vendor/bin/pint` and `vendor/bin/pest`, need a terminal opened in the project with the right
PHP on `PATH`. Kiln knows the site's PHP and folder, and the app can stream a job's lines.
But the only project command in the app is Tinker (ADR 0079). Lerd runs these from its
site page; the user asked for what Lerd has.

A test run can take minutes, and `artisan queue:work` never ends on its own, so the user
needs a way to stop one. Today a job runs until it ends or hits its two-hour limit.
Stopping the `kiln` process alone would leave the `php` it started running.

## Decision

1. **`kiln run <site> <command> [args...]`** runs, in the site's folder, with the PHP Kiln
   picks for it (and that PHP first on `PATH`):

   | `<command>` | Runs |
   |---|---|
   | `artisan` | `php artisan …` |
   | `composer` | Kiln's Composer (installed if missing, as `kiln new` does) |
   | `vendor/bin/<tool>`, or a tool's bare name found there (`pint`, `pest`, `phpstan`, `phpunit`, …) | `php vendor/bin/<tool> …` |

   Nothing else: not `php`, not a path, not another program. It is the project's own code,
   as a terminal would run it; the app's API can't name an arbitrary executable.

2. **On a terminal**, the command has the console, so it can ask, show colour and be
   stopped with Ctrl+C.

   **With `--json`** (the app):
   - its standard input is empty;
   - `artisan` and `composer` get `--no-interaction`;
   - each output line is a `note` event as it comes;
   - the final result carries the exit code.

   A non-zero exit fails the command, so a failing test run shows as failed. It isn't a
   Kiln error: the output says why.
3. **`kiln run <site> --list`** (`--json`) says what the site has:
   - whether `artisan` is there;
   - `composer.json`'s scripts;
   - the tools in `vendor/bin`;
   - a few suggestions for the app's buttons, from what's present:
     - tests (`artisan test`, else `pest` or `phpunit`);
     - `pint` and `phpstan`;
     - `artisan migrate:status`, `artisan route:list` and `artisan about`.
4. **The tree stops with Kiln:**
   - On Windows, `kiln run` puts what it starts in a kill-on-close job object.
   - When `kiln` ends, by Ctrl+C or because it was stopped, Windows ends the command and
     every process it started.
   - **`jobs.stop`** (API, new) stops a running job, but only a `kiln run` job. Stopping any
     other job, mid-change, could leave a journal entry half-applied, so those can't be
     stopped.
5. **API and app:**
   - `run` is on the allow-list. It makes its change without asking: the user picked the
     command.
   - The site page gets a **Commands** tab:
     - the suggestions as buttons;
     - a field for any `artisan`, `composer` or `vendor/bin` command;
     - the output streaming in as it comes;
     - Stop while it runs.
   - The MCP server doesn't get it.

## Consequences

- `pest`, `pint` and `migrate:status` are a click from the site page, with the site's PHP.
- Long and endless commands can be stopped, without leaving PHP processes behind.
- Interactive prompts can't be answered from the app; `--no-interaction` makes them take
  their defaults or fail with a message.
