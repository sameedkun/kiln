# 0053. API version 2: the CLI's `--json` mode, read methods, and changes as jobs

- **Status:** accepted
- **Date:** 2026-11-12
- **Task:** P4-01 · Implements [ADR 0050](0050-desktop-app.md) §3 · Amends [ADR 0026](0026-kilnd-daemon-and-local-api.md) §2 · Builds on [0048](0048-kilnd-and-services.md), [0052](0052-phase-4-screens-and-scope.md)

## Context

The desktop app talks only to `kilnd` ([ADR 0050](0050-desktop-app.md)). The screens drawn
in P4-15 need much more than API version 1 gives (status, the web server, the CA and
starting or stopping services). They need:

- **Reads:** sites, PHP and Node versions and what can be installed, extensions, services
  and the catalog, history, and the doctor's report.
- **Changes:** everything the CLI can change, with the same preview first and progress
  for downloads.

Four facts shape the design:

1. **The CLI holds the logic.** How `kiln sites` combines parked folders, links, proxies,
   drivers and PHP versions lives in `kiln-cli`, a binary the daemon can't call into. It
   prints text through about 360 `println!` calls.
2. **One function each:**
   - every confirmation goes through `apply::confirm`;
   - every preview through `apply::print_preview`;
   - download progress through `php::progress_printer`.

   A declined confirmation returns success, so the command exits 0.
3. **Most changes never ask.**
   - **Eleven places confirm.** Seven ask before changing anything:
     - `hosts set|clear|sync`;
     - `ca trust`;
     - `shims install`;
     - `daemon enable|disable`;
     - `undo`;
     - `env use`.
   - **The others ask partway through:**
     - `services up` adds and starts services, then asks before wiring the `.env`;
     - `uninstall` asks after its plan.
   - **The rest apply at once:** `link`, `php install`, `service add`, and so on.
4. **The daemon answers a connection's requests one at a time.** A read that fetches from
   the network (`php available`) or scans ports (`doctor`) would hold up every other call
   from the app.

## Decision

1. **`kiln --json`: a machine mode for every command.** A global flag. Standard output
   becomes JSON Lines, one object per line, each with a `type`:
   - `note`: a line the command would have printed (`text`).
   - `warning`: a line it would have printed to standard error (`text`).
   - `progress`: a download's progress (`label`, `done`, `total` in bytes, `total` may be
     null). At most about one per 2%.
   - `preview`: what a change will do, before it is made (`summary` and `changes`: each
     with `target`, `before` and `after`). It comes from the operations' own previews, the
     same ones the CLI prints.
   - `confirm`: the question the CLI would have asked (`question`).
   - Last, exactly one of:
     - `result`: `ok` true, with `data` for commands that read (§3) or report something;
     - `error`: `message`, the words the CLI prints after `error:`.

   **The final line decides the outcome, not the exit code.** `kiln doctor`, for example,
   exits 1 when a check fails but reads successfully.

   Exit codes stay: 0 success, 1 error, 2 usage. One is new: **3**, which means stopped at a
   confirmation. Its `result` has `ok` false and `needs_confirmation` true.

   **How it works inside:**
   - **One output module:** behind the flag, every `println!` and `eprintln!` in `kiln-cli`
     goes through `say!` and `say_err!`, which write text or a `note`/`warning`. No command
     prints raw text in JSON mode, and no command needs a code path of its own.
   - **Child processes:** programs the CLI runs that print, such as Composer and
     `php artisan` for `kiln new`, have their output read line by line and forwarded as
     `note`s.
   - **Interactive commands** (the database clients `kiln db` opens) refuse to run in JSON
     mode.
2. **Confirmations in JSON mode never read the terminal.**
   - `confirm()` sends a `confirm` event.
   - It answers yes only when the global `--confirmed` flag was given.
   - Otherwise it records a decline, and the command ends with `needs_confirmation`
     (exit 3) instead of success.
   - `--confirmed` means nothing without `--json`, where the commands' own `--yes` and the
     prompt still apply.
   - Text mode is unchanged: a declined prompt still says "Nothing was changed." and
     exits 0.
3. **Read commands give data in JSON mode** (`result.data`), built from the same values
   their text is built from:
   - `kiln sites`: parked folders, then each site with its URL, host, driver, document
     root, how it was found (parked, link or alias), PHP version and where it came from,
     HTTPS, and whether its name is in the hosts file. Proxies come with their targets.
   - `kiln php list`, `php available`, `php ext list [--php X]`.
   - `kiln node list`, `node available`.
   - `kiln service list`: each instance with kind, version, ports, enabled, running, and
     its project.
   - `kiln service catalog` (new): every kind Kiln offers, with its versions, licence and
     ports.
   - `kiln history [--all]`: each operation with id, time, state and description.
   - `kiln doctor`: its existing JSON report.
4. **API version 2.** `kiln.hello` reports `api: 2`. Version 1's methods stay as they were.
   - **Each request is answered on its own thread.** Responses carry their ids, so a slow
     read no longer holds the connection.
   - **Read methods** run the matching command with `--json` and return its `data`, with a
     2-minute limit:
     - `sites.list`, `php.list`, `php.available`, `php.extensions` (`php`);
     - `node.list`, `node.available`;
     - `services.list`, `services.catalog`;
     - `history.list` (`all`), `doctor.report`.

     Parameters are checked before they reach a command line. The two `available`
     methods ask the network, so the app calls them only when the user asks.
   - **Changes are jobs.** `jobs.start` takes `args`, the CLI's arguments as a list, and a
     `mode`. It answers at once with a job id, then runs `kiln` on its own thread, as
     follows:
     - **`preview`** runs `kiln --json <args>`, which stops at the confirmation with the
       preview. It is allowed only for the commands that ask before changing anything
       (fact 3). For any other, a preview could make the change, so it is refused.
     - **`apply`** runs `kiln --json --confirmed <args>`.
     - **Not through a shell:** the arguments are handed to `kiln` as they are.
       - **Refused anywhere in them:** `--json`, `--confirmed`, `--yes` and `-y`.
       - **The first two must be command words, not options.**
     - **An allow-list of commands:**
       - `link`, `unlink`, `park`, `unpark`, `secure`, `unsecure`, `isolate`,
         `unisolate`, `proxy`, `unproxy`, `new`, `import herd`;
       - `php install|remove|default|update|rollback|workers|ext on|ext off`;
       - `node install|remove|default`, `composer install`;
       - `service add|start|stop|restart|port|remove`, `services up|down`;
       - `env use`, `tool add|remove`, `db dump|restore`, `undo`;
       - `web install|start|stop|restart`, `ca init|trust`, `hosts sync`,
         `shims install`, `daemon enable|disable`.

       Anything else is refused: `uninstall`, which stops the daemon itself, and every
       read and every interactive command.
     - **Removing data stays allowed** (`service remove --delete-data`, `undo --force`): the
       app asks the user first. The MCP server (P4-19) will get a narrower list of its own.
     - **Events:** every line the job prints is sent to subscribers as `job.event`
       (`job`, `event`). The last one is `job.finished`, with:
       - `job`, `ok`, `exit`;
       - `needs_confirmation`;
       - the final `result` or `error`.
     - **A run that ends without a final line** (a crash, a usage error) finishes with an
       `error` built from what it wrote to standard error and its exit code.
     - **After an `apply` job,** the daemon sends `state.changed`, so open views refresh.
     - `jobs.get` returns a job's events so far. `jobs.list` returns the newest 20 jobs.
       Each keeps at most 1,000 events.
   - **One change at a time still holds:** jobs run the CLI, which takes the change lock
     ([ADR 0026](0026-kilnd-daemon-and-local-api.md) §1). A second job waits for the first,
     and says so in a `note`.
5. **Elevation (amends [ADR 0026](0026-kilnd-daemon-and-local-api.md) §2).** The daemon
   still never elevates on its own. Asked by a client, it may run the commands that need
   approval (`hosts sync`, `ca trust`, `shims install`). Their UAC and certificate prompts
   then come from a process without a window. Whether Windows shows them in front, from the
   tray app's session, is checked by hand on a developer machine before P4-06 relies on it.
6. **What the screens need that version 2 doesn't give yet**, against the screens' sample
   data ([ADR 0052](0052-phase-4-screens-and-scope.md)):

   | Field on a screen | Where it comes from |
   |---|---|
   | Site: URL, PHP version, how it was found, HTTPS | `sites.list` (this task) |
   | Site: framework name | `sites.list`'s driver (this task); its version later (P4-04) |
   | Site: Node version, which services it uses | P4-04 (from `.nvmrc` and the project's `.env` wiring) |
   | Site: workers | P4-09 |
   | Site: requests | P4-18 |
   | Service: kind, version, ports, running | `services.list` (this task) |
   | Service: which sites use it | P4-05 (from `.env` wiring records) |
   | PHP: versions, default; extensions | `php.list`, `php.extensions` (this task) |
   | PHP: which sites use each, updates offered | `sites.list` (per site, this task); updates P4-17 |
   | History, doctor | `history.list`, `doctor.report` (this task) |
   | Logs, footprint, settings | P4-08, P4-16, P4-07 |

## Consequences

- **One implementation:** the app and the CLI can't disagree, because every read and every
  change runs the CLI's own code. Scripts get the same JSON (`kiln --json sites`).
- **Cost:** each read starts a short-lived `kiln` process, tens of milliseconds on Windows.
  That is fine for views that refresh on events. A cache in the daemon can come later if it
  is ever needed.
- **Unstructured parts:** text from commands that don't build data yet arrives as `note`s.
  The app shows them as the CLI would, and commands gain structured `data` as the pages
  need it.
- **Exposure:** a job is exactly what the user could run in a terminal, through a socket
  only they can open ([ADR 0026](0026-kilnd-daemon-and-local-api.md) §3). The allow-list
  keeps the app to the commands it is meant to run.
- **Not covered yet:**
  - Changes made in a terminal send no `state.changed`, so the app also refreshes when its
    window is shown.
  - A running job can't be cancelled.

## Alternatives considered

- **Move the read logic into library crates and answer in the daemon's process.** It is
  faster, but it is a large move of code that works, and it creates a second path beside
  the CLI's text output. It can still come later, one read at a time, behind the same
  method names.
- **A `--json` code path per command.** Hundreds of separate changes, each one a place to
  drift. Routing all output through one module covers every command at once.
- **Confirming through an environment variable.** It is invisible in the job's command
  line and in logs. A flag shows in both.
- **Blocking change methods with progress as notifications.** One slow download would hold
  the connection, and a client that reconnects would lose the outcome. Jobs keep their
  events.
