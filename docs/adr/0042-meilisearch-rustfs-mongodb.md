# 0042. Meilisearch, RustFS and MongoDB on Windows; companion archives

- **Status:** accepted
- **Date:** 2026-11-02
- **Task:** P3-14 · Builds on [ADR 0035](0035-services.md), [0036](0036-service-framework.md), [0037](0037-service-ports-and-coexistence.md), [0038](0038-mysql-and-mariadb.md) §2, [0039](0039-windows-first-and-services-catalog.md) §2, [0040](0040-postgresql.md) §3

## Context

Three more services from Laravel Herd Pro's list, all with official Windows builds
([ADR 0039](0039-windows-first-and-services-catalog.md)). One ADR covers the group, as that
ADR asked.

| | Meilisearch 1.56.1 | RustFS 1.0.4 | MongoDB 9.0.4 and 8.0.34 |
|---|---|---|---|
| Download | `meilisearch-windows-amd64.exe`, 353 MB: the program itself | `rustfs-windows-x86_64-v1.0.4.zip`, 110 MB: one `rustfs.exe` (348 MB) | `mongodb-windows-x86_64-<v>.zip`, 989 MB (9.0) and 808 MB (8.0), most of it `.pdb` debug symbols |
| Published check | GitHub's asset digest | GitHub's asset digest | SHA-256 in MongoDB's release feed (`downloads.mongodb.org/full.json`) |
| License | MIT, Community Edition. The `-enterprise-` binary is under another license | Apache-2.0 | SSPLv1 (Community Server) |
| Default listen address | 127.0.0.1:7700, but the flag is given anyway | `:9000` (S3) and `:9001` (console): **every interface** | 127.0.0.1:27017 |
| Clean stop on Windows | none needed: LMDB keeps committed writes when killed | none offered | `mongod --shutdown` is Linux-only; shutdown goes through a client |

The supervisor can't send a polite stop signal on Windows: after a provider's stop command,
it kills the program ([ADR 0036](0036-service-framework.md) §1). MongoDB therefore needs
its shell, `mongosh` (Apache-2.0, a separate 63 MB download from GitHub), for a clean
shutdown. `kiln db` needs it too.

Two installer limits came up:

- Single-file installs read the executable into memory, which is too much for a 348 MB
  program.
- The downloader refused anything over 512 MB, and MongoDB's archive is nearly 1 GB.

## Decision

1. **Companions:**
   - A provider may name other archives a release needs (`companions()`): pinned like a
     release, and each unpacked into its own folder in the version's `bin`, named after
     its top folder.
   - They're installed after the server, whose install clears `bin`, and only if missing.
   - The contract tests check their pins like any other.
   - Single-file installs now stream to disk. A download that is itself the executable
     (Meilisearch) is copied.
   - The download limit is now 2 GB.
2. **Meilisearch** (`meilisearch`, port role `http`, 7700):
   - The Community Edition, run as
     `kiln-meilisearch --http-addr 127.0.0.1:<port> --db-path <data> --env development --no-analytics`.
   - Dumps and snapshots go in the instance folder.
   - Development mode needs no master key, which matches a local Scout setup.
   - `--no-analytics` keeps it from sending usage data.
   - Stopping kills it, which its store handles.
3. **RustFS** (`rustfs`, port roles `s3`, 9000, and `console`, 9001):
   - `kiln-rustfs server --address 127.0.0.1:<s3> --console-address 127.0.0.1:<console>
     data`. Both addresses are given because RustFS's defaults listen on every interface.
   - The volume is `data`, relative to the instance folder it runs in: RustFS splits its
     volume list at spaces, and a user's folder name may have one.
   - Credentials are RustFS's own defaults (`rustfsadmin` for both), set explicitly
     through its environment variables. The server is reachable from this computer only.
   - Stopping kills it, as RustFS offers no stop command. That's acceptable for local
     development.
4. **MongoDB** (`mongodb`, port role `mongodb`, 27017):
   - Two lines: 9.0 (current) and 8.0 (long-term), named by major version
     (`mongodb-9`).
   - Kept from the archive: `mongod.exe` and the licence files. Left out: debug symbols,
     `mongos`, the bundled VC++ installer, and Compass's installer script.
   - Runs as `kiln-mongod --bind_ip=127.0.0.1 --port=<port> --dbpath=<data>
     --wiredTigerCacheSizeGB=0.25`. WiredTiger's default cache, half the computer's
     memory, is sized for a dedicated server.
   - **Stop:** `mongosh … --eval "db.getSiblingDB('admin').shutdownServer()"`. An
     instance counts as stopped once `mongod.lock` is empty, which mongod does at the end
     of a clean shutdown.
   - **`mongosh` stays inside Kiln's folders:**
     - Its `%APPDATA%` and `%LOCALAPPDATA%` (history, logs) point into the instance
       folder.
     - Its global config, `mongosh.cfg` next to `mongosh.exe`, turns off telemetry and
       update checks. Kiln writes it after install.
   - **Client:** `kiln db mongodb-9 [database]` opens `mongosh`.
   - The license line says SSPL, downloaded from MongoDB at the user's request and never
     redistributed ([ADR 0003](0003-license-and-dependency-policy.md)).
5. **Contract tests** accept a port given as `--port=<n>`, and a config line in Redis's
   (`port 6379`) or PostgreSQL's form, as well as MySQL's.
6. **Like every service,** these run without administrator rights
   ([ADR 0040](0040-postgresql.md) §3).

## Consequences

- Windows now has Herd Pro's set except Reverb (P3-16): mail, MySQL, MariaDB, PostgreSQL,
  Redis, Meilisearch, S3 storage and MongoDB.
- **Download sizes:** MongoDB costs a large download (about 1 GB per version) for about
  70 MB kept. Downloads resume ([I-74](../issues.md#i-74)), and the archive stays in Kiln's
  cache until uninstall.
- **Clean stops:** Meilisearch and RustFS stop by being killed. A write in progress at that
  moment can be lost, which local development tolerates.
- **CI checks per service:**
  - Meilisearch: a document indexed, found, and found again after a restart.
  - RustFS: a bucket and an object over signed S3 requests, read back after a restart;
    the console answers.
  - MongoDB: a document in and out through `kiln db`, a clean shutdown through `mongosh`,
    and nothing left under the real `%APPDATA%`.

## Alternatives considered

- **MinIO for S3.** Not possible: MinIO now publishes source only
  ([ADR 0039](0039-windows-first-and-services-catalog.md)).
- **Killing MongoDB like the others.** Rejected: an unclean shutdown makes WiredTiger
  recover from its journal at the next start, and a database should stop cleanly.
- **Kiln sending MongoDB's `shutdown` command itself.** Possible, but `kiln db` needs
  `mongosh` anyway.
- **MongoDB's MSI installer.** Rejected: it installs system-wide and registers a Windows
  service.
- **Testing MongoDB 8.0 in CI too.** Left out: it adds an 800 MB download to every run for
  the same code path.
