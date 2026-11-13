# 0060. A project's dev processes: queue worker, scheduler and Vite

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-09 · Builds on [ADR 0036](0036-service-framework.md), [0047](0047-reverb.md), [0055](0055-sites-php-node-pages.md)

## Context

A Laravel project in development often needs three processes beside its site:

- a **queue worker**, so queued jobs, mail and notifications run;
- the **scheduler**, so scheduled commands run every minute;
- **Vite's dev server**, so the pages get their scripts and styles with hot reload.

Each is usually a terminal left open. Herd Pro and others run some of them for a project.

Reverb ([ADR 0047](0047-reverb.md)) already made a service that runs in a project: no
download, the project's own code, started by the services supervisor with the PHP Kiln picks
for the folder, kept up across crashes, its output in a log. Its consequences said a queue
worker or Vite could follow the same path. Two things didn't fit yet:

- the queue worker and the scheduler listen on no port, and the framework took every
  instance's first port as its main one, for starting, readiness and "is it running";
- Vite runs on Node, not PHP.

## Decision

1. **Three new kinds that run in a project:** `queue`, `scheduler` and `vite`. Providers say
   what they run on (`runtime()`: PHP or Node) and read the project's version of what they
   run (`project_version()`), or refuse with the command that sets the project up, which
   Kiln never runs:
   - **`queue`:** `php artisan queue:listen`. It starts a fresh worker for each job, so
     changed code is picked up without a restart. The queue is the one the project's `.env`
     names; `kiln env use redis-… --queue` already wires one. Version: the project's
     `laravel/framework`; a folder without `artisan` isn't a Laravel project.
   - **`scheduler`:** `php artisan schedule:work`, which runs the schedule every minute.
   - **`vite`:** `node node_modules/vite/bin/vite.js --host=127.0.0.1 --port=<port>
     --strictPort`, with the Node Kiln picks for the folder (its `.nvmrc`, `.node-version` or
     `kiln.toml`, else the default). Not `npm run dev`: ending npm can leave Vite running.
     Port 5173, or the next free one, like any service's port. `--strictPort` makes Vite
     refuse another port, so the `public/hot` file Laravel's Vite plugin writes names the
     port Kiln gave it. Version: `node_modules/vite/package.json`; without it,
     "run `npm install` there first".
2. **A process may listen on nothing.** The queue worker and the scheduler have no port:
   the supervisor runs them on port 0, waits for no address before counting them started,
   and Kiln counts them running while the supervisor that keeps them up runs. Only a
   process in a project may have no port (the contract tests check this).
3. **Stopping ends what a process started.** On Windows each program the supervisor runs
   gets its own kill-on-close job, nested in the supervisor's: stopping it closes the job
   and ends its children too (a queue worker's job processes, Vite's helpers), not only the
   process itself. Without the nested job (an older Windows), it still ends with the
   supervisor.
4. **What a stopped process leaves is tidied.** Ended rather than asked to stop, Vite can't
   remove `public/hot`, and the site would keep asking a stopped server for its scripts.
   After a Vite instance stops, Kiln removes `public/hot` only when it holds that instance's
   own address.
5. **Started only when asked.** These run the project's own jobs and scheduled commands, so
   Kiln starts them only from `kiln service add <kind> --project <folder>`, `kiln.toml`'s
   `[services]`, or a button; never on its own. Through the API, `service add` for a kind
   that runs in a project must name `--project`, and it must be a Kiln site's folder (as for
   `env.show`); otherwise the command would run in the daemon's own folder.
6. **In the app:** a site's page gets a **Dev processes** section: the queue worker and the
   scheduler for a Laravel site, Vite for any site. Each shows whether it runs, with Start
   or Stop, its log (the Logs page opens on it), and Remove. The Services page groups them
   under "Dev processes", without the port and wiring entries they don't have.

## Consequences

- One click runs a project's queue worker, scheduler or Vite, kept up across crashes and
  sign-ins like the databases, with its output on the Logs page.
- Laravel's Vite plugin must accept the site's origin for scripts served from
  `127.0.0.1`. Its recent versions allow `.test` sites; an older one may need its `cors`
  setting. Browsers treat `http://127.0.0.1` as secure, so an HTTPS site may load from it.
- CI runs the queue worker on the Scenario's app and checks a dispatched job runs and that
  no queue process is left after a stop; it runs the scheduler and checks Vite's refusal
  without `node_modules`. Vite itself is checked by unit tests: the Scenario has no Node.

## Alternatives considered

- **Made-up ports for the queue worker and the scheduler**, to keep every instance's main
  port. Rejected: a port nobody listens on would be waited for, reported and checked for
  conflicts.
- **`npm run dev` for Vite.** Rejected: on Windows npm runs through `npm.cmd`, and ending it
  can leave Vite holding its port.
- **`queue:work`.** Faster, but it keeps the code it started with: a developer would have to
  restart it after every change. `queue:listen` is the development choice.
- **Starting them with the site, by default.** Rejected: they run the project's code on
  their own (jobs, scheduled commands), which the user should choose.
