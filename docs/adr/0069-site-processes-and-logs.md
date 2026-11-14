# 0069. A site's processes and their logs on its page; several queue workers

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-03 · Builds on [ADR 0059](0059-log-viewer.md), [0060](0060-dev-processes.md)

## Context

A site's dev processes (queue worker, scheduler, Vite, dumps) run as project processes
(ADR 0060). Their "Log" button left the site for the Logs page, and a site ran one queue
worker at most. The user asked for each process's log beside the app's own logs on the
site's page, as Lerd shows them, and for more than one queue worker (2026-11-14).

## Decision

1. **The site's Logs tab** has a row of choices: the app's logs (`storage/logs`, with its
   file list, ADR 0059 §4 as amended in P4C-01), then each of the site's processes by name
   ("Queue worker", "Queue worker 2", "Scheduler", "Vite", "Dumps"). A process's "Log" button
   opens this tab on it rather than the Logs page.
2. **Several queue workers** are several queue instances of the same project: the first
   keeps its name (`queue-shop`), the next are `queue-shop-2`, `queue-shop-3`, and so on, up
   to 8. Each is its own program in the supervisor, with its own log, restarted on its own.
   No change to the supervisor or the API: `service add queue --project … --name …` already
   does this, and the allow-list already checks `--project`.
3. The queue worker's row shows how many run, with buttons to add one (the next free
   number) and to remove the last one added; Start and Stop act on all of them.

## Consequences

- `queue:listen` runs each job in a fresh PHP process, so N workers cost N listeners plus
  the jobs; 8 is a sane ceiling on a laptop.
- Per-worker settings (queues, tries, timeout) can come later as service options.
