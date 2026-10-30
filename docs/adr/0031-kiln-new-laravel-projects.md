# 0031. `kiln new`: Laravel projects

- **Status:** accepted
- **Date:** 2026-10-30
- **Task:** P2-08 · Builds on [ADR 0015](0015-parked-folders-and-drivers.md), [0016](0016-per-site-php-and-shims.md), [0027](0027-project-config-kiln-toml.md)

## Context

Starting a Laravel app on Kiln today takes four steps:

1. `kiln composer install`, once.
2. `composer create-project laravel/laravel shop`, through the shim.
3. `kiln link`, in the new folder.
4. `kiln secure`, if the app should use HTTPS.

Herd and Valet do this in one command. Herd uses the Laravel installer (`laravel new`),
which is itself a Composer package.

The Phase 2 exit criterion asks for "`kiln new` produces a Laravel app served over HTTPS".

## Decision

1. **The command:** `kiln new <name> [--path <folder>] [--php <version>] [--secure]`.
   - **Where:** it makes `<folder>/<name>`. The folder defaults to the current one.
   - **The name** is also the site's name (`<name>.test`), so it must be a valid site name:
     lowercase letters, digits and hyphens.
   - **An existing `<name>` folder is refused,** even if it's empty, before anything is
     downloaded.
2. **Kiln's PHP and Composer:**
   - **Which PHP:** the one `--php` names (an installed `8.3` or `8.3.36`), else the default
     PHP. With no PHP installed, Kiln says to install one first.
   - **Composer:** if it isn't installed yet, Kiln installs it as `kiln composer install`
     does (current stable, SHA-256 checked) and says so.
   - **What runs:** `php composer.phar create-project laravel/laravel <name>
     --prefer-dist --no-interaction`, in the parent folder.
     - That PHP's folder goes first on `PATH`, so a script calling `php` gets the same
       version.
     - Composer's output is shown as it runs.
     - The Laravel skeleton's own scripts write `.env` and `APP_KEY`, create the SQLite
       database, and run the first migration.
3. **No Laravel installer and no starter kits.** `laravel/laravel` through Composer needs
   nothing beyond what Kiln already verifies. Starter kits (React, Vue, Livewire) need the
   installer's prompts and Node. A later task can add them.
4. **Served at once:**
   - **The new folder is linked** as `<name>.test`, as `kiln link` would. `--secure` serves
     it over HTTPS, as `kiln link --secure` does.
   - **If the parent folder is parked,** the project is already served as `<name>.test`.
     Kiln doesn't add a link; `--secure` just secures that site.
   - The URL is printed, with the hosts-file and trust hints `kiln link` gives.
5. **Its PHP is the project's:** with `--php`, Kiln also writes the project's `kiln.toml`
   (`[php] version = "<version>"`, [ADR 0027](0027-project-config-kiln-toml.md)). The site
   and the shims then keep using that version. Without `--php`, no `kiln.toml` is written,
   and the project follows the default.
6. **On failure** (network, a PHP extension missing, Composer's own errors):
   - `kiln new` stops with Composer's exit code, and nothing is linked.
   - It names any partial folder for the user to delete. Kiln doesn't delete files in the
     user's folders.
7. **Not journaled:**
   - The project is the user's files, made by Composer.
   - The link is Kiln's own state: `kiln unlink` removes it, and `kiln uninstall` forgets
     it.
   - Nothing system-wide changes.

## Consequences

- **One command** gives a working Laravel app at `http(s)://<name>.test`, on a PHP the
  user chose, with nothing installed beyond Kiln's own folder.
- **The Laravel version** is whatever `laravel/laravel` currently publishes for that PHP:
  Composer picks the newest one the PHP version supports.
- **It needs the network,** for Packagist and GitHub, as `composer create-project` always
  does.

## Alternatives considered

- **Bundle or download the Laravel installer:** one more package to pin and update, for
  prompts that Kiln's own flags cover.
- **Run `npm install` too:** the skeleton works without it until `npm run dev` or
  `npm run build`. Many users want their own package manager, so Kiln prints the next step
  instead.
- **Delete a partial project after a failure:** Composer often cleans up after itself. A
  wrong guess about what Kiln made would delete user files.
