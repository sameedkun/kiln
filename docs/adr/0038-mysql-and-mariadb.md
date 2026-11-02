# 0038. MySQL and MariaDB: vendor archives, a Kiln-written config, first start, libaio

- **Status:** accepted
- **Date:** 2026-11-01
- **Task:** P3-04 · Builds on [ADR 0035](0035-services.md), [0036](0036-service-framework.md), [0037](0037-service-ports-and-coexistence.md), [0003](0003-license-and-dependency-policy.md), [0013](0013-downloads-and-php-runtimes.md)

## Context

Phase 3's first databases. Both are GPL, so under
[ADR 0003](0003-license-and-dependency-policy.md) they are downloaded from their vendors when
the user asks, never shipped by Kiln. Before deciding, each was run by hand in both test VMs:
data folder set-up, renamed server, loopback config, client, shutdown.

| | MySQL 8.4.13 | MariaDB 11.8.9 |
|---|---|---|
| Windows archive | `mysql-8.4.13-winx64.zip`, 281 MB; 1,254 MB unpacked, of which 693 MB `.pdb` debug symbols | `mariadb-11.8.9-winx64.zip`, 92 MB; 295 MB unpacked, 73 MB `.pdb` |
| Linux archive | `…-linux-glibc2.28-x86_64-minimal.tar.xz`, 80 MB; 405 MB unpacked | `…-linux-systemd-x86_64.tar.gz`, 448 MB; 1.4 GB unpacked (273 MB test suite, 469 MB plugins with debug info) |
| Published checks | MD5 on the download page; a GPG signature from MySQL Release Engineering | SHA-256 in the downloads API; GPG signatures |
| Data folder | `mysqld --initialize-insecure`: 4.5 s (Linux), 6.5 s (Windows). Creates only `root@localhost` | `scripts/mariadb-install-db` (Linux, 3 s) or `bin\mariadb-install-db.exe` (Windows, 2.6 s). Also creates `root@127.0.0.1` and `root@::1`, no passwords |
| Linux libraries | `libaio.so.1` and `libnuma.so.1`. Ubuntu 24.04 renamed libaio to `libaio.so.1t64`, and its desktop install has neither name | `libsystemd`, `liblz4`, `libzstd`: present on a standard install |
| Windows quirks | started directly, `mysqld` relaunches itself under a "monitor" parent (two processes) | the installer needs the data folder's parent to exist |
| Renamed (`kiln-mysqld`, `kiln-mariadbd`) | runs | runs |
| Clean shutdown via `mysqladmin` / `mariadb-admin` | 1.3–1.5 s | 0.1–0.2 s |

Laravel connects over TCP to `127.0.0.1`. With `skip-name-resolve`, MySQL's `root@localhost`
refuses that ("Host '127.0.0.1' is not allowed to connect").

## Decision

1. **Kinds `mysql` and `mariadb`,** each with its pinned long-term-support lines:
   - **MySQL** 8.4.13 and 9.7.4: the Windows x64 zip and the Linux x86-64 *minimal* tarball
     (`.tar.xz`).
   - **MariaDB** 11.8.9 and 12.3.3: the Windows x64 zip and the Linux x86-64 `systemd`
     tarball.
   - **Pins:** each archive by SHA-256.
     - **MariaDB:** from its downloads API.
     - **MySQL:** computed after checking Oracle's GPG signature against MySQL Release
       Engineering's key, and Oracle's MD5.
   - **Downloads:** from the vendor: `cdn.mysql.com` and `archive.mariadb.org`. Oracle
     takes a superseded release off its CDN, so an older Kiln can't download MySQL after
     that, and says to update Kiln. MariaDB's archive keeps every release.
   - **Default instance names:** the release series, e.g. `mysql-8.4` and
     `mariadb-11.8`.
   - **One port role,** `mysql`, usually 3306, for both. A second database takes the next
     free port ([ADR 0037](0037-service-ports-and-coexistence.md)).
   - x86-64 only for now.
2. **Whole archives, without the extras:**
   - The archive's top folder is unpacked into the version's `bin` folder, skipping:
     - debug symbols (`*.pdb`);
     - static libraries (`*.lib`, `*.a`);
     - test suites, docs, man pages and headers;
     - MariaDB's `lib/plugin`: neither Laravel nor the root account needs one, and on Linux
       they carry 469 MB of debug information. MySQL keeps its plugins, since it loads
       `component_reference_cache` from there at every start. The first Windows run without
       them logged an error each time.
   - The server program gets its Kiln name as a hard link next to the original:
     `kiln-mysqld(.exe)`, `kiln-mariadbd(.exe)`. The vendors' set-up tools still find the
     original, and no disk space is used twice.
   - `.tar.xz` is read with `lzma-rust2` (Apache-2.0, pure Rust).
3. **A config Kiln writes,** `<instance>/kiln.cnf`, passed as `--defaults-file`:
   - `bind-address=127.0.0.1`, the instance's port, `skip-name-resolve`;
   - for MySQL, `mysqlx=OFF`, so its X plugin doesn't open 33060;
   - the data folder, a pid file, the error log in the instance folder, and on Linux the
     socket there too.
   - On Windows, MySQL also gets `--no-monitor`, so the supervisor watches the server
     itself.
4. **First start creates the accounts Laravel uses:**
   - **MySQL:** `--initialize-insecure` with an init file that creates `root@127.0.0.1` and
     `root@::1` without a password, with all privileges.
   - **MariaDB:** its installers already do. On Linux they run with
     `--auth-root-authentication-method=normal --skip-test-db`.
   - These are ADR 0035 §6's development credentials, safe only because nothing listens
     beyond loopback.
5. **Stop:** the stop command is the vendor's admin tool,
   `mysqladmin|mariadb-admin --host=127.0.0.1 --port=<port> --user=root shutdown`, then
   the supervisor's waits ([ADR 0036](0036-service-framework.md) §1).
6. **Missing system libraries** follow [ADR 0013](0013-downloads-and-php-runtimes.md)'s rule
   for PHP's VC++ runtime: Kiln detects, explains, and doesn't install system components.
   - **After installing,** Kiln runs the server once with `--version`.
   - **Windows:** a missing DLL means Microsoft's VC++ runtime. Kiln says so and links the
     official installer.
   - **Linux:** when `libaio.so.1` is missing but `libaio.so.1t64` exists (Ubuntu 24.04
     and later), Kiln links the first to the second in the version's own `kiln-lib`
     folder and runs MySQL with `LD_LIBRARY_PATH` set to it. No system change.
   - **Linux, any other missing library:** e.g. no libaio at all. Kiln names it and the
     package that provides it (`sudo apt install libaio1t64`), and stops.
7. **`kiln db <instance> [database]`** opens the vendor's client (`mysql` or `mariadb`) on
   the instance as root.

## Consequences

- MySQL and MariaDB run side by side in several versions. Each is set up the way its vendor
  documents, and confined to loopback by Kiln's config.
- **Disk:**
  - MySQL is about 560 MB per version on Windows and 405 MB on Linux.
  - MariaDB is about 220 MB on Windows and about 650 MB on Linux, where `mariadbd` carries
    its debug information.
  - That is the vendors' build; Kiln doesn't strip binaries.
- A Linux user without libaio installs one package for MySQL. MariaDB needs none.
- The Ubuntu test VM has no libaio. Its scenario checks MySQL's refusal message; running
  MySQL there needs libaio installed in the VM first, which is a change to the test VM.
- A pinned MySQL release can't be downloaded once Oracle supersedes it, until Kiln's pin
  moves on.

## Alternatives considered

- **MySQL's full Linux tarball (834 MB).** Rejected: the minimal one has the server and
  the clients.
- **Bundling or downloading libaio ourselves.** Rejected for now. It's a system library;
  Kiln would take over its updates. Ubuntu's renamed copy covers the usual case, and the
  message covers the rest.
- **Renaming the server instead of linking it.** Rejected: MariaDB's set-up script calls
  `bin/mariadbd` by name.
- **`skip-name-resolve` off, so that `127.0.0.1` maps to `localhost`.** Rejected: it would
  make every connection do a reverse DNS lookup, and depend on the hosts file.
