# 0048. `kilnd` and services: the watchdog brings them back; `service.*` in the API

- **Status:** accepted
- **Date:** 2026-11-11
- **Task:** P3-10 · Builds on [ADR 0026](0026-kilnd-daemon-and-local-api.md), [0035](0035-services.md) §3, [0036](0036-service-framework.md)

## Context

`kilnd`, Kiln's optional per-user daemon ([ADR 0026](0026-kilnd-daemon-and-local-api.md)),
keeps the web stack up:

- Every 15 seconds it looks at the web server's and the PHP supervisor's PID files. A
  clean stop removes a PID file, so one whose process is gone means a crash or a reboot.
- It repairs that by running the `kiln` program next to it, with growing waits between
  attempts.
- It answers a local JSON-RPC API.

ADR 0035 §3 asked the same of services: after a crash or a reboot, `kilnd` brings back
the services supervisor, and the API gains `service.*` methods.

Services run under their own supervisor (`<local data>/services/supervisor`), which
restarts a crashed instance itself. What it can't survive is its own end. When it exits,
the job object it holds ends every instance with it
([ADR 0036](0036-service-framework.md)), and after a reboot nothing starts it again.

## Decision

1. **The watchdog** checks the services supervisor on each pass, with its own backoff:
   - **A stale PID file** (exited on its own) triggers `kiln services resume`.
   - **A running supervisor** counts as healthy.
   - **No PID file** (stopped through Kiln, or never started) is left alone.
   - Each restart is logged, kept in `daemon.status` as `service_repairs`, and published
     as a `services.repaired` event.
2. **`kiln services resume`** (hidden; the daemon's command) starts the supervisor again
   with the instances that should be running. It changes no instance's enabled flag, so a
   stopped service stays stopped. It differs from `kiln services up`, which starts every
   instance.
3. **API methods:**
   - **`services.status`:** the supervisor's state, and every instance with its kind,
     version, ports, project, whether it should run, and whether its main port answers.
     Read from Kiln's state.
   - **`service.start`, `service.stop`, `service.restart`** with `{"name": …}`. As for
     every change the daemon makes, these run the `kiln` CLI, so there is one
     implementation and the change lock applies.
   - **Names are checked first.** The name must be an existing instance, so nothing else,
     such as an option, reaches the command line.
4. **`kiln daemon status`** adds a services line: how many of the wanted instances answer,
   or that the supervisor exited and is being restarted, plus the count of restarts.

## Consequences

- With `kilnd` running (and started at login, `kiln daemon enable`), services come back
  after a crash of their supervisor and after a reboot, as the web server does.
- Without `kilnd`, nothing changes: services run until stopped, and after a reboot
  `kiln services up` (or any service command) starts them again.
- A GUI (Phase 4) can list and control services through the API.
- **CI** starts the daemon and reads the services through the API, restarts `mailpit-1`
  through it, and checks that an unknown name is refused. It then kills the services
  supervisor (every instance goes down with it) and waits for the daemon to bring them
  back: Mailpit answers again with its mail, and `kiln daemon status` counts one restart.

## Alternatives considered

- **`kiln services up` as the repair.** Rejected: it starts every instance, including ones
  the user stopped.
- **The daemon running the services supervisor itself.** Rejected: the CLI already owns
  starting it (ports, configs, the change lock). The daemon only decides when.
- **Watching each instance from the daemon.** Not needed: the supervisor already restarts
  a crashed instance with its own backoff.
