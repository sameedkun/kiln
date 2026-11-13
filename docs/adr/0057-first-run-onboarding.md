# 0057. First-run onboarding: each change shown, each one consented to

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-06 · Implements [ADR 0004](0004-reversible-operation-engine.md)'s previews in the app · Builds on [ADR 0053](0053-api-v2-and-json-output.md), [0054](0054-desktop-app-shell.md), [0055](0055-sites-php-node-pages.md)

## Context

Phase 4's exit criteria ask for a first run that "previews each system change (trusting the
CA, the hosts file, the shims on PATH, starting at login) and makes none without consent".
After installing the app, nothing is set up: no web server, no PHP, no certificate
authority. A new user shouldn't have to know the CLI's order of commands.

Every piece already exists as a CLI command on the jobs allow-list:

| Change | Command | Preview |
|---|---|---|
| Install and start the web server | `web install`, `web start` | none needed |
| Install PHP | `php install`, `composer install` | none needed |
| Trust Kiln's CA | `ca init`, `ca trust` | `ca trust` |
| Projects | `park`, `link` | none needed |
| Hosts file | `hosts sync` | yes |
| Shims on PATH | `shims install` | yes |
| Start at sign-in | `app enable` | yes |

What was missing is one read of what is already done, so onboarding can skip finished steps
and the app knows when to offer it.

## Decision

1. **`kiln setup`** (a read; `setup.status` through the API) says what is set up. It is
   built from the same facts `kiln doctor` gathers:
   - the web server: installed, running;
   - the PHP versions and the default, Composer, and how many Node versions;
   - Kiln's CA: made, and trusted;
   - which sites aren't in the hosts file;
   - whether the shims are on PATH (an applied PATH or shell-profile operation);
   - what the login item starts;
   - `ready`: the web server and one PHP are there.
2. **The onboarding is a sequence of steps in the main window:**
   1. **Welcome:** what Kiln will set up, and that every change is shown first and can be
      undone in History.
   2. **Web server:** install and start.
   3. **PHP:** pick a version, optionally Composer.
   4. **HTTPS:** make Kiln's CA, then trust it, with the certificate store change shown.
   5. **Projects:** park a folder of projects or link one; then add their names to the hosts
      file, through its preview (Windows asks for administrator approval).
   6. **Terminal:** the `php`, `composer` and `node` shims on PATH, through its preview.
   7. **Sign-in:** start Kiln in the tray when you sign in, through its preview.
   8. **Done:** what was set up, and where each change can be undone.

   About the steps:
   - **Each step but the web server and PHP can be skipped.**
   - **A step that is already done** shows "Done" and moves on.
   - **Every change runs as a job,** with its preview where the command has one (ADR
     0053 §4), so the app never changes anything the user hasn't seen and agreed to.
3. **When it shows:**
   - **On its own:** when the app starts and `kiln setup` says Kiln isn't `ready`, unless the
     user dismissed it ("Not now"). The app remembers that in `app.json`.
   - **Any time:** Settings has "Set up Kiln…".
4. **Checked:**
   - **The Scenario:** `kiln --json setup` reports a finished set-up truthfully.
   - **By hand:** the steps, on a machine without Kiln.

## Consequences

- **A user can go from nothing to a served HTTPS site** without a terminal, consenting to
  each system change.
- **Same answers as the CLI:** the app's idea of "set up" is the CLI's `kiln setup`.
