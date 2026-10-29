# 0029. PHP patch updates and rollback

- **Status:** accepted
- **Date:** 2026-10-29
- **Task:** P2-06 · Builds on [ADR 0013](0013-downloads-and-php-runtimes.md), [0014](0014-php-cgi-supervisor.md), [0016](0016-per-site-php-and-shims.md), [0022](0022-linux-php-runtimes-and-fpm.md), [0025](0025-own-static-linux-php-builds.md)

## Context

PHP ships a patch release of each supported line about once a month, often for security.
Today a user gets one by running `kiln php install 8.4` again. The new patch then sits
beside the old one, and sites pinned to `8.4` move to it at the next reload. But:

- **No single command** does it for every line, and nothing says what changed.
- **No way back:** if a patch breaks something, nothing returns to the previous one.
- **No previous build can be installed again:**
  - **Windows:** windows.php.net publishes checksums only for the newest patch of each line
    (`releases.json`). Its `archives/` folder has the older builds, but no checksums.
  - **Linux:** Kiln offers only the builds pinned in its source, and the `2026-10-27`
    release of `sameedkun/kiln-php` has the newest patch only.

The Phase 2 exit criterion asks for `kiln php update` followed by `kiln php rollback` to
round-trip, checked in the VM scenarios. That needs an older build to start from.

## Decision

1. **Kiln pins the previous patch of each line, on both systems:**
   - **Linux:** `sameedkun/kiln-php` now also builds exact patches on request. Its release
     `2026-10-29` holds 8.2.34, 8.3.35, 8.4.26 and 8.5.11, built and checked like
     `2026-10-27` ([ADR 0025](0025-own-static-linux-php-builds.md)).
     - Each pin now names its release.
     - Pins are added the same way as before: the release's `.sha256` files and a download
       in the test VM agree, and the build runs (`php -v`, `php -m`, `php-fpm -t`).
   - **Windows:** Kiln pins the SHA-256 of windows.php.net's archived build of the same
     patches.
     - Each was checked by downloading it from both of windows.php.net's addresses and by
       running it (`php -v`).
     - These are the only Windows builds whose checksum comes from Kiln instead of the
       index. `kiln php install 8.4.26` installs one.
   - **Why the previous patch:** it is the version a user most likely needs to go back to,
     and it lets the VMs test a real update. Each Kiln release moves the pins along; older
     patches stay refused, as before.
2. **`kiln php update [minor]`** updates every installed line, or the one named:
   - **What's newest:** the release index on Windows, the newest pinned build on Linux. On
     Linux, new patches therefore arrive with a Kiln update. A PHP from the user's
     distribution (`--system`) is skipped: their package manager updates it.
   - **Installed beside the current patch,** never over it, with the same verified install
     as `kiln php install`.
   - **What follows the update:**
     - `php.default`, if it names the old patch exactly, moves to the new one.
     - Sites and `kiln.toml` files that ask for a line (`8.4`) follow by themselves: they
       always get the newest installed patch.
     - A site pinned to an exact patch (`kiln isolate 8.4.26`) stays on it, and the update
       says so: an exact pin is a deliberate choice.
   - **Live:** a running server switches pools as for any change ([ADR 0014](0014-php-cgi-supervisor.md)
     and live reload): the new pool starts, Caddy reloads, the old pool retires.
   - **Rollback record:** Kiln records the patch it moved away from, per line
     (`php.rollback.<minor>`). The old patch stays installed: rollback needs nothing from
     the network.
   - **Already newest:** it says which patch is newest, and from where.
3. **`kiln php rollback [minor]`** goes back to the recorded patch:
   - **Which line:** the named one, or the only line with a record. With several, the
     user names one.
   - **Order:** the default moves back first. Then the newer patch is taken out of use, the
     pools switch back live, and only then is its folder deleted.
     - On Windows, files still held by an exiting worker are retried for a few seconds.
     - If one still can't be deleted, the folder is marked incomplete, so nothing uses it,
       and the next install of that version replaces it.
   - **Refused if a site is pinned exactly to the newer patch,** naming the site, so a
     rollback never moves a site the user pinned on purpose.
   - **The downloaded archive stays in Kiln's cache,** so `kiln php update` again is quick
     and offline.
   - **One step only:** the record is then cleared. A second rollback needs another update.
4. **Not journaled:** builds and their records live in Kiln's own folder and state, as for
   `kiln php install` ([ADR 0013](0013-downloads-and-php-runtimes.md)). `kiln php rollback`
   is the undo of an update; `kiln uninstall` removes all of it.

## Consequences

- **One command keeps every line current,** with an exit when a patch misbehaves.
- **Linux patches lag** until a Kiln release pins them. That is the price of SHA-256s pinned
  in Kiln ([ADR 0025](0025-own-static-linux-php-builds.md)).
- **Disk:** an update keeps the previous patch (about 120 MB on Windows, 60 MB on Linux)
  until a rollback or `kiln php remove`. The update names patches of that line nothing uses
  any more, with the command that frees their space.
- **Windows previous-patch pins** are checksums Kiln vouches for, not windows.php.net. They
  were checked from two addresses and by running each build.
- **Later:** the daemon can check for new patches (ADR 0026 lists PHP update checks among
  its timers) and say so. It won't update by itself.

## Alternatives considered

- **Update in place** (replace the folder): no way back, and a running pool would lose its
  files mid-request.
- **Rollback by reinstalling the old patch:** needs the network and a verifiable old build.
  Keeping the old folder is simpler and works offline.
- **Journaling updates for `kiln undo`:** the files are Kiln's own, not system state, and an
  undo that deletes a PHP build would be surprising among hosts-file and trust changes.
- **A test-only switch to fake an older index:** would test update without real older
  builds, but leaves a test hook in shipping code and proves less.
