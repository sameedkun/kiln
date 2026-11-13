# 0061. Xdebug on demand: a debug worker per PHP version

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-10 · Builds on [ADR 0014](0014-php-cgi-supervisor.md), [0016](0016-php-versions-per-site.md), [0030](0030-php-extension-toggles.md)

## Context

Xdebug makes every request slower when it is loaded, even when nothing is being debugged.
The plan asks for Xdebug per PHP version, "triggered per request (cookie or browser
extension); a separate debug worker so other requests stay fast".

Kiln's PHP on Windows comes from windows.php.net, whose builds don't include Xdebug. Xdebug
publishes Windows builds for each PHP minor and compiler on xdebug.org, with their SHA-256
sums. Its licence is the Xdebug License 1.03, based on the PHP License 3.01 (read from the
project's `LICENSE`).

Each PHP version Kiln serves runs as a pool of php-cgi workers on loopback ports, and Caddy
sends a site's `*.php` requests to its version's pool ([ADR 0016](0016-php-versions-per-site.md)).

## Decision

1. **Pinned builds, fetched when asked.** Kiln pins Xdebug 3.5.3's non-thread-safe x64 builds
   for PHP 8.2 to 8.5 (the VS16 builds for 8.2 and 8.3, VS17 for 8.4 and 8.5, matching the
   PHP builds), by file name and SHA-256 from xdebug.org. `kiln php xdebug on [--php X.Y]`
   downloads the build for that PHP through the same checked cache as every other download,
   into `%LOCALAPPDATA%\Kiln\xdebug\3.5.3\`: Kiln's own folder, not PHP's `ext` folder, so
   `kiln php ext on xdebug` can't load it into every worker and `kiln php update` keeps it.
   Kiln doesn't ship Xdebug, so it needs no notice in the repository. CI downloads all four
   pins and checks their sums.
2. **A debug worker per PHP version.** With Xdebug on for a PHP minor, its pool gets a
   companion: one php-cgi as the pool `<version>+xdebug`, loading
   `%LOCALAPPDATA%\Kiln\xdebug\<version>.ini`. That file is the PHP's own `php.ini` followed
   by Xdebug's section, written again each time pools are planned, so extension toggles
   reach it; `kiln php ext on|off` restarts it with the plain workers. Xdebug's section:
   - `zend_extension` with the full path of Kiln's copy;
   - `xdebug.mode=debug` and `xdebug.start_with_request=trigger`: only a request that asks
     is debugged;
   - `xdebug.client_host=127.0.0.1` and `xdebug.client_port=9003`, the port editors listen on
     by default;
   - `opcache.jit=off`, since Xdebug and the JIT don't run together.
   Before turning it on, Kiln runs that PHP with the debug `php.ini` and requires Xdebug
   among its Zend modules with no start-up warning about it; otherwise it stays off and the
   output is shown.
3. **Caddy routes only triggered requests to it.** In a site's PHP route, after the
   front-controller rewrite and before the plain workers, a request for a `*.php` file goes to
   the debug worker when it carries a trigger:
   - the `XDEBUG_SESSION` or `XDEBUG_TRIGGER` cookie (what browser helpers set), matched by
     cookie name;
   - or `XDEBUG_SESSION_START`, `XDEBUG_SESSION` or `XDEBUG_TRIGGER` in the query.
   Each trigger is its own matcher set (Caddy ORs the sets, and ANDs within one), each also
   requiring `*.php`. Everything else goes to the plain workers as before.
4. **On and off by PHP minor.** The setting `php.xdebug` lists the minors (`8.4`), so an
   update to a new patch keeps it. Like `php workers`, it changes only Kiln's own files and
   applies without downtime (the pools switch beside each other, then Caddy). `off` stops
   the debug worker; Xdebug's file stays for next time.
5. **Windows only for now.** On Linux, Kiln says to install the distribution's `php-xdebug`.
6. **In the app:** the PHP page's menu for each version has Turn Xdebug on or off (when a
   build is pinned for it), a tag marks the versions with Xdebug on, and a Debugging note
   says how to trigger it. `php list` reports `xdebug` and `xdebug_available`.

## Consequences

- Debugging needs no change to the site and costs nothing until a request asks for it.
- One debug worker per version: a second debugged request waits for the first, as it would
  in an editor stopped at a breakpoint.
- New PHP minors need a new pin; until then `xdebug on` says which versions have one.
- The Scenario checks a page says "plain" without a trigger and "debug" with the cookie and
  with the query (and "plain" for another cookie whose value merely mentions the name),
  that Xdebug's init packet reaches a listener on 127.0.0.1:9003, and that after `off` no
  debug worker is left.

## Alternatives considered

- **Loading Xdebug into every worker with `start_with_request=trigger`.** Rejected: loaded,
  Xdebug slows every request even when it isn't debugging.
- **Restarting the pool with Xdebug when a session starts.** Rejected: slow, and every site on
  that version would be debugged at once.
- **Keeping the DLL in PHP's `ext` folder.** Rejected: `php ext on xdebug` would load it into
  the plain workers, and `php update` would drop it.
