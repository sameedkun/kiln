# 0081. A doctor for one site (`kiln doctor <site>`)

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-02 · Builds on [ADR 0017](0017-kiln-doctor.md), [0045](0045-env-wiring.md),
  [0058](0058-history-doctor-settings.md), [0068](0068-service-pages.md),
  [0080](0080-setting-up-a-cloned-project.md)

## Context

`kiln doctor` checks Kiln: its ports, server, PHP, certificates and the hosts file. When one
site shows an error page, the cause is usually in the project instead. Typical causes:

- a key in `.env.example` that `.env` lacks;
- an empty `APP_KEY`;
- `vendor/` missing after a clone;
- a database that was never made;
- a `.env` pointing at a stopped instance;
- a PHP older than `composer.json` allows;
- a missing extension.

Lerd has a per-site check. The user asked for what Lerd has (2026-11-15).

ADR 0017 keeps the doctor read-only, with each fix a command; `doctor --fix` stays out.

## Decision

1. **`kiln doctor <site>`** checks one site; `kiln doctor` alone is unchanged.
   - `<site>` is a site's name (`shop` or `shop.test`), or a folder when it has a path
     separator or is `.`. A folder Kiln doesn't serve gets every check but the answer.
   - The output, `--json` and the exit code are as in ADR 0017: grouped by area, and 1 if
     anything failed.
2. **It only reads.** It doesn't run Composer, npm or `artisan`, and doesn't open a database
   with the project's credentials. It opens Kiln's state read-only, as `kiln doctor` does.
   - Two things run code:
     - the site's PHP, as `php -m`, to list its extensions;
     - one request to the site, which runs the project's code as any visit does.
3. **Checks, by area:**

   | Area | Checks |
   |---|---|
   | Project | what it is (Laravel, WordPress, static, other, from the site's driver); for all but a static site, the PHP Kiln picks against `composer.json`'s `require.php`; each `ext-*` it requires against the extensions that PHP loads |
   | Environment file | (Laravel) `.env` exists; keys in `.env.example` that `.env` lacks (a warning) and the other way round (a note); keys set twice, by line, the last one being what Laravel reads; `APP_KEY` set; `APP_URL` against the site's URL |
   | Packages | `vendor/` installed when there is a `composer.json`, and not older than `composer.lock`; `node_modules/` when there is a `package.json`, and not older than its lock file; the storage link |
   | Database | the `.env`'s database. SQLite: the file exists. MySQL, MariaDB or PostgreSQL on this computer: the Kiln instance on that port is running and has the database. Redis and mail: the instance on that port is running |
   | Answer | (a served site) one `GET /` to Kiln's web server on 127.0.0.1, with the site's name as `Host`. HTTPS is checked against Kiln's root only. No redirects are followed, timeouts are 10 s, and at most 64 KB is read. A 5xx adds the last error line from `storage/logs/laravel.log` |

4. **Unknown is never fine** (I-55):
   - These are said as "not checked", with why:
     - a database Kiln doesn't run;
     - a stopped instance whose databases can't be listed;
     - a `require.php` Kiln can't read;
     - a stopped web server.
   - A host off this computer is "not Kiln's; not checked".
5. **Secrets stay out:** findings name keys, never values. `APP_KEY` is only "set" or
   "empty". The JSON reaches the app; the MCP server's doctor tool stays Kiln's own check,
   with no site.
6. **Fixes:**
   - A finding's fix is a command, as before. When that command is Kiln's own and on the
     API's allow-list, the finding also carries it as arguments (`run`).
   - Two show their preview and ask, and the app runs them through the usual preview and
     confirmation:
     - `kiln init <folder>` for `.env`, the key, `vendor/`, `node_modules/` (npm only), a
       missing SQLite file or storage link (it does only what's missing, ADR 0080);
     - `kiln env use <instance> --project <folder>` when the `.env`'s database port has
       nothing on it, preferring a running instance.
   - The rest make their change without a preview, so the app asks in words first:
     - `kiln service start` and `kiln service restart <instance>`;
     - `kiln db create <instance> <database>`;
     - `kiln php ext on <name> --php <version>`;
     - `kiln php install 8.4`;
     - `kiln web start`.
   - A test pins this split to the allow-list.
   - Everything else is text, for example:
     - "add these keys from `.env.example`";
     - "`composer install`" (composer.lock changed);
     - "`pnpm install`" (another package manager's lock file).
   - Copying keys from `.env.example` isn't automated: their values are the project's choice.
7. **The PHP range** is read with a small parser for Composer's constraints:
   - `|` and `||` for or; a space or comma for and;
   - the operators `>=`, `>`, `<=`, `<`, `!=`, `=`, `^`, `~`;
   - `8.2.*` and `@stability`.
   Anything else is "not checked".
8. **The app:** the site page gets a **Check** tab, reading `site.doctor` (validated as a site
   name). Findings come problems first, with each fix's Run button where it has one.

## Consequences

- "Why does this site fail?" gets one answer, and the clone case points at `kiln init`.
- The project's code runs once, through the same request a browser would make.
- `.env` keys set twice are read as Laravel reads them, the last one. Kiln's
  `env_file::value_of` reads the first ([I-102](../issues.md#i-102)).

## Alternatives considered

- **Running `artisan about` or `migrate:status`:** they show more, but run the project's
  commands and can prompt. A request does that already, once.
- **Connecting to the database with the `.env`'s credentials:** that would reach servers that
  aren't Kiln's. Only Kiln's instances are asked, with Kiln's own client.
