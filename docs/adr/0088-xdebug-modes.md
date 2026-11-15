# 0088. Xdebug's other modes: profile and trace on demand, coverage for test runs

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-09 · Builds on [ADR 0061](0061-xdebug.md), [0084](0084-site-commands.md)

## Context

Kiln's Xdebug (ADR 0061) runs a debug worker beside each PHP's normal workers. A request
with the `XDEBUG_SESSION` trigger goes to it and connects to the editor; every other request
stays fast. Xdebug does more:
- **profile** writes a cachegrind file per request, for QCacheGrind or PhpStorm;
- **trace** writes every function call;
- **coverage** lets `pest --coverage` and `phpunit --coverage-*` measure tests.

Coverage needs Xdebug in the command-line PHP that runs the tests, not in a web worker.
Lerd offers these modes; the user asked for what Lerd has.

## Decision

1. **`kiln php xdebug mode <modes>`** chooses which modes the debug worker runs: any of
   `debug`, `profile` and `trace`, comma-separated (`debug` alone by default).
   - The setting is `xdebug.modes`, one for every PHP version.
   - It applies at once wherever Xdebug is on: the worker's `php.ini` is written again and
     the pools reloaded.
   - The worker still starts Xdebug only on a trigger (`start_with_request=trigger`), and
     each name starts what Xdebug says it does:
     - `XDEBUG_SESSION` (the browser helpers' cookie) starts step debugging only;
     - `XDEBUG_TRIGGER`, as a cookie or `?XDEBUG_TRIGGER=1`, starts every mode that is on;
     - the legacy `XDEBUG_PROFILE` and `XDEBUG_TRACE` start their own.
   - The web server sends all of these to the debug worker; other requests are untouched.
2. **Files** from profile and trace go to Kiln's own `xdebug\output` folder:
   `cachegrind.out.<time>.<request>` and `trace.<time>.<request>.xt`.
   - **`kiln php xdebug status`** (`--json`, the read `xdebug.status`) gives:
     - the PHP versions Xdebug is on for;
     - the modes;
     - the folder;
     - its files, newest first, with sizes.
   - **`kiln php xdebug clear`** deletes those files, and only those.
3. **Coverage** in `kiln run` (ADR 0084):
   - For a command with a `--coverage…` option, `kiln run` starts PHP with Kiln's pinned
     Xdebug loaded for that run only (`-d zend_extension=… -d xdebug.mode=coverage`, JIT
     off). Examples: `artisan test --coverage`, `pest --coverage`,
     `phpunit --coverage-html out`.
   - Xdebug's file is fetched and checked first if it isn't there, as `xdebug on` does.
   - Nothing changes for other commands or the web workers.
   - On Linux, the distribution's Xdebug is the user's (ADR 0061).
4. **The app's PHP page:**
   - the three modes as switches;
   - the output folder's files, with Open folder and Clear;
   - a note that the Commands tab's `--coverage` runs use Xdebug by themselves.

   `php xdebug mode` and `php xdebug clear` are on the allow-list.

## Consequences

- Profiling a slow page is one switch and one request with the trigger.
- `pest --coverage` works from Kiln without editing `php.ini`.
- Profile and trace files can grow large; they stay in one folder Kiln can clear.
