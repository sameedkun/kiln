# 0035. Services: instances, providers, ports, data and `.env` wiring

- **Status:** accepted
- **Date:** 2026-10-31
- **Task:** Phase 3 (P3-01 to P3-12) · Builds on [ADR 0003](0003-license-and-dependency-policy.md), [0004](0004-reversible-operation-engine.md), [0013](0013-downloads-and-php-runtimes.md), [0014](0014-php-cgi-supervisor.md), [0019](0019-coexistence.md), [0025](0025-own-static-linux-php-builds.md), [0026](0026-kilnd-daemon-and-local-api.md), [0027](0027-project-config-kiln-toml.md), [0032](0032-proxy-sites.md)

## Context

Phase 3 adds the services a PHP project needs next to its site: MySQL, MariaDB, PostgreSQL, a
Redis-compatible cache, and Mailpit to catch outgoing mail. It covers Windows and Linux;
macOS moved to its own phase ([ADR 0007](0007-platform-delivery-order.md), amended).

Five things make services harder than PHP:

1. **They hold the user's data.** A database is the user's work, unlike a PHP build that
   can be downloaded again. "Uninstall leaves the machine as it was" can't simply mean
   deleting it.
2. **Other tools run them too.** Laravel Herd Pro, XAMPP, Laragon, Docker, and the
   distribution's or Windows' own MySQL and PostgreSQL services often hold 3306, 5432 and
   6379 already ([ADR 0019](0019-coexistence.md)).
3. **Their licenses differ.** MySQL and MariaDB are GPL. Under
   [ADR 0003](0003-license-and-dependency-policy.md) they are downloaded from the vendor
   when the user asks, and never shipped by Kiln.
4. **Official binaries don't cover every platform**, as checked on 2026-10-31:

   | Service | Windows | Linux |
   |---|---|---|
   | MySQL | vendor zip | vendor glibc tarball |
   | MariaDB | vendor zip | vendor tarball |
   | PostgreSQL | EDB archive | **none since version 10** |
   | Valkey (BSD) | **none** (WSL only) | vendor builds for Ubuntu 22.04 and 24.04 |
   | Mailpit (MIT) | GitHub release | GitHub release |
   | Garnet (MIT, Redis-compatible) | GitHub release | GitHub release |

5. **Projects must be wired to them.** That means the `DB_*`, `REDIS_*` and `MAIL_*` keys
   in the project's `.env`, which is project data too.

## Decision

1. **Instances.** `kiln service add <kind> [--version X] [--name N] [--port P]` creates an
   instance:
   - a name (default `<kind>-<major>`, such as `mysql-8.4`), its kind, its exact version and
     its port;
   - a data folder, `<local data>/services/<name>/data`;
   - whether it starts with the others.

   Instances are stored in the state database. Two versions of one kind can run side by
   side on different ports, for example MySQL 8.0 and 8.4.

   The other commands: `kiln service list | start | stop | restart | logs | remove`.
   `kiln services up` and `down` act on all instances.
2. **Providers.** Each kind is a `ServiceProvider` in a new crate, `kiln-services`. A
   provider says:
   - where its binaries come from and how they are verified;
   - how to create a data folder (`mysqld --initialize-insecure`, `mariadb-install-db`,
     `initdb`);
   - the command line that runs it in the foreground, with a config file Kiln writes;
   - how to tell it is ready (a protocol-level ping, not only an open port);
   - how to stop it gracefully;
   - its `.env` keys and its client (`kiln db` opens `mysql` or `psql` on the instance).

   Providers are compiled in. "Plug-in" means a trait with contract tests that every
   provider passes, not dynamic loading.
3. **Running.** Services run as the user, never as system services, under a second
   `kiln-supervisor` instance (`<local data>/services/supervisor`).
   - The same binary as for PHP ([ADR 0014](0014-php-cgi-supervisor.md)), generalized from
     php-cgi pools to programs. It gives services the same crash back-off and the same
     kill-on-close job, so nothing is left running.
   - It is separate from the PHP pools, so `kiln web stop` leaves the databases running,
     and the reverse.
   - The CLI works without `kilnd`. With `kilnd` running, its watchdog restarts the
     services supervisor after a crash or a reboot, as it does for the web server
     ([ADR 0026](0026-kilnd-daemon-and-local-api.md)). Its API gains `service.*` methods.
4. **Loopback only** (plan principle 3).
   - Every service listens on `127.0.0.1`, and on `::1` where it can, set in the config file
     Kiln writes. There is no option to listen more widely in Phase 3.
   - Mailpit's web UI is also served as a Kiln HTTPS site (`mail.test`) through a proxy
     ([ADR 0032](0032-proxy-sites.md)).
5. **Ports.**
   - Each kind has its usual port: 3306, 5432, 6379, and 1025 and 8025 for Mailpit.
   - If another program holds it, Kiln takes the next free port in that kind's range, says
     who holds the usual one, and keeps the choice for that instance. Kiln never stops the
     other program ([ADR 0019](0019-coexistence.md)).
   - `kiln doctor` names the other services it finds: Herd's, XAMPP's, Laragon's, Docker's,
     and the Windows and systemd MySQL and PostgreSQL services.
6. **Development credentials.**
   - MySQL and MariaDB: `root` with an empty password.
   - PostgreSQL: `postgres`, with `trust` authentication for loopback connections only.
   - The cache: no password.
   - These are what Laravel's own defaults and Herd use. They are safe only because nothing
     listens beyond loopback, and `kiln service add` says so.
   - `--password` sets one instead. Kiln then keeps it in the OS secret store, as it does
     the CA key, and `.env` wiring writes it.
7. **Data is never deleted by default.**
   - `kiln service remove` stops the instance and deletes its binaries, but keeps the data
     folder unless `--delete-data` is given. It prints where the data is and how large.
   - `kiln uninstall` refuses while service data exists, unless the user picks one:
     - `--delete-data` deletes it, and the machine is then as it was;
     - `--keep-data` moves each data folder to `~/Kiln data (<date>)`, the one thing
       uninstall leaves. It is named in the summary.
   - `kiln db dump <instance> [database]` and `kiln db restore` use the service's own tools
     (`mysqldump`, `pg_dump`), so data can leave Kiln in a portable form first.
8. **Binary sources and verification:**
   - **Pinned digests:** every download is checked against a SHA-256 pinned in Kiln
     ([ADR 0013](0013-downloads-and-php-runtimes.md)), and also against the vendor's
     signature where it publishes one.
   - **MySQL, MariaDB:** downloaded from the vendor at the user's request, never
     redistributed ([ADR 0003](0003-license-and-dependency-policy.md)). Kiln shows the
     license when it installs one.
   - **PostgreSQL:** EDB's archives on Windows. On Linux, Kiln's own relocatable builds, from
     the same build pipeline and release process as its Linux PHP
     ([ADR 0025](0025-own-static-linux-php-builds.md)). The PostgreSQL license allows that.
   - **Cache:** Valkey from the vendor on Linux. Windows has no native Valkey, so a spike
     (P3-02) checks Garnet (MIT, Microsoft) against what Laravel uses:
     - cache with locks and tags;
     - sessions;
     - queues, which run Lua scripts;
     - rate limiting.

     If Garnet passes, it is Windows' cache under the kind `cache`, with `valkey` and
     `garnet` as its implementations. If it doesn't, Windows has no cache service in
     Phase 3, and Kiln says so rather than half-working.
   - **Mailpit:** its GitHub releases on both platforms.
   - **Linux system libraries:** binaries that need a library the system lacks are a
     known risk. For example, MySQL's generic tarball needs `libaio.so.1`, which Ubuntu
     24.04 renamed. The provider finds such cases. Kiln points to the library through its
     own folder, such as a symlink in the instance's `lib` folder, and never installs
     system packages.
9. **`.env` wiring** is a journaled operation ([ADR 0004](0004-reversible-operation-engine.md)).
   - `kiln env use <instance> [--project <folder>]` sets only the keys that instance needs:
     - databases: `DB_CONNECTION`, `DB_HOST`, `DB_PORT`, `DB_DATABASE`, `DB_USERNAME`,
       `DB_PASSWORD`;
     - the cache: `REDIS_HOST`, `REDIS_PORT`, `REDIS_PASSWORD`;
     - Mailpit: `MAIL_MAILER`, `MAIL_HOST`, `MAIL_PORT`.
   - Every other byte of `.env` stays as it was: comments, order, quoting, line endings.
   - **Preview first:** Kiln shows the keys and their old and new values before changing
     anything.
   - The journal keeps each key's earlier value, or that it wasn't there. `kiln undo`
     restores the file byte for byte, unless it was edited since; then Kiln stops and says
     so, as for the hosts file.
   - It creates the project's database (named after the site) if missing. A database is
     data, so undo doesn't drop it.
   - **Choosing what Laravel uses:** `CACHE_STORE`, `SESSION_DRIVER` and `QUEUE_CONNECTION`
     are left alone unless `--cache`, `--session` or `--queue` is given.
10. **Project config.**
    - `kiln.toml` gains a `[services]` table, for example `mysql = "8.4"`, `cache = "*"`,
      `mailpit = "*"`.
    - `kiln services up` in a project creates and starts what it lists, and offers the
      `.env` wiring.
    - `kiln import herd` maps `herd.yml`'s `services` into it, finishing what
      [ADR 0027](0027-project-config-kiln-toml.md) left for Phase 3.
    - `kiln new --db mysql|pgsql` wires a new project at creation.

## Consequences

- One model covers all five kinds: install, data folder, run, ready, stop, wire. The Phase 4
  GUI gets them through `kilnd`'s API.
- Databases outlive Kiln unless the user says otherwise. The uninstall comparison of
  [ADR 0024](0024-integration-test-vms.md) runs with `--delete-data`, and a second check
  covers `--keep-data`.
- Linux PostgreSQL adds a second product to the Linux build pipeline, with its own release
  and pinning.
- Windows' cache depends on the Garnet spike. A Windows user may have no cache service in
  Phase 3.
- Empty passwords on loopback match common practice. Anyone with a local account can reach
  the databases, as with Herd or XAMPP. The `--password` option is there for those who
  share a machine.
- Kiln never touches another program's service, data or port. Herd Pro's services, XAMPP's
  MySQL and system services go on as they were.

## Alternatives considered

- **Running services as system services** (Windows services, systemd system units).
  Rejected: needs admin rights for every start, and conflicts with the system's own
  MySQL and PostgreSQL. Per-user processes match how Kiln runs PHP and Caddy.
- **Docker containers.** Rejected as the default: a heavy dependency, slow on Windows, and
  a second network stack. Kiln may detect a Docker database and wire `.env` to it later.
- **Distribution packages on Linux.** Rejected: system-wide, need root, one version at a
  time, and they collide with an existing install. Kiln installs nothing system-wide.
- **Redistributing MySQL or MariaDB in Kiln's own release.** Rejected by
  [ADR 0003](0003-license-and-dependency-policy.md).
- **Deleting service data at uninstall like other Kiln data.** Rejected. Losing a database
  without warning is the worst outcome Kiln could cause.
- **A third-party redistributor's PostgreSQL builds for Linux.** Rejected, for example
  zonky's, published for embedded testing. Kiln would depend on someone else's build
  choices and release timing. Building PostgreSQL alongside Kiln's PHP keeps one
  pipeline, one pinning process and one place to fix a library problem.
