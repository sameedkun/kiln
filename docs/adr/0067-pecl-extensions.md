# 0067. PECL extensions for Kiln's PHP on Windows, phpredis first

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4-20 · Builds on [ADR 0030](0030-php-extension-toggles.md), [0061](0061-xdebug.md)

## Context

The user asked for phpredis (2026-11-13). windows.php.net's PHP builds ship the bundled
extensions only; PECL ones such as phpredis come separately. The spike found:

- PHP's own PECL Windows builds live at `downloads.php.net/~windows/pecl/releases/`
  (`windows.php.net/downloads/pecl/…` redirects there). phpredis 6.3.0 has non-thread-safe
  x64 builds for PHP 8.2 and 8.3 (VS16) and 8.4 and 8.5 (VS17), like Kiln's PHP.
- **No checksums are published** for them, unlike PHP's own releases.
- Each zip holds `php_redis.dll`, loaded with `extension=redis` like a bundled extension.
- phpredis is licensed under the PHP License 3.01.

## Decision

1. **Pinned builds.** Kiln pins phpredis 6.3.0's four builds by file name and SHA-256. Since
   none are published, the sums were computed on a fresh GitHub runner by a manual workflow,
   "Pin downloads" (`.github/workflows/pin.yml`), which downloads given HTTPS URLs and prints
   each SHA-256; the Scenario downloads every pinned file again and checks it. The same
   workflow pins later extensions.
2. **`kiln php ext add redis [--php X.Y]`** downloads the build for that PHP through the
   checked cache, unpacks it (paths can't leave the folder), copies `php_redis.dll` into that
   PHP's `ext` folder, then turns it on with `kiln php ext on`: one journaled line in
   `php.ini`, undoable from History, the workers restarted. From then on it is a switch like
   the bundled ones.
3. **Kept across updates.** `kiln php update` places the extension in the new patch's `ext`
   folder before carrying the toggles over, so phpredis stays on.
4. **Windows only for now**; on Linux, the distribution's `php-redis`.
5. **In the app:** the PHP page's extensions list offers "Add phpredis" until it is there.
   Laravel uses it with `REDIS_CLIENT=phpredis`; Kiln's wiring keeps writing `predis`, which
   needs no extension, and the user changes it when they want phpredis.

## Consequences

- phpredis works with Kiln's PHP without hunting for DLLs.
- A computed pin is trust on first download from php.net over HTTPS, then checked on every
  download after; it is weaker than a published checksum, and says so here.
- imagick and others follow the same path, each with its own pins (imagick also needs
  ImageMagick's DLLs next to PHP, so it gets its own spike).
