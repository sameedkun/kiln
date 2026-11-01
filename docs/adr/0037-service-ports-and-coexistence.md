# 0037. Service ports next to other tools: who holds a port, later conflicts, doctor

- **Status:** accepted
- **Date:** 2026-11-01
- **Task:** P3-03 · Extends [ADR 0019](0019-coexistence.md) · Builds on [ADR 0035](0035-services.md) §5, [ADR 0036](0036-service-framework.md) §7

## Context

[ADR 0035](0035-services.md) §5 gives each service its usual port, or the next free one when
another program holds it. Kiln says who that is, and never stops it. P3-01 implemented the
choice, but only said "taken". The web server already names a port's holder
([ADR 0019](0019-coexistence.md)): it reads the listening sockets, finds each process's
program, and attributes it to Herd, XAMPP, Laragon or WampServer by folder. It recognizes
web servers and PHP by program name.

Services add three cases:

1. **The usual service ports are often taken** by other tools' databases, caches and mail
   catchers, by the system's own MySQL or PostgreSQL service, or by Docker's port
   forwarding.
2. **A port can be taken after Kiln chose it.** Say XAMPP's MySQL starts after a reboot,
   before `kiln services up`. The supervisor would then retry a database that can't bind,
   and the user would see only "not running".
3. **`kiln doctor` should show what else serves databases, caches and mail on this
   machine**, including system services that aren't running at that moment.

## Decision

1. **Recognize service programs by name** (`kiln-coexist`), next to the web servers:
   - MySQL (`mysqld`), MariaDB (`mariadbd`), PostgreSQL (`postgres`);
   - Redis (`redis-server`), Valkey (`valkey-server`), Memurai, Garnet (`GarnetServer`);
   - Mailpit, MailHog;
   - Docker's port forwarders: `com.docker.backend`, `docker-proxy`, `vpnkit` and
     `wslrelay`, the last for WSL.

   With the existing folder rule, a holder reads "XAMPP (mysqld.exe)" or "MySQL
   (C:\Program Files\MySQL\…\mysqld.exe)".
2. **The next free port, explained:** when a role's usual port is busy, `kiln service add`
   says who holds it:
   - "Port 3306 (MySQL) is used by XAMPP (mysqld.exe); using 3307."
   - On Linux a system service often runs as another user, whose program Kiln can't
     inspect. Then it names a running system service of the same kind, if there is one:
     "probably the system's mysql service".
3. **Conflicts at start:** before the supervisor gets its programs, every port of every
   instance that should run must be free, or held by that instance's own Kiln process.
   - Otherwise the command stops before starting anything: "mysql-8.4 can't start: port
     3306 is used by XAMPP (mysqld.exe)".
   - It suggests both ways out: stop that program, or move the instance with
     `kiln service port <name> <role>=<port>`.
   - Kiln never stops the other program.
4. **`kiln service port <name> <role>=<port>…`** changes an instance's ports. The new ones
   must be free. A running instance is restarted on them. Its data stays.
5. **`kiln doctor`:**
   - **A new "Services" area,** one line per Kiln instance. It is ok when running as it
     should, and a warning when it should run but doesn't. A warning also names a port
     another program holds, with the fix.
   - **"Other tools" lists other services:**
     - whoever listens on a usual service port (3306, 5432, 6379, 1025, 8025), as for
       the web ports;
     - the system services of these kinds and whether they run: on Windows from `sc.exe
       query`, on Linux from `systemctl list-units`.

     Both commands are read-only, and only their output is parsed: the service name and
     the state code. Their labels can be translated, but the code and the name can't.

   These are information, not warnings. Another tool's database is no fault.

## Consequences

- A busy port is explained once, where it happens, in the same words as for the web
  server.
- A conflict that appears later stops a start with the holder's name, instead of a
  service that keeps restarting.
- Doctor shows the whole picture of local services before a user wires a project to one.
- Recognition by program name can miss a renamed program. It then shows as its path,
  which is still enough to find it.

## Alternatives considered

- **Taking over the usual port by stopping the other service.** Rejected by ADR 0019:
  Kiln never stops another tool's process.
- **Reading Windows services through the service-control API.** Deferred. It would need
  new unsafe FFI in `kiln-sys` for a report that `sc.exe` already gives.
- **Moving an instance to a free port automatically at start.** Rejected. The port is in
  projects' `.env` files (P3-08). Moving it silently would break them; the user decides.
