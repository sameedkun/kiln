# 0047. Laravel Reverb: a service that runs in its project

- **Status:** accepted
- **Date:** 2026-11-03
- **Task:** P3-16 · Builds on [ADR 0036](0036-service-framework.md), [0037](0037-service-ports-and-coexistence.md), [0039](0039-windows-first-and-services-catalog.md) §2, [0040](0040-postgresql.md) §3, [0045](0045-env-wiring.md), [0046](0046-project-services.md)

## Context

Laravel Reverb is Laravel's WebSocket server for broadcasting. It isn't a program to download:

- it's a Composer package, `laravel/reverb`, that a project installs;
- it runs as `php artisan reverb:start` in that project, with that project's PHP.

Laravel Herd Pro runs it for a project. Reverb listens on 8080 by default.

Every Kiln service so far is a pinned download in Kiln's `bin` folder, and the provider
trait and its contract tests assume that.

Laravel's Reverb settings come in two groups:

- **`REVERB_SERVER_HOST` and `REVERB_SERVER_PORT`:** where the server listens.
- **`REVERB_HOST`, `REVERB_PORT` and `REVERB_SCHEME`:** where the app and browsers
  connect.

`REVERB_APP_ID`, `REVERB_APP_KEY` and `REVERB_APP_SECRET` are the app's own credentials,
which `php artisan install:broadcasting --reverb` writes.

## Decision

1. **A service can run in a project.** Providers gain:
   - **`runs_in_project()`:** such a provider has no releases, and the contract tests
     require exactly that.
   - **`project_launch(project, php, ports)`:** how it starts. The contract tests check
     that it runs the PHP it is given, in the project, and listens on loopback only
     (a `--host=` must be `127.0.0.1`).
2. **Instances keep their project folder.**
   - State schema v8 adds a `project` column to `services`.
   - An instance is named after the project's site (`reverb-shop`), else its folder.
   - Its version is the `laravel/reverb` version in the project's `composer.lock`.
   - Without the package, `kiln service add reverb` refuses and names the command that
     installs it.
3. **It runs under the services supervisor** like every service:
   - `php artisan reverb:start --host=127.0.0.1 --port=<port>`, in the project folder.
   - **The PHP:** the one Kiln picks for that folder at each start, so a site pinned to
     another PHP brings Reverb along.
   - **Without administrator rights:** like every service
     ([ADR 0040](0040-postgresql.md) §3).
   - **Ports** are chosen as for any service ([ADR 0037](0037-service-ports-and-coexistence.md)):
     8080, or the next free one.
   - **Stopping** ends the process. Reverb holds connections, not data.
   - **A project that can't start** (folder gone, no PHP) is skipped with a note. It
     doesn't stop the other services.
   - **Kiln's own PHP counts as Kiln's program** when Kiln checks who holds a service's
     port.
4. **`kiln service add reverb [--project <folder>]`** (default: the current folder). In
   `kiln.toml`, `reverb = "*"` under `[services]` makes `kiln services up` find or add the
   instance for that project. `kiln import herd` maps Herd's `reverb`.
5. **`kiln env use reverb-shop`** wires the instance's own project by default. It sets:
   - `BROADCAST_CONNECTION=reverb`;
   - `REVERB_SERVER_HOST=127.0.0.1` and `REVERB_SERVER_PORT`;
   - `REVERB_HOST=127.0.0.1`, `REVERB_PORT` and `REVERB_SCHEME=http`.

   The app's own `REVERB_APP_*` stay as they are. Browsers treat `127.0.0.1` as a secure
   origin, so a page served over HTTPS may still open `ws://127.0.0.1:<port>`; no TLS
   proxy is needed.

## Consequences

- One command runs a project's Reverb, kept up across crashes like the databases, and one
  more wires the project to it.
- **Projects are the user's.** Kiln never installs `laravel/reverb` and never writes the
  app's Reverb credentials.
- **`kiln uninstall`** stops it with the other services. The project's files are
  untouched.
- **The framework now covers project processes.** A queue worker or Vite could use the same
  path later.
- **CI** installs `laravel/reverb` in the scenario's app and adds the instance. It checks:
  - loopback only;
  - the `.env` keys;
  - a browser-style WebSocket handshake (`101 Switching Protocols`);
  - a broadcast from Laravel, a request Reverb verifies with the app's secret;
  - a restart.

## Alternatives considered

- **Reverb as a special case outside the providers.** Rejected: every service command
  (list, start, stop, logs, ports, doctor, remove) would need its own branch.
- **Writing the project folder into another field of the record.** Rejected for a
  column: the schema says what it holds.
- **A TLS proxy site for Reverb** (`wss://…`), as for Mailpit's inbox. Not needed for
  local development, since browsers allow WebSockets to `127.0.0.1` from HTTPS pages. A
  user can still `kiln proxy` it.
- **Running `install:broadcasting` for the user.** Rejected: it changes the project's
  code, configuration and front-end dependencies, which is the user's decision.
