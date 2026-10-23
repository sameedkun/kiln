# 0016. Per-site PHP versions; `php` and `composer` shims

- **Status:** accepted
- **Date:** 2026-10-22
- **Task:** P1-11 · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0013](0013-downloads-and-php-runtimes.md), [0014](0014-php-cgi-supervisor.md), [0015](0015-parked-folders-and-drivers.md)

## Context

One PHP version serves every site so far (`php.default`, [ADR 0014](0014-php-cgi-supervisor.md)
§8). Real projects pin different versions: a legacy app on 8.1, a new one on 8.4. Two places
need the right version:

1. **The web server:** each site's PHP requests must reach workers of its version.
2. **The terminal:** `php artisan …` or `composer install` in a project folder must use the
   same version as the site. Otherwise dependencies resolve against the wrong PHP.

Windows has no per-directory interpreter switching. The usual answer is a small program named
`php.exe` on `PATH` that looks at the current folder and runs the right PHP: a **shim**. Putting
it on `PATH` changes which `php` every new terminal uses. Laravel Herd, if installed, also puts
its PHP on the user's `PATH`. So this must be visible, consented and reversible.

## Decision

### Isolation

1. **`kiln isolate <version> [site]`** pins a site to a PHP version; `kiln unisolate [site]`
   removes the pin.
   - The site defaults to the one whose project folder contains the current folder.
   - `<version>` is a minor (`8.3`) or an exact version (`8.3.36`).
   - A minor means "the newest installed 8.3.x", so installing a patch update moves the site
     along.
   - The version must be installed when isolating. Kiln suggests `kiln php install 8.3`.
2. Pins live in a new `isolations` table (host → version spec, migration v5), keyed by host. It
   works the same for linked and parked sites; parking stays a pure folder listing.
3. If a pinned version is later removed, the site falls back to the default version.
   `kiln sites` and `kiln web start` warn about it. Failing would take every other site down
   with it.
4. `kiln sites` shows each site's PHP version, marking pins.

### One pool per version in use

5. Pools are planned from the sites:
   - the default version, if any PHP site isn't pinned (or no site exists yet);
   - one pool per other version that a site is pinned to.

   Each pool has `php.workers` workers. Its id is the full version, e.g. `8.3.36`.
6. Caddy routes each site to its own pool's upstreams. `WebConfig` holds the pools
   (id → upstreams) and `SiteSpec` names its pool. A static site, or a site whose pool isn't
   running, gets the plain file server.
7. **Changing pins on a running server reuses running pools.** A version already running keeps
   its workers and ports, so its sites see no change.
   - New versions get new pools. Kiln waits until they listen, reloads Caddy, waits
     `RETIRE_GRACE`, then retires unused pools ([ADR 0014](0014-php-cgi-supervisor.md) §11).
   - The same path handles `kiln php default` and `kiln php workers`.

### Shims

8. **`kiln-shim`** is a small binary.
   - `kiln shims install` copies it into `<local data>\bin\shims\` as **`php.exe`** and
     **`composer.exe`**.
   - It decides what to be from its own file name, so there are no `.cmd` wrappers, which
     mangle `^`, `%`, `!` and print "Terminate batch job?" on Ctrl+C.
9. **Resolution** (also shown by `kiln which`):
   - the site whose project folder contains the current folder (longest match) → its pinned
     version;
   - otherwise → the default version.

   `php.exe` runs that build's `php.exe` with the same arguments and the same console, and
   exits with its exit code. While the child runs, the shim ignores Ctrl+C, so PHP handles it.
   If no Kiln PHP is installed, it says so and how to fix it.
10. **Composer:** `kiln composer install` downloads the current stable `composer.phar`.
    - The version is from `https://getcomposer.org/versions`; the SHA-256 is from
      `/download/<v>/composer.phar.sha256sum`. The same TLS-anchored trust as PHP
      ([ADR 0013](0013-downloads-and-php-runtimes.md) §8).
    - It's stored as `<local data>\bin\composer.phar`. The `composer.exe` shim runs
      `<resolved php> composer.phar <args>`.
11. **`PATH`:** `kiln shims install` puts the shims folder **first** on the *user* `PATH`. It's
    a journaled operation (`env.user_path.prepend`), so `kiln undo` removes it.
    - The preview shows the new `PATH` order, and which `php` and `composer` new terminals
      resolve to now and after the change. If Herd or another PHP is ahead, the user sees it
      being displaced.
    - Kiln asks before applying.
    - `prepare` records whether the entry was already there. `revert` removes only Kiln's
      entry and leaves every other change the user made to `PATH` since.
    - The value keeps its registry type (`REG_EXPAND_SZ` stays expandable). Windows is told
      the environment changed (`WM_SETTINGCHANGE`), so new terminals see it; open terminals
      keep their old `PATH`.
    - Only the user's `PATH`, never the machine's, so no administrator rights are needed.
12. **Windows only** in P1-11. On Linux the same shims came with P1-04, with `PATH` set by a block in the shell startup files ([ADR 0023](0023-linux-shims-and-path.md)).

## Consequences

- `cd D:\Sites\legacy && php -v` reports 8.1 while `D:\Sites\new` reports 8.4, and the browser
  matches.
- Each extra version in use costs `php.workers` php-cgi processes.
- The `php` on `PATH` is Kiln's after `kiln shims install`, visibly and undoably. Herd's `php` is
  used again after `kiln undo`.

## Alternatives considered

- **`.cmd` shims:** argument mangling and the batch-job prompt.
- **Copying `php.exe` itself per project:** no single place to update, and many copies.
- **Changing the machine `PATH`:** needs elevation and affects other users.
- **Restoring the whole old `PATH` on undo:** would erase the user's own later edits.
