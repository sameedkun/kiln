# 0004. All system changes go through a reversible, journaled operation engine

- **Status:** accepted (skeleton implemented in `crates/kiln-ops`)
- **Date:** 2026-10-19

## Context

A local development environment edits the hosts file or DNS resolver configuration, OS trust
stores, PATH and autostart entries. When tools change these silently, uninstall leaves debris,
such as trusted CAs, firewall rules or machine-wide environment variables. Users can't see
what changed, and a crash halfway through can leave the machine inconsistent.

## Decision

Every change outside Kiln's own data directory is a `kiln_ops::Operation` with `preview`,
`prepare` (snapshot prior state, no side effects), `apply`, `verify` and an idempotent `revert`.
The `Engine`:

1. journals the receipt as `Prepared` **before** touching the system, then `Applied` after
   verifying;
2. treats each batch as all-or-nothing, reverting partial work newest-first on any failure;
3. on startup, rolls back entries left `Prepared` by a crash (`recover`);
4. supports `revert(id)` for undo and `revert_all` for uninstall;
5. refuses to revert targets that drifted after apply unless forced, and marks entries
   `Stuck` (with the reason) instead of guessing;
6. rebuilds operations from `kind` + `params` through a `Registry`, so a different process
   can undo them later.

The journal is append-only JSON Lines, fsynced per event. A torn final line is truncated on
open, because the write was never acknowledged. Any other corruption is a hard error.

## Consequences

- Every adapter must ship contract tests: apply → verify → revert leaves the original state.
- Previews feed the CLI (`kiln plan`) and the GUI confirmation sheet from one source.
- Operations must store enough in their receipt to restore state byte-for-byte, or document
  their merge strategy for drift.

## Follow-ups

- ~~Journal storage may move into the SQLite state store.~~ **Done in P1-01 (2026-10-21):**
  `kiln_state::StateStore` implements `Journal` (latest state plus full event history). It
  passes the same `kiln_ops::testing` contract as the memory and JSONL journals, and imports an
  existing `journal.jsonl` once. `FileJournal` remains for tests and as an export format.
- Elevated operations: the engine passes `requires_elevation` batches to `kiln-helper`
  (ADR to come with the helper).
