# 0036. The service framework: instances, providers, a second supervisor, Mailpit first

- **Status:** accepted
- **Date:** 2026-10-31
- **Task:** P3-01 · Builds on [ADR 0035](0035-services.md), [0014](0014-php-cgi-supervisor.md), [0011](0011-caddy-integration.md) §9–10, [0018](0018-history-undo-uninstall.md)

## Context

[ADR 0035](0035-services.md) sets the shape of Phase 3's services. P3-01 builds the part
every service shares, before any database:

- instances in the state database;
- the provider trait with its contract tests;
- the commands;
- the process that keeps services running.

A framework with only fake providers can't be checked in the VMs. Mailpit is the simplest
real service: one MIT-licensed executable for every platform, with no data folder to set
up and no graceful-stop protocol. So it becomes the first provider here. P3-07 keeps its UI
site (`mailpit.test`, [ADR 0044](0044-service-web-uis.md)) and the `MAIL_*` wiring.

## Decision

1. **The supervisor runs programs too.** `kiln-supervisor` gets a third pool kind,
   `program`, next to php-cgi and PHP-FPM ([ADR 0014](0014-php-cgi-supervisor.md)):
   - **The spec:** an executable, its arguments, environment and working folder, and a log
     file for its output.
   - **Readiness:** one entry, keyed by its main port. Any other ports it opens, such as
     Mailpit's web UI, are listed as also listening. Start-up waits until all of them
     accept connections.
   - **Stopping:** an optional stop command, such as `mysqladmin shutdown` later, run
     first. Then up to 10 s for the program to exit. Then, on Linux, SIGTERM and another
     wait. Only then a kill. A database must never be killed while it can still shut down
     cleanly.
   - **Shared machinery:** the crash back-off and, on Windows, the kill-on-close job, as for
     PHP. Nothing outlives the supervisor.
   - **A separate instance** for services, in `<local data>/services/supervisor`. `kiln web
     stop` and `start` don't touch it, and its readiness wait is 60 s, since a first
     database start can be slow.
2. **Instances** live in a new `services` table (schema v7):
   - name, kind, exact version, its ports by role (for example `smtp` and `http`), and
     whether it should be running;
   - `kiln service start` and `stop` set that flag. The supervisor's `pools.json` is
     written from the instances that should run, so what runs and what is recorded can't
     disagree.
3. **Folders:**
   - binaries: `<local data>/services/bin/<kind>/<version>/`, shared by instances of that
     version;
   - data: `<local data>/services/<name>/data`;
   - logs: `<local data>/services/<name>/<name>.log`.
4. **Providers** implement `ServiceProvider` in the new crate `kiln-services`:
   - its kind, display name and license;
   - its ports by role, with their usual numbers;
   - its pinned releases, each with an archive, URL, SHA-256 and the files it needs;
   - how to set up a data folder;
   - the program spec to run;
   - an optional stop command.

   **Contract tests** run every provider through the same checks:
   - well-formed pins;
   - unique port roles with usual numbers;
   - a program spec that names only loopback addresses and lives under Kiln's folders;
   - binaries installed under a Kiln-specific name.
5. **Kiln-specific executable names,** as for Caddy ([I-11](../issues.md#i-11)): Mailpit
   runs as `kiln-mailpit(.exe)`, so tools that clean up `mailpit` by name leave Kiln's
   alone. Later providers that can't be renamed must say why.
6. **Mailpit:**
   - **Release:** v1.31.6, pinned by SHA-256 for Windows and Linux on x64 and arm64.
     Mailpit publishes no checksum file. The pins are GitHub's own asset digests,
     re-checked by downloading the Windows archive and hashing it. The release had been
     out 16 days when pinned; newer ones are taken after they have settled the same way.
   - **Command line:** `--smtp 127.0.0.1:<smtp>`, `--listen 127.0.0.1:<http>` and
     `--database <data>/mailpit.db`, plus `--disable-version-check`, so it doesn't contact
     the internet by itself.
7. **Ports for now:** the usual port if it is free and no other instance has it, else the
   next free one above it, recorded with the instance. P3-03 adds naming who holds the
   usual port, and `kiln doctor`.
8. **Commands:**
   - `kiln service add <kind> [--version] [--name] [--port role=N]` downloads and checks
     the release if needed, sets up the data folder, records the instance and starts it;
   - `kiln service list | start | stop | restart | logs | remove`;
   - `kiln services up | down` for all instances.

   `list` and `logs` don't take the change lock
   ([ADR 0026](0026-kilnd-daemon-and-local-api.md) §1).
9. **Data from the start** ([ADR 0035](0035-services.md) §7):
   - `kiln service remove` deletes the data folder only with `--delete-data`, and otherwise
     says where it is and how large.
   - `kiln uninstall` lists the services' data in its plan, and refuses before changing
     anything while data exists, unless `--delete-data` is given.
   - `--keep-data` and `kiln db dump/restore` come with P3-11.

## Consequences

- Every later provider is a trait implementation plus its pins. The supervisor, the
  commands, the folders and the data rule are shared.
- Mailpit runs in the VMs from P3-01 on, so the framework is checked against a real
  program, ports and an uninstall.
- The VM scenarios' uninstall runs with `--delete-data` from now on. A check makes sure
  uninstall refuses without it.
- `kilnd` doesn't restart services yet. After a reboot they stay stopped until
  `kiln services up`; P3-10 changes that.

## Alternatives considered

- **A new supervisor binary for services.** Rejected. The reconcile loop, crash back-off
  and Windows job are what services need, already tested and shipped.
- **Services in the same supervisor as PHP.** Rejected. Stopping the web stack would stop
  the databases, and a PHP pool change would re-read the services' specs.
- **Starting with a fake provider only.** Rejected. Nothing would show that the framework
  works with a real program on a real machine.
