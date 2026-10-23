# 0023. Linux shims: `php` and `composer` copies, on PATH through a block in the shell startup files

- **Status:** accepted
- **Date:** 2026-10-23
- **Task:** P1-04 (part d) · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0016](0016-per-site-php-and-shims.md), [0022](0022-linux-php-runtimes-and-fpm.md)

## Context

On Windows, `kiln shims install` copies `kiln-shim` in as `php.exe` and `composer.exe`, and puts
their folder first on the user `PATH`, a registry value, as a journaled operation
([ADR 0016](0016-per-site-php-and-shims.md) §8–11). The shim runs the PHP of the site whose
folder contains the current one, or the default PHP.

Linux has no user `PATH` value. `PATH` is built by startup files, as seen in the Ubuntu 24.04
test VM:
- `~/.profile` is read at desktop login, and its `PATH` reaches every app started from the
  desktop. Ubuntu's copy adds `~/.local/bin` there.
- Terminals run interactive bash, which reads `~/.bashrc`, not `~/.profile`.
- zsh users have `~/.zshrc`; fish users have `~/.config/fish/conf.d/`.

Two Linux details matter for the shim:
- `std::env::current_exe()` reads `/proc/self/exe`, which resolves symlinks. A `php` symlink
  to `kiln-shim` would see itself as `kiln-shim`.
- `~/.local/bin` is a shared folder. A `php` the user or another tool put there must not be
  replaced.

## Decision

1. **The shims are copies** of `kiln-shim`, named `php` and `composer`, in
   `<local data>/bin/shims/` (`~/.local/share/kiln/bin/shims`), as on Windows.
   - The shim takes its role from its program name: `argv[0]` first (so `php` resolves even
     through a link a user makes), then `current_exe()`.
   - Path constants use the platform's separator (`bin/shims` on Linux).
2. **One script, sourced by each shell:** Kiln writes `<local data>/env`, which puts the shims
   folder first on `PATH` unless it's already there. It's idempotent, so sourcing it twice
   doesn't repeat the entry.
   - A fish version, `env.fish`, does the same.
   - These are Kiln's own files, deleted with its folder.
3. **The `PATH` change is a marked block in the startup files**, one journaled operation
   (`env.shell_profile`) per file:
   ```sh
   # kiln:begin - managed by Kiln; `kiln undo` removes this block
   . "/home/me/.local/share/kiln/env"
   # kiln:end
   ```
   - The files: `~/.profile` (desktop apps and login shells; created if missing),
     `~/.bashrc` if it exists, `~/.zshrc` if it exists or zsh is the login shell, and
     `~/.config/fish/conf.d/kiln.fish` if fish is installed (a file of its own).
   - The block goes at the **end** of each file, so it comes after the distribution's own
     `PATH` lines (`~/.local/bin`) and wins.
   - Same rules as the hosts block ([ADR 0006](0006-windows-dns-strategy.md)): every byte
     outside the block is kept, ambiguous markers are refused, line endings are kept, and the
     file is replaced atomically with its mode.
   - `prepare` records whether the file existed. `revert` removes only the block, and deletes
     the file if Kiln created it and nothing else was written to it since.
   - User files, so no password is needed.
4. **Preview and confirmation** as on Windows: Kiln shows each file's change and which `php`
   and `composer` a new terminal gets now and after, then asks.
   - Open terminals keep their `PATH`; Kiln says to open a new one.
   - Desktop apps (an IDE started from the menu) see it after the next login, since only
     `~/.profile` reaches them.
5. **Not used:** links in `~/.local/bin`, systemd's `environment.d`, and `/etc/profile.d`
   (see below).

## Consequences

- `php` and `composer` in a new terminal follow each site's PHP on Linux too, and
  `kiln undo` takes it back out of every file.
- The shell files carry a few lines that are clearly Kiln's, and nothing else of them
  changes.
- A shell Kiln doesn't know (e.g. nushell) needs `<local data>/bin/shims` added by hand;
  `kiln doctor` reports where `php` comes from.

## Alternatives considered

- **Links in `~/.local/bin`:** no file edits, but the folder is shared, isn't on `PATH` on
  every distribution, comes after other entries on some, and symlinked shims would misread
  their role (`/proc/self/exe`).
- **`~/.config/environment.d/kiln.conf`:** only reaches programs started by the systemd user
  manager, not every terminal.
- **`/etc/profile.d/kiln.sh`:** needs root and affects every user.
- **Asking users to edit their shell files:** error-prone, and nothing to undo.
