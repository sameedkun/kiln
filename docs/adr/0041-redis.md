# 0041. Redis on Windows: upstream Redis built in the open by redis-windows

- **Status:** accepted
- **Date:** 2026-11-02
- **Task:** P3-06 · Builds on [ADR 0035](0035-services.md), [0036](0036-service-framework.md), [0003](0003-license-and-dependency-policy.md), [0039](0039-windows-first-and-services-catalog.md) §2, [0040](0040-postgresql.md) §3

## Context

Redis has no official Windows build. [ADR 0039](0039-windows-first-and-services-catalog.md)
left P3-06 to choose between `zkteco-home/redis-windows` (which DBngin uses) and a
Kiln-built Valkey. It asked first where zkteco's binaries come from. Garnet stays blocked
([I-75](../issues.md#i-75)).

| | `zkteco-home/redis-windows` | `redis-windows/redis-windows` |
|---|---|---|
| What it ships | `redis-server.exe` and friends, committed into the repository; releases carry no files | zip files on each GitHub release, with GitHub's own SHA-256 for each |
| How they're built | not shown: no source for the Windows port, no build workflow. Its README says Visual Studio, which needs changes to Redis that aren't published | a public GitHub Actions workflow (Apache-2.0) that downloads the upstream tag, builds it with MSYS2 or Cygwin, and runs a `SET`/`GET` smoke test. The only edits are a banner line and dropping module tests from `make all` |
| License stated | MIT, for Redis 8 binaries | Redis's own |
| Latest | 8.10.3 | 8.10.3, 8.8.3, and older lines |

Redis 8 is licensed RSALv2, SSPLv1 or AGPLv3, at the user's choice. Under AGPLv3, binaries
of a modified Redis come with that modified source. zkteco's port publishes none, and calls
the binaries MIT. A pinned digest would only prove Kiln gets the same opaque file each time.

The MSYS2 builds are traceable: each one comes from an upstream tag, with a public build log
on GitHub. They need the MSYS2 runtime (`msys-2.0.dll`, LGPL), which the zip carries. They
are built with `-O0`. That's slower than an optimised build, but irrelevant for local
development.

## Decision

1. **Kind `redis`,** with port role `redis` (usual port 6379):
   - From `redis-windows/redis-windows`' MSYS2 zips, pinned by SHA-256. Kiln's digests
     match the ones GitHub records for the assets.
   - Two lines: 8.10.3 and 8.8.3. Instances are named by minor version (`redis-8.10`),
     as Redis lines are minor versions.
   - Windows only. Linux waits for containers
     ([ADR 0039](0039-windows-first-and-services-catalog.md)).
   - Under [ADR 0003](0003-license-and-dependency-policy.md), the binaries are downloaded
     from that publisher at the user's request and never redistributed by Kiln. The license
     line says so.
2. **A whole-tree install,** keeping the server, `redis-cli`, the RDB and AOF checkers and
   the MSYS2 DLLs. Left out: Sentinel, the benchmark, the sample configs, `start.bat` and
   the readmes. The server runs as `kiln-redis.exe`, a hard link; Redis picks its mode
   from the program name only for `redis-sentinel` and `redis-check-*`.
3. **The config** is written by Kiln to `<instance>/kiln.cnf` before every start:
   - `bind 127.0.0.1`, the port, and `protected-mode yes`.
   - Snapshots in the data folder (`dump.rdb`) at Redis's default save points and at
     shutdown, so queued jobs and sessions survive a restart. No append-only file.
   - The log on standard output, which is the instance's log. The supervisor opens that
     file for writing at its end rather than append-only: the MSYS2 runtime can't write
     through an append-only handle ([I-79](../issues.md#i-79)).
   - Every path is relative: the server runs in the instance folder and is given
     `kiln.cnf`, with `dir data` and `pidfile server.pid` (opened once Redis has moved
     into `data`). The MSYS2 build reads `C:/...` as a relative path and puts its current
     folder in front, so absolute Windows paths fail ("can't open config file
     '/cygdrive/.../C:/...'", [I-80](../issues.md#i-80)).
4. **Stop:** `redis-cli -h 127.0.0.1 -p <port> shutdown`, which saves, then exits. An
   instance counts as stopped once its pid file (`data/server.pid`) is gone; Redis removes
   it last.
5. **Client:** `kiln db redis-8.10 [n]` runs `redis-cli` on database `n` (`-n`).
6. **Like every service, Redis runs without administrator rights**
   ([ADR 0040](0040-postgresql.md) §3).
7. **Valkey stays possible.** A Kiln-built Valkey (BSD) would need a public repository
   to publish from, as `kiln-php` does ([ADR 0025](0025-own-static-linux-php-builds.md)).
   That is a separate decision.

## Consequences

- Windows gets real Redis 8, which behaves as Laravel expects. The CI scenario checks
  databases 0 and 1, a Lua script in database 1 (what broke Garnet), data across a
  restart, and a clean shutdown.
- Kiln depends on one community project's releases. Its builds can be rebuilt from its
  public workflow if it stops; Valkey (point 7) is the fallback.
- The binaries are unoptimised (`-O0`): fine for local development, not for benchmarks.

## Alternatives considered

- **`zkteco-home/redis-windows`.** Rejected: binaries with no source and no build
  workflow, under a license that doesn't match Redis 8's.
- **A Kiln-built Valkey now.** Deferred (point 7): it needs a new public repository and
  a build workflow to maintain. Nothing in local development needs it while the open
  Redis builds exist.
- **Memurai.** Rejected: proprietary, and its free edition's license limits use.
- **Garnet.** Blocked by [I-75](../issues.md#i-75).
- **The Cygwin builds** from the same releases. Equivalent. The MSYS2 zip is smaller
  (14 MB, against 15 MB for Cygwin).
