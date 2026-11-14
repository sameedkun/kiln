# 0071. pgAdmin on request

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-07 · Builds on [ADR 0040](0040-postgresql.md), [0044](0044-service-web-uis.md); replaces WhoDB (P4-12, dropped)

## Context

The user dropped WhoDB and asked for pgAdmin as Lerd offers it: on PostgreSQL's page, added
only by those who want it (2026-11-14). Adminer already covers PostgreSQL. pgAdmin's own
Windows installer is a desktop app, which the user wants to avoid, and is large.

A look at the zip Kiln already pins for PostgreSQL 18.6 (its table of contents read by range
request, nothing run) found:

- `pgAdmin 4/` is 777 MB unpacked: `runtime/` (its desktop shell, 384 MB), `python/` (an
  embedded Python 3.13 with pgAdmin's packages, 284 MB), `web/` (pgAdmin 9.18, 60 MB),
  `docs/` (48 MB).
- `python/python313._pth` fixes Python's path to its own folder and zip, so the
  environment's `PYTHONPATH` is ignored.
- EDB's `config_distro.py` sets desktop mode and points pgAdmin's backup and restore tools at
  `runtime/`; `config_local.py` is empty.

## Decision

1. **A service kind, `pgadmin`**, that `kiln service add pgadmin` adds like any other, and
   never by default. Its release is PostgreSQL 18.6's zip itself, same URL and SHA-256 (a
   test keeps them equal), so a PC with that PostgreSQL downloads nothing. Only `python/`
   and `web/` are unpacked (about 340 MB).
2. **Run as a web app on loopback**, not the desktop shell: the embedded Python, under a
   Kiln name (`kiln-pgadmin.exe`, beside `python.exe` so it finds its library), runs a
   launcher Kiln writes before every start. The launcher sets, before pgAdmin starts:
   desktop mode (no pgAdmin login), `127.0.0.1` and the instance's port, every data path
   (settings database, sessions, storage, logs) in the instance's own folder, no update
   check, no master password, and Kiln's PostgreSQL 18.6 `bin` for backup and restore. The
   process also gets `APPDATA` and `LOCALAPPDATA` pointed there, so nothing reaches
   `%APPDATA%\pgAdmin`, another pgAdmin's settings.
3. **Kiln's PostgreSQL instances are listed**: Kiln writes `servers.json` (each instance by
   name and port, as `postgres`, which Kiln's PostgreSQL trusts on loopback) before each
   start, and the launcher loads it with pgAdmin's own `setup.py load-servers --replace` in
   a separate process. An instance added later shows after pgAdmin restarts.
4. **Served as `pgadmin.test`** through the web server, like Mailpit's inbox (ADR 0044).
5. **The app** offers it on PostgreSQL's page, under Tools: "Add pgAdmin…", saying what it
   costs on disk and what it touches, then "Open pgAdmin" once added. It's also in the
   service catalog under "Data tools".

## Consequences

- Desktop mode has no login, like Adminer: anything on this PC can open it on loopback, as
  with Kiln's other tools; nothing beyond the PC can.
- pgAdmin follows PostgreSQL 18's pin: a newer EDB zip brings a newer pgAdmin.
- If a pgAdmin version changes its configuration names, the Scenario's check (the page
  answers, it listens on loopback only, the servers load, `%APPDATA%\pgAdmin` stays
  untouched) catches it.
