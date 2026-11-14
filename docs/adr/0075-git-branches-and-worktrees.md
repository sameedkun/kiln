# 0075. Git branches and worktrees

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-11 (from Phase 5's "git worktrees as sites")

## Context

The user asked for branch switching and worktrees, as Lerd has them (2026-11-14): switch a
site's branch from the app, and serve a second branch side by side as a site of its own.

## Decision

1. **Through the user's own Git** (`git.exe` on `PATH`), run without a console window. Kiln
   doesn't ship Git; without it, the app says so.
2. **`kiln git branches <site>`** (read, also `git.branches` in kilnd): the current branch,
   the local branches, whether the tree has uncommitted changes (`git status --porcelain`
   isn't empty), and the repository's other worktrees with the sites serving them.
3. **`kiln git switch <site> <branch>`** runs `git switch` only on a clean tree; with
   changes it refuses and says to commit or stash them. Kiln never forces, stashes or
   discards anything.
4. **`kiln git worktree add <site> <branch> [--new]`** makes a worktree beside the project
   (`<project>-<branch>`, the branch as a site label) with `git worktree add` (`-b` for a new
   branch from the current one), and links it as `<branch>.<site>` (`feature-checkout.shop.test`).
   **`kiln git worktree remove <site> <branch>`** stops serving it, then runs `git worktree
   remove` without `--force`: Git itself refuses a worktree with changes, and Kiln checks
   first too. The branch stays.
5. **Branch names** are checked with Git's own `check-ref-format --branch` and can't start
   with `-`, so nothing reaches Git as an option.
6. **The app**: a "Git" section on the site's Overview: a branch to switch to (off while the
   tree has changes, saying why), the worktrees with their sites, "New worktree…".

## Consequences

- Switching a branch changes the project's files under the running site; Kiln serves what's
  there, as it would after a `git switch` in a terminal.
- A worktree is a site like any linked one: its own PHP pin, HTTPS and services wiring.
