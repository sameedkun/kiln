# 0046. Project services: `[services]` in `kiln.toml`, `kiln services up`, `herd.yml` services

- **Status:** accepted
- **Date:** 2026-11-03
- **Task:** P3-09 · Implements [ADR 0035](0035-services.md) §10 · Finishes [ADR 0027](0027-project-config-kiln-toml.md) (§5's `services`) · Builds on [ADR 0045](0045-env-wiring.md)

## Context

A project should say which services it needs, so a teammate or a fresh machine gets them
with one command. ADR 0035 §10 planned a `[services]` table in `kiln.toml`, and `kiln
services up` in a project to create, start and wire what it lists. It also asked
`kiln import herd` to carry `herd.yml`'s services over, which ADR 0027 left for Phase 3.

[Herd's documentation](https://herd.laravel.com/docs/macos/sites/herd-yaml) describes
`herd.yml`'s `services:` as a map from a service name (`mysql`, `redis`, `postgresql`,
`meilisearch`, `minio` and others) to a `version`, usually a full release such as
`8.0.36`, and a `port`. The port can be a fixed number or a placeholder that reads `.env`,
such as `${DB_PORT}`. Kiln's own pinned releases differ: MySQL 8.4 and 9.7, Redis 8, and no
MinIO ([ADR 0039](0039-windows-first-and-services-catalog.md)).

## Decision

1. **`[services]` in `kiln.toml`:**
   - One line per service: `mysql = "8.4"`, `redis = "*"` (any version), or
     `mysql = { version = "8.4", port = 3307 }`.
   - Versions are quoted, as for PHP; unquoted, TOML would read `8.10` as `8.1`.
   - The file's order is kept: it decides which database is wired. The `toml` crate's
     `preserve_order` is on for this.
   - Bad entries are errors that name the key; unknown keys in an entry are refused.
   - Names are lowercase. `pgsql` and `postgresql` mean `postgres`, and `minio` means
     `rustfs`.
2. **`kiln services up [--project <folder>] [--yes]`** in a project that lists services.
   For each service, in order:
   - **The release:** the version asked for, or else the newest of its major line, or
     else Kiln's newest. Every fallback prints what was chosen; the last one adds "check
     it suits the project".
   - **The instance:** an existing one of that release's series, a running one first, or
     else `kiln service add` makes one, on the asked port if there is one. A reused
     instance on another port gets a note on how to move it.
   - **Start, then wire:** what was chosen is started, then each instance with `.env` keys
     is wired with `kiln env use` ([ADR 0045](0045-env-wiring.md)), showing the change
     first unless `--yes`.
   - **One database:** only the first database is wired. A second would set the same
     `DB_*` keys, so Kiln names it and how to switch.
   - A project without `.env` is set up but not wired, with a note.
   - Outside such a project, `kiln services up` starts every instance, as before.
3. **`kiln import herd`** carries `services:` into `[services]`:
   - Names become Kiln's kinds; `minio` becomes `rustfs`, with a note.
   - Versions keep their line (`8.0.36` becomes `8.0`); `services up` resolves them as
     above.
   - Fixed ports carry over. Placeholders don't, with a note: Kiln picks free ports, and
     `kiln env use` writes them to `.env`.
   - Services Kiln doesn't have are noted. Reverb comes with P3-16; others, such as
     Typesense, have no Windows build.

## Consequences

- A project carries its services in `kiln.toml`, and `kiln services up` gives anyone the
  same setup: instances reused where they exist, `.env` pointed at them.
- A project from Herd keeps its service list. Where Kiln pins other releases, it uses its
  own and says so instead of failing.
- **CI:** imports a `herd.yml` in the documented shape (MySQL `8.0.36` with
  `${DB_PORT}`, Redis `7.0.0`, `minio`, MariaDB `12.3`), runs `kiln services up`, and
  checks:
  - the fallbacks and reuse it reports;
  - MariaDB 12.3 added but not wired, being a second database;
  - the `.env` keys for MySQL, Redis and RustFS.

## Alternatives considered

- **Failing on a version Kiln doesn't pin.** Rejected: projects made with other tools
  name lines such as MySQL 8.0 or Redis 7, and a local copy of a close release serves
  them. The note keeps the choice visible.
- **Wiring every database listed.** Rejected: the last one would silently win
  `DB_CONNECTION`.
- **Keeping Herd's port placeholders.** Rejected: Kiln resolves ports itself
  ([ADR 0037](0037-service-ports-and-coexistence.md)) and writes them into `.env`, which
  is what the placeholder points at anyway.
