# 0018. History, undo, and a previewable uninstall

- **Status:** accepted
- **Date:** 2026-10-22
- **Task:** P1-13 · Builds on [ADR 0004](0004-reversible-operation-engine.md) (operation engine), [0011](0011-caddy-integration.md)–[0016](0016-per-site-php-and-shims.md)

## Context

Every system change Kiln makes is a journaled operation: the hosts block (`dns.hosts.block`),
CA trust (`trust.ca.user`) and the user `PATH` (`env.user_path.prepend`). Each can be undone
with `kiln undo <n>`. Three gaps remain:

1. **No overview.** Operation numbers are printed once; finding them again means scrolling
   back.
2. **`kiln undo <n>` only states the kind.** It doesn't say *what* will be undone (which names,
   which certificate, which `PATH` entry).
3. **No way to remove Kiln completely.** The project brief requires a clean uninstall that
   restores what Kiln changed and removes what it added, without touching anything else.

## Decision

### History and undo

1. **`kiln history`** lists operations, newest first: number, when, kind, state, and a
   one-line description built from the operation's parameters (for example
   `hosts block: hello.test, shop.test`, `trust Kiln Local Development CA (…)`,
   `C:\…\shims first on user PATH`). `--all` includes undone and rolled-back ones; by default
   only those still in effect, or needing attention, are shown.
2. **`kiln undo`** with no number undoes the **most recent operation still in effect**.
   - With a number, it undoes that one, as before.
   - It shows the description before asking.
   - An operation that isn't in effect (already undone, rolled back) is refused with its
     state, instead of a generic error.

### Uninstall

3. **`kiln uninstall`** always prints its full plan first. **`--dry-run`** (also
   `kiln plan uninstall`) prints the plan and stops. The plan lists:
   - **stop:** Kiln's web server and PHP supervisor, if running (verified PIDs);
   - **undo:** every operation still in effect, **newest first**, with descriptions
     (`Engine::revert_all`), each restoring what was there before;
   - **delete:** Kiln's two folders, `%LOCALAPPDATA%\Kiln` (Caddy, PHP builds, downloads,
     shims, run directories) and `%APPDATA%\Kiln` (state database, CA), with their sizes;
   - **keep:** what is never touched: project folders (linked and parked), other tools
     (Herd's files and processes, other PHP installs), and the `kiln` executables themselves.
4. **Order:** stop processes → undo operations → delete folders, with the state folder last,
   since the journal is needed until every undo is done.
   - **If an undo fails, uninstall stops there and deletes nothing.** The journal stays, so
     `kiln uninstall` can be run again after fixing the cause (or `kiln undo <n> --force`).
   - Operations stuck from an earlier crash are listed. While any operation is still in effect
     or stuck, the state folder isn't deleted.
5. **Safety checks before deleting:**
   - each folder must be exactly `<APPDATA or LOCALAPPDATA>\Kiln`;
   - no linked or parked project may lie inside it (if one does, that folder is skipped and
     reported).

   Deleting goes through Rust's `remove_dir_all`, which removes junction points without
   following them, so nothing outside the folder is deleted through a link.
6. **`--keep-files`** stops processes and undoes system changes but leaves both folders, for
   example before reinstalling. `--yes` skips the confirmation (the plan is still printed).
7. Asking for confirmation is required, since uninstall is the most destructive command:
   - the hosts undo needs one UAC prompt;
   - the trust undo shows Windows' own dialog;
   - the `PATH` undo needs neither.

## Consequences

- `kiln history` + `kiln undo` cover "what did Kiln change, and take it back".
- `kiln uninstall --dry-run` answers "what exactly would uninstalling do" before anything
  happens. A real uninstall leaves the machine as it was before Kiln, apart from the `kiln`
  binaries, which the eventual installer removes.
- Uninstall can be re-run after a partial failure.

## Alternatives considered

- **Delete Kiln's folders first, then undo:** the journal would be gone, and nothing could be
  undone.
- **Restore whole files from backups instead of operation reverts:** would overwrite changes
  other tools made since (Herd edits the hosts file too). Reverts restore only Kiln's part.
- **No confirmation for uninstall:** too easy to run by mistake; `--yes` exists for scripts.
