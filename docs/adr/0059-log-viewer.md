# 0059. The log viewer: every log Kiln knows of, followed live

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-08 · Builds on [ADR 0053](0053-api-v2-and-json-output.md), [0054](0054-desktop-app-shell.md), [0056](0056-services-page.md)

## Context

The plan asks for a log viewer with the web server's log, PHP's errors per site, the
services' logs and `kilnd`'s, followed live. Each log already exists as a file:

| Log | Where |
|---|---|
| Kiln's daemon | `%LOCALAPPDATA%\Kiln\logs\kilnd.log` (rolled by the daemon) |
| The web server | `%LOCALAPPDATA%\Kiln\caddy\caddy.log` (JSON lines, rolled by Caddy) |
| PHP's errors | `%LOCALAPPDATA%\Kiln\supervisor\php-errors.log`: one file for every pool |
| PHP's workers | `%LOCALAPPDATA%\Kiln\supervisor\supervisor.log` |
| A service | the files its provider names, usually `<services>\<name>\<name>.log` |
| A Laravel site | its own `storage\logs\*.log` (one file, or one per day) |

PHP-FPM has one error log for all pools: a PHP error doesn't say which site it came from
unless the script's path does. A site's own errors, with their stack traces, are in its
framework's log.

Following needs repeated reads. Each read method of the API runs `kiln --json`, which is
fine for a page but not once a second.

## Decision

1. **`kiln logs`** (a read; `logs.sources` through the API) lists every log Kiln knows of, in
   three groups: Kiln (`kiln`, `web`, `php`, `php-workers`), Services (`service:<name>`, or
   `service:<name>/<file>` when an instance has several), and Sites (`site:<host>`, the
   newest `*.log` in the site's `storage\logs`, for sites that are not aliases). Each source
   has its id, label, path, whether it exists, its size and when it last changed.
   **`kiln logs <id> [--lines N]`** shows a log's last lines (200 by default), reading at most
   its last 1 MiB.
2. **PHP errors per site are the site's own log.** PHP's shared error log stays one source;
   splitting it by script path would guess. The site's page gets a Logs tab with its log.
3. **`logs.tail`** (`{source, after?, lines?}`) is read by the daemon itself, not through
   `kiln`:
   - without `after`, it returns the last `lines` complete lines (200 by default, at most
     5,000) and the offset after them;
   - with `after`, it returns the complete lines written since that offset (at most 1 MiB per
     call) and the new offset; a line still being written waits for the next call;
   - when the file is shorter than `after` (rolled or emptied), it says `reset` and returns
     the last lines of the new file.
   Only a log that `kiln logs` lists can be read: the daemon resolves the id from that list
   (kept for 10 seconds) and never takes a path from the caller.
4. **The Logs page** lists the sources by group on the left (a source without a file says
   "none yet") and shows the chosen one on the right. The app reads `logs.tail` about once a
   second while Follow is on, keeps the last 3,000 entries, and stays at the newest line
   unless the reader has scrolled up. Filters: Everything, Warnings and errors, Errors (with
   counts), and Find (text in a line). The file's path is shown under the log.
5. **Lines are read into time, level and text in the app**, for the formats Kiln's logs use:
   Caddy's JSON lines, Laravel's (`[date] env.LEVEL: text`, with stack traces joined to their
   entry), PHP's error log, Kiln's daemon (UTC, shown in local time) and workers (seconds since
   1970), Redis's, and ISO-timestamped lines (MySQL, MariaDB, PostgreSQL and others). Anything
   else is shown as it is, with a level only when the line names one.

## Consequences

- No page is a placeholder any more.
- A follow costs one small file read a second in the daemon, and nothing while the page is
  closed or Follow is off.
- The Requests tab (P4-18) will be a second view of the web server's log, once it records
  access lines with timings.
