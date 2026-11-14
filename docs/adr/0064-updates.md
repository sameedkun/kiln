# 0064. Updates: PHP patches where they apply, and a new Kiln only when asked

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4-17 · Builds on [ADR 0052](0052-phase-4-screens-and-scope.md)

## Context

ADR 0052 asked for "PHP patch updates shown where they apply; checking GitHub for a new Kiln,
off by default". Kiln already reads windows.php.net's release index for `kiln php available`
and `kiln php update <minor>` installs a line's newest patch beside the old one.

Asking GitHub for Kiln's newest release is a request to a third party every time the app looks.
It sends nothing about the user, but it tells GitHub that someone runs Kiln, so it is the
user's choice.

## Decision

1. **`kiln updates`** (a read; `updates.check` through the API) answers with:
   - `php`: for each PHP Kiln installed (not a system PHP), the newest patch of its line in
     windows.php.net's index and whether it is newer;
   - `kiln`: this version, whether checking GitHub is on, and when it is, the newest release's
     tag, whether it is newer, and the releases page. A repository without releases yet is
     "no newer release", not an error; a failure to reach GitHub is reported, not fatal.
2. **Checking GitHub is off by default.** `kiln updates check-kiln on|off` sets
   `updates.check_kiln`. Only with it on does Kiln send one GET to
   `api.github.com/repos/sameedkun/kiln/releases/latest`, with Kiln's own user agent and
   nothing else.
3. **Versions compare by their numbers** (`v0.2.0` against `0.1.9`, `8.4.100` against
   `8.4.99`).
4. **Kiln doesn't update itself.** It says a newer Kiln is out and opens the releases page;
   installing stays the user's step until Kiln has signed installers.
5. **In the app:** the PHP page tags a version with "8.4.28 available" (its menu's "Update to
   its newest patch" does it); Settings has Updates, with the "Check GitHub for a new Kiln"
   switch, what it found, and "See Kiln x.y" opening the releases page. The app opens only
   that fixed address, never one the page gives.

## Consequences

- Patch updates show where they apply, without a background service.
- Nothing reaches GitHub until the user turns it on.
- The Scenario checks the runner's PHP is listed against the index, that GitHub checking is
  off by default, and that the switch works.
