# 0082. `kiln env use` for WordPress (`wp-config.php` and Bedrock's `.env`)

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-03 · Builds on [ADR 0045](0045-env-wiring.md), [0070](0070-new-site-templates.md)

## Context

`kiln env use <instance>` points a project at one of Kiln's services by editing its `.env`,
as a journaled change that `kiln undo` reverses byte for byte (ADR 0045). A WordPress site
has no `.env`: its database settings are `define( 'DB_NAME', '…' );` lines in
`wp-config.php`. Kiln writes that file when it makes a WordPress site (ADR 0070), but it
can't repoint an existing one. `kiln env use` stops with "it keeps its database settings in
wp-config.php".

Bedrock (Roots' WordPress layout) has a `.env`, with WordPress's names: `DB_NAME`, `DB_USER`,
`DB_PASSWORD`, `DB_HOST`. `env use` writes Laravel's names there, which Bedrock ignores.

Lerd writes WordPress's config for its services; the user asked for what Lerd has.

## Decision

1. **`kiln env use <instance>` decides by what the project has:**

   | The project has | Kiln writes |
   |---|---|
   | a `.env` and is Bedrock (`web/wp`, or `roots/wordpress` in `composer.json`) | WordPress's names in `.env` |
   | a `.env` | Laravel's names, as before |
   | `wp-config.php` and no `.env` | constants in `wp-config.php` |

2. **The constants**, for MySQL and MariaDB:
   - `DB_NAME`: the database, made if missing, as before;
   - `DB_USER`;
   - `DB_PASSWORD`;
   - `DB_HOST`: `127.0.0.1:<port>`, as WordPress takes the port in the host.

   For Redis, `WP_REDIS_HOST` and `WP_REDIS_PORT`: the constants of the common object-cache
   plugin (Redis Object Cache). Kiln doesn't install the plugin; it says so.

   Other services stop with what to do by hand. For example, WordPress sends mail with PHP's
   `mail()`, so pointing it at Mailpit takes an SMTP plugin.

   Bedrock gets the four database names in `.env`, through the existing `.env` change. It
   gets no Redis.
3. **Editing `wp-config.php`** is a new journaled operation, `project.php.defines`:
   - It rewrites the value of each constant's first `define` in place, the one PHP keeps.
     Every other byte stays, including comments, quoting style elsewhere, and line endings.
   - A `define` on a commented line is never touched.
   - A missing constant goes on its own line above WordPress's "That's all, stop editing!"
     comment, else above the line that loads `wp-settings.php`. With neither, the preview
     fails before asking.
   - Values are written single-quoted and escaped; a `…_PORT` is written as a number.
   - The receipt and undo work as for `.env` (ADR 0045 §3):
     - an untouched file gets its old bytes back;
     - an edited one only with `--force`;
     - with `--force`, only where Kiln's value is still there: the old literal goes back
       exactly, and a line Kiln added is removed.
4. **Like `.env` wiring**, the preview shows the old and new values. History describes it.
   Uninstall leaves it in effect, as a project file, and says so.
5. **Not yet:** Drupal's `settings.php` (an array, not constants), and the `--cache`,
   `--session` and `--queue` switches, which are Laravel's.

## Consequences

- A WordPress site moves between Kiln's MySQL and MariaDB like a Laravel app, with undo.
- `wp-config.php` keeps its shape; a hand edit after Kiln's is never overwritten by undo
  without `--force`.
- `env show` and the site doctor still read `.env` only. For a plain WordPress site, the
  services page doesn't yet list it as using the database.
