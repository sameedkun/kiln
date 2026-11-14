# 0063. Kiln's footprint: memory, CPU and disk, and clearing the download cache

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4-16 · Builds on [ADR 0052](0052-phase-4-screens-and-scope.md)

## Context

A tool that runs in the background should say what it costs. ADR 0052 put a footprint view in
Phase 4: memory and CPU per Kiln process, disk use by folder, and clearing the download cache.

Everything Kiln runs is a program in a folder Kiln owns: its own binaries next to `kiln`, and
under `%LOCALAPPDATA%\Kiln` the web server (`bin`), PHP (`php\<version>`), Node (`node`) and
the services (`services\bin\<kind>\<version>`). Project processes (a queue worker, Vite) run
Kiln's PHP or Node. So a process is Kiln's when its program is in one of those folders.

The download cache (`downloads`) keeps the verified archives Kiln installed from. Nothing reads
it after the install; it only saves downloading again.

## Decision

1. **`kiln footprint`** (a read; `footprint.get` through the API) answers with:
   - **processes**, grouped by what they are: Kiln's daemon, app and supervisor, the web
     server, each PHP version's workers, project processes on PHP or Node, and each service
     with its version. Each group has its count, memory in use (working set) and CPU use,
     measured over half a second as a share of the whole machine. The command itself isn't
     counted;
   - **folders**: PHP, Node, Kiln's programs, the services' programs and their data, the
     download cache, the web server's files, logs, Xdebug, and Kiln's settings and history,
     each with its size (links aren't followed);
   - the totals, and the cache's size.
   On Windows it reads processes through `kiln-sys` (a process snapshot, then each process's
   memory and processor time with query-only rights). Elsewhere it lists folders only for now.
   WebView2's processes, which the app's window uses, are Windows' and aren't counted.
2. **`kiln cache clear`** deletes the download cache's contents and says how much it freed. It
   is the only thing the footprint offers to remove: everything else is installed software or
   the user's data, which have their own commands (`php remove`, `service remove`,
   `uninstall`). It isn't journaled: there is nothing to put back, only something to download
   again. It is on the API's allow-list.
3. **In the app:** Settings shows Memory and CPU (with Measure again) and On disk, with "Clear
   the download cache", which asks first and says how much it holds.

## Consequences

- The user can see what Kiln costs while it runs, and where its disk use goes.
- Clearing the cache is safe at any time: a later install or update downloads and checks its
  archive again.
- The Scenario checks the web server, PHP and MySQL show with their memory, PHP's folder and
  the cache have sizes, and that after `cache clear` the cache is empty and the site still
  serves.
