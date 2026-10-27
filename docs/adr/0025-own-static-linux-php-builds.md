# 0025. Kiln's own static PHP builds for Linux, published from a public repo

- **Status:** accepted
- **Date:** 2026-10-27
- **Task:** P1-15 follow-up · Amends [ADR 0022](0022-linux-php-runtimes-and-fpm.md) §1; issue [I-62](../issues.md#i-62)

## Context

Kiln installs static-php-cli builds on Linux, with each archive's SHA-256 pinned in Kiln
([ADR 0022](0022-linux-php-runtimes-and-fpm.md)). It uses the `common` set from
dl.static-php.dev.

The P1-15 VM scenario with a real WordPress site showed that set can't run WordPress. It has
`mysqlnd` and `pdo_mysql`, but WordPress requires `mysqli` and stops with "Requirements Not
Met" ([I-62](../issues.md#i-62)).

None of the published sets fits:

| Set | `mysqli` (WordPress) | `pdo_sqlite` (Laravel's default database) |
|---|---|---|
| `common` | no | yes |
| `bulk`, `gnu-bulk` | yes | no |

Kiln's Windows PHP enables both, plus `intl`, `sodium` and `opcache`. The Linux and Windows
defaults should match, so a project behaves the same on either.

## Decision

1. **Kiln builds its own static PHP for Linux** with static-php-cli, using the same recipe
   static-php.dev uses for `common` (`--with-suggests`, prebuilt libraries) with these
   changes:
   - **Extensions:** the `common` list plus `mysqli`, `intl`, `sodium` and `opcache`. This
     matches Kiln's Windows defaults.
   - **No UPX packing:** packed binaries are often flagged by antivirus software, and size
     isn't a concern here.
   - **static-php-cli pinned to a commit,** not a branch, so a build can be repeated.
   - **Output:** PHP 8.2–8.5, CLI and FPM, x86-64. aarch64 can follow with the same workflow.
2. **The builds live in a separate public repository, `sameedkun/kiln-php`** (Apache-2.0).
   It holds only the build workflow and its release files: the archives, a `.sha256` file
   for each, and the licences of everything statically linked (`spc dump-license`). Reasons:
   - PHP security releases can be rebuilt and published without a Kiln release.
   - Kiln's own releases stay small.
   - The repository stays public after Kiln's own repository goes public, so nothing moves.
3. **Kiln trusts only what it pins.** Each archive's SHA-256 is recorded in
   `kiln_php::linux::STATIC_BUILDS` after the build's own checksum file, an independent
   download, and a run (`php -v`, `php -m`, `php-fpm -t`) agree. As before, a pin is never
   added without that check.
4. **Archive names stay static-php-cli's** (`php-8.4.23-cli-linux-x86_64.tar.gz`, containing
   `php`), so the install code is unchanged. Only the download base moves to the release:
   `https://github.com/sameedkun/kiln-php/releases/download/<tag>/`.

## Consequences

- WordPress and Laravel both run on Kiln's Linux PHP, with the same extensions as on
  Windows.
- **Kiln now publishes binaries.** Each release must carry the licence notices, and PHP
  security releases need a new build plus new pins in Kiln.
- **Builds take CI time:** about 30–60 minutes per version on GitHub's runners. They run only
  on demand.
- **Same upstream trust as before:** the build trusts static-php-cli's source downloads and
  prebuilt libraries, as `common` already did. Pinning a static-php-cli commit makes changes
  to that trust explicit.

## Alternatives considered

- **The `bulk` set:** has `mysqli` but breaks a new Laravel app's SQLite database.
- **Keep `common`; WordPress users use `--system` PHP:** the distribution's PHP works, but it
  makes WordPress a special case. Its version also follows the distribution, not the project.
- **Attach the builds to Kiln's own releases:** that repository is private for now, so
  anonymous downloads would fail. Releases would also mix Kiln and PHP update cadences.
