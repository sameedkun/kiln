# 0027. Project config: `kiln.toml`, read live; `herd.yml` import

- **Status:** accepted; finished by [ADR 0046](0046-project-services.md): `[services]`, and `herd.yml` services imported
- **Date:** 2026-10-28
- **Task:** P2-04 · Builds on [ADR 0015](0015-parked-folders-and-drivers.md), [0016](0016-per-site-php-and-shims.md); recon: feature-parity row "project manifest"

## Context

Today a project's settings live only in Kiln's state on one machine: `kiln isolate 8.3` and
`kiln secure`. A team can't commit "this project runs PHP 8.3 over HTTPS", and moving from
Laravel Herd means re-entering what a project's `herd.yml` already says.

Herd's [public documentation](https://herd.laravel.com/docs/windows/sites/herd-yaml.md)
describes `herd.yml`'s keys:

- `name`;
- `php` (a version);
- `secured` (a boolean);
- `aliases` (extra names, without the TLD);
- `services` and `workspaces`, both Herd Pro features.

Reading a user's file in that format to migrate it is interoperability; no Herd code or text
is used (plan principle 5).

## Decision

1. **`kiln.toml` in a project's folder** (the folder a site serves, its `project`), in TOML:

   ```toml
   [site]
   name = "shop"              # the name `kiln link` / `kiln import herd` uses by default
   secure = true              # serve over HTTPS
   aliases = ["admin", "api"] # extra names for the same site (TLD optional)

   [php]
   version = "8.3"            # a minor (newest installed patch) or an exact version

   [node]
   version = "22"             # used by the Node shims (P2-05)
   ```

   All keys are optional.
   - **Unknown keys** are reported as warnings, not errors, so a newer file still works with
     an older Kiln.
   - **A file Kiln can't read** (bad TOML, a bad version or name) is reported by
     `kiln sites` and `kiln doctor`. The site is still served, as if the file weren't there.
2. **Read live, never imported into state.** Kiln reads the file every time it resolves
   sites (`kiln sites`, `web start/reload`, `hosts sync`, the shims, `kiln which`).
   - Editing it takes effect at the next `kiln web reload`, and at the next `hosts sync`
     for new names.
   - Nothing in it is journaled: it is the user's file and Kiln never writes it, except
     `kiln import herd` creating a new one.
3. **Precedence (most specific to this machine first):**
   - **PHP:** a `kiln isolate` pin on this machine wins, then `kiln.toml`'s
     `[php] version`, then the default PHP. The pin is a deliberate local choice, such as
     trying an upgrade. `kiln which` and `kiln sites` say which source applied, and
     `kiln isolate` notes when it overrides `kiln.toml`.
   - **HTTPS:** a site is secure if Kiln's state says so or `kiln.toml` has
     `secure = true`. `secure = false` (or no key) adds nothing.
   - **Node** (P2-05): `kiln.toml` wins over `.nvmrc` / `.node-version`, since it is the
     more explicit file.
   - **`[site] name`** is the default name when the folder is linked, or imported from
     `herd.yml`. A parked subfolder keeps its folder name, so names never depend on a file
     nobody looks at.
4. **Aliases are extra sites for the same folder.** While resolving, each alias becomes
   another site with the same project, document root and HTTPS setting, marked as an alias
   of the main one. Hosts sync, Caddy and the PHP pools then serve it with no special case.
   - An alias that is already another site's name is skipped, with a warning.
   - An alias uses its main site's PHP, including that site's `kiln isolate` pin.
5. **`kiln import herd [folder]`** reads `herd.yml` and writes `kiln.toml` beside it.
   - It maps `name`, `php`, `secured` → `secure`, and `aliases`.
   - It refuses to overwrite an existing `kiln.toml` without `--force`.
   - It names what it didn't import: `services` arrive with Phase 3, and Kiln has no
     `workspaces`.
   - If the folder isn't a site yet, it links it under that name, so an imported project is
     served like any other. The user then runs `kiln hosts sync` as usual.
   - YAML parsing uses `yaml-rust2` (MIT/Apache-2.0), only in the importer. TOML uses the
     `toml` crate.

## Consequences

- A team commits `kiln.toml`, and every member gets the same PHP version, HTTPS and names
  without running Kiln commands. Local overrides stay possible.
- **Herd users migrate with one command,** keeping their committed `herd.yml` for Herd users
  on the team.
- **One file read per site per resolution:** small (a few hundred bytes), and only for sites
  being served.
- **Edits aren't watched yet:** they need a `kiln web reload`. The daemon may watch the
  files later.

## Alternatives considered

- **YAML as the native format:** familiar to Herd users, but YAML's implicit typing turns
  `php: 8.10` into the number 8.1. TOML has no such surprise, and Cargo users know it.
- **Importing `kiln.toml` into state** (`kiln init` applies it): settings would go stale
  when the file changes, and two sources of truth would disagree.
- **`kiln.toml` beating `kiln isolate`:** the shared file would silently override a
  deliberate local choice, and undoing that would mean editing a committed file.
