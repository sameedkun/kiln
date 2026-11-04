# 0039. Windows first: Linux on hold, a Herd-like services catalog, tests on CI

- **Status:** accepted; amended 2026-11-04: the test VMs are deleted, and the by-hand Herd check runs on a developer machine (see the end)
- **Date:** 2026-11-02
- **Task:** Phase 3 re-plan · Amends [ADR 0007](0007-platform-delivery-order.md), [ADR 0035](0035-services.md) · Changes how [ADR 0024](0024-integration-test-vms.md)'s scenarios run

## Context

After P3-04, four facts changed the plan:

1. **Linux services aren't portable as native binaries.** PostgreSQL has no Linux binaries
   since version 10, MySQL needs libaio (missing on a standard Ubuntu 24.04 desktop), and
   Valkey ships builds for Ubuntu only. Kiln would end up supporting one distribution, which
   isn't what "Linux" means to its users. Kiln's Linux core already runs anywhere: PHP from
   its own static builds ([ADR 0025](0025-own-static-linux-php-builds.md)), Caddy as one
   static binary. Only the services don't.
2. **Lerd** ([lerd.sh](https://lerd.sh/)) shows the way on Linux. It is a local PHP
   environment that runs its services as rootless Podman containers: no Docker daemon, no
   sudo, nothing installed system-wide, and the same images on every distribution.
3. **The test VMs cost too much.**
   - Each run takes 45 minutes or more.
   - The two VMs can't run together on a 16 GB laptop.
   - Runs have been lost to host sleep, restarts and slow mirrors.
4. **The service set Windows users expect** is Laravel Herd Pro's: MySQL, MariaDB,
   PostgreSQL, MongoDB, Redis, Laravel Reverb, Meilisearch, MinIO and RustFS, plus mail.
   Most of these publish official Windows builds.

## Decision

1. **Windows first.** Phase 3 delivers services on Windows. Linux is **on hold**:
   - its code stays, and keeps compiling and passing its unit tests in CI;
   - no new Linux-only work, and no Linux VM runs;
   - when Linux resumes, its services run as **rootless Podman containers**, Lerd's approach,
     instead of native binaries. That is decided in its own ADR then. Kiln's Linux PHP and
     Caddy stay native.
2. **The services catalog on Windows,** in this order:

   | Service | Source | State |
   |---|---|---|
   | Mailpit | GitHub release | done (P3-01) |
   | MySQL, MariaDB | vendor archives | done (P3-04) |
   | PostgreSQL | EDB's Windows binaries | P3-05 |
   | Redis | a Windows build of Redis, chosen in P3-06 | P3-06 |
   | Meilisearch, RustFS, MongoDB | official Windows releases | P3-14 |
   | phpMyAdmin, Adminer | PHP apps, served as Kiln sites | P3-15 |
   | Laravel Reverb | a Laravel package: a per-project process, not a server binary | P3-16 |

   - **Redis:**
     - Redis has no official Windows build. Herd's docs don't say which build Herd Pro
       uses.
     - DBngin uses `zkteco-home/redis-windows`, an MIT-licensed Windows port of Redis.
     - Redis itself is source-available since 7.4 (RSALv2/SSPL). Local development is within
       its terms. Under ADR 0003 it may be downloaded from its publisher at the user's
       request, but never redistributed by Kiln.
     - P3-06 chooses between that port and Valkey (BSD) built by Kiln. Its releases carry
       no files on GitHub, so where its binaries come from is checked first. Garnet stays
       a fallback ([I-75](../issues.md#i-75)).
   - **MinIO** now publishes source only (`go install …`); RustFS covers S3-compatible
     storage.
   - **Not planned:** services with no Windows build (Typesense, ClickHouse, Gotenberg,
     LocalStack, Beanstalkd, Kafka). An optional WSL2 or container backend may come later.
3. **Tests run on GitHub Actions' Windows runners** (P3-13), not in local VMs:
   - The scenario runs on `windows-latest` on every push to `main`: services, ports, data,
     `.env` wiring, and uninstall with the before/after comparison.
   - A runner has no desktop session and no Herd. Herd coexistence and Windows' certificate
     dialog are already proven in the VM ([ADR 0024](0024-integration-test-vms.md)), and are
     checked by hand before a release.
   - The VM scripts stay for those checks.
4. **Smaller ceremony per task:**
   - one ADR per group of similar services, not one per service;
   - one CI run per task instead of two VM runs;
   - several simple providers per task where they share a pattern.

## Consequences

- Windows users get Herd Pro's service set without paying, plus phpMyAdmin and Adminer.
- Linux users wait. Lerd serves them well in the meantime.
- Each push gets an integration run at no cost in RAM or attention. Herd coexistence is
  checked less often: by hand, before releases.
- The plan's Phase 3 task list and exit criteria change accordingly (plan.md).

## Alternatives considered

- **Keep both platforms in lockstep.** Rejected. Linux services need a container design
  first, and the doubled VM work was the main cost of the last two weeks.
- **Containers on Windows too (WSL2, Docker, Podman).** Rejected as the default: a heavy
  install and a second network stack, and Herd and DBngin show that native services work on
  Windows. Kept as a possible opt-in for services with no Windows build.
- **Only official Windows builds, so no Redis.** Rejected. Redis is what Laravel projects
  expect, and the user asked for it.

## Amendment (2026-11-04): no test VMs

The three Hyper-V VMs (`kiln-win11`, `kiln-ubuntu`, `kiln-spike02`, about 124 GB) were
deleted at the user's request.

- **Tests run on GitHub Actions' Windows runners** (P3-13), as decided above.
- **Herd coexistence and Windows' certificate dialog** need a desktop session and Herd.
  They are checked by hand before a release on a developer's own machine, which runs Laravel
  Herd, instead of in `kiln-win11`.
- The VM scripts (`tests/vm/…`) stay in the repository. `new-vm.ps1` and the setup scripts
  can rebuild a VM if one is ever needed again, for example when Linux resumes.
