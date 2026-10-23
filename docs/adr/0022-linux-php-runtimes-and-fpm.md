# 0022. Linux PHP: pinned static builds or the system's PHP, served by PHP-FPM

- **Status:** accepted
- **Date:** 2026-10-23
- **Task:** P1-04 (part c) · Builds on [ADR 0013](0013-downloads-and-php-runtimes.md), [0014](0014-php-cgi-supervisor.md), [0016](0016-per-site-php-and-shims.md), [0020](0020-linux-dns-and-privilege.md)

## Context

On Windows, Kiln installs PHP from windows.php.net, which publishes a SHA-256 for every
build ([ADR 0013](0013-downloads-and-php-runtimes.md)). Sites are served by php-cgi pools
under `kiln-supervisor` ([ADR 0014](0014-php-cgi-supervisor.md)). On Linux, `kiln php
install` refuses and sites are static.

Sources of PHP for Linux, checked on 2026-10-23:

- **static-php-cli** (`dl.static-php.dev`): single-file, statically linked `php` (CLI) and
  `php-fpm` builds of PHP 8.0–8.5 for x86-64 and AArch64. They run on any distribution and
  need nothing installed. Laravel Herd uses the same builds.
  - The `common` set has PDO for SQLite, MySQL and PostgreSQL, redis, gd, zip, curl,
    mbstring, bcmath and the rest of what Laravel needs, but not intl, opcache or sodium.
  - The `bulk` set adds opcache, intl, sodium, imagick and swoole, but **lacks `pdo_sqlite`
    and `pdo_pgsql`**, and a new Laravel app uses SQLite through PDO (checked by running both
    sets' `php -m`).
  - **No checksums or signatures are published.** The builds are UPX-compressed, and patch
    versions lag upstream (8.4.23 there against 8.4.27 on windows.php.net).
- **The distribution's own PHP** (`apt install php8.3-fpm`, `dnf install php-fpm`): verified
  by the package manager and patched with the system. But it is one version per release
  (Ubuntu 24.04: 8.3), unless the user adds a third-party repository such as Ondřej Surý's
  PPA.
- php-cgi isn't in the static builds. On Linux, PHP-FPM is the normal way to serve PHP, and
  it does what Kiln's supervisor does for php-cgi on Windows: it manages its own children,
  recycles them (`pm.max_requests`), and keeps the listening socket open throughout.

## Decision

### Where PHP comes from

1. **`kiln php install 8.4`** downloads static-php-cli's `common` CLI and FPM builds for this
   CPU. Their **SHA-256 checksums are pinned in Kiln's source**, as Caddy's are
   ([ADR 0013](0013-downloads-and-php-runtimes.md) §7):
   - Each pin was recorded from two separate HTTPS downloads (in the Linux test VM and on
     Windows) that had to agree, and the build was run before pinning. The trust rests on
     those recorded downloads, since the source publishes no checksums.
   - Projects that need intl, opcache or sodium use the distribution's PHP (2.).
   - A build without a pin can't be installed. New pins come with Kiln releases.
   - Pinned now: the latest `bulk` build of 8.2, 8.3, 8.4 and 8.5 for **x86-64**. AArch64
     builds are pinned once they can be tested.
   - Verified downloads, cache and extraction rules are the same as on Windows (`kiln-fetch`).
     Each archive holds one binary, installed as `php` or `php-fpm` (mode 0755) in
     `<local data>/php/<version>/`.
2. **`kiln php install 8.3 --system`** uses PHP the distribution installed. Nothing is
   downloaded, and nothing outside Kiln's folder is changed.
   - Kiln looks in fixed places: Debian/Ubuntu's `/usr/bin/php8.3` and
     `/usr/sbin/php-fpm8.3`, and Fedora/Arch's single `/usr/bin/php` and
     `/usr/sbin/php-fpm`.
   - It creates `<local data>/php/8.3.6-system/` with **links** to them and its own
     `php.ini`. Per-site versions, pools and shims then treat it like any other install.
   - `kiln php list` shows which versions are Kiln's and which are the system's.
     `kiln php remove` deletes only Kiln's folder, never the system's PHP.
   - Without a system PHP, Kiln says what to install, e.g. `sudo apt install php8.3-fpm
     php8.3-cli`.
3. **`php.ini`:** static builds have their extensions compiled in, so Kiln's file only sets
   development defaults, as on Windows: errors shown, memory limit, upload sizes. A system PHP
   still reads its distribution's extension files (`/etc/php/8.3/fpm/conf.d`), because `-c`
   replaces only the main `php.ini`, not the scan directory.

### Serving: one PHP-FPM master per version

4. Pools on Linux are **PHP-FPM pools** (`pools.json` gains `kind: fpm`). There is one master
   per PHP version in use, listening on one loopback port, and Caddy's upstream for that
   version is that one address ([ADR 0016](0016-per-site-php-and-shims.md) §6).
5. Kiln writes each pool's `php-fpm.conf` into its run directory:
   - `listen = 127.0.0.1:<port>`, `pm = dynamic`, `pm.max_children` = the `kiln php workers`
     setting (default 4), `pm.max_requests = 500`;
   - `clear_env = no`, so sites see the environment Kiln was started with, as with php-cgi;
   - logs in Kiln's supervisor folder;
   - no `user`/`group`: the pool runs as the user.
6. `kiln-supervisor` runs `php-fpm --nodaemonize --fpm-config <conf> -c <php.ini>` and
   restarts it if it exits, like a php-cgi worker. Two Linux specifics:
   - **The master can't outlive the supervisor:** it is started with `PR_SET_PDEATHSIG`
     (SIGTERM when the supervisor dies; `kiln-sys::linux`), the counterpart of Windows'
     kill-on-close job.
   - **It is stopped with SIGTERM**, which FPM passes on to its children, and killed only if
     it hasn't exited within 5 s. Killing the master alone would leave its children serving.
7. Switching versions live works as on Windows: a new pool starts beside the old one, Caddy
   moves over, and the old pool retires after the grace period
   ([ADR 0014](0014-php-cgi-supervisor.md) §11).

## Consequences

- `kiln php install 8.4` and per-site versions work on Linux like on Windows. Developers who
  prefer distribution packages can use `--system` and get the distribution's updates.
- Kiln's PHP downloads on Linux are pinned rather than checked against a published checksum.
  A compromised build pinned by mistake would stay trusted until Kiln drops the pin; that
  risk is noted for every pin update.
- Static builds can't load extra extensions (`pecl install`); projects needing one use
  `--system` with the distribution's extension package.
- PHP-FPM's own recycling replaces the staggered php-cgi limits of
  [ADR 0014](0014-php-cgi-supervisor.md), and [I-14](../issues.md#i-14) and
  [I-15](../issues.md#i-15) don't arise: the master keeps the socket open while children
  restart.

## Alternatives considered

- **System PHP only:** fully verified, but one version per distribution release without a
  third-party repository. Per-site versions are a core feature.
- **Downloads without a checksum (trust on each download):** against
  [ADR 0013](0013-downloads-and-php-runtimes.md) §1.
- **Building PHP on the user's machine:** minutes of compiling and a full toolchain.
- **php-cgi on Linux too:** not in the static builds, and FPM is the standard here.
- **The distribution's PHP-FPM service (`systemctl`):** needs root, is shared with
  everything else, and its pools are system configuration.
