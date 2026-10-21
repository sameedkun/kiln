# 0008. `unsafe` is confined to one audited crate, `kiln-sys`

- **Status:** accepted
- **Date:** 2026-10-21
- **Resolves:** [I-38](../issues.md#i-38)

## Context

The workspace forbids `unsafe_code` ([ADR 0002](0002-rust-core-and-tauri-gui.md)). But the
Windows hosts writer must replace the hosts file with `ReplaceFileW`. Spike 02 showed that
rename-based replacement (`MoveFileEx`, `std::fs::rename`) silently drops explicit ACEs
([I-25](../issues.md#i-25)). The privileged helper will need more Win32 calls soon: named-pipe
security descriptors, client process and token checks.

We looked for a safe wrapper on crates.io (2026-10-21). `atomicwrites` 0.4, `atomic-write-file`
0.3 and `staged_file` 0.5 all replace by rename. None of them calls `ReplaceFileW`. The
`windows` / `windows-sys` crates expose the function, but calling it is `unsafe`.

## Decision

1. A new crate, **`crates/kiln-sys`**, is the **only** crate allowed to contain `unsafe`. It
   holds small, safe wrappers around OS APIs that need FFI, such as `replace_file`. Callers
   never see raw pointers or handles.
2. Inside `kiln-sys`:
   - `unsafe_code` is `deny`, with `#[allow(unsafe_code)]` permitted only on the platform
     modules that need it;
   - `unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks` and
     `clippy::multiple_unsafe_ops_per_block` are `deny`, so every `unsafe` block holds a single
     operation and carries a `// SAFETY:` justification;
   - FFI bindings come from `windows-sys` (raw, no hand-written `extern` blocks);
   - each wrapper has behavioural tests on the real OS. For `replace_file`: content replaced,
     source consumed, explicit deny ACE kept, missing target reported.
3. Every other crate keeps `[lints] workspace = true`, so `unsafe_code = "forbid"` applies. A CI
   step fails if any crate other than `kiln-sys` omits workspace lints.
4. Changes to `kiln-sys` need a reviewer other than the author, once the project has more than
   one maintainer.

## Consequences

- The unsafe surface is one small, reviewable crate. The security review planned for Phase 5
  starts there.
- Adding an OS capability means adding a safe wrapper to `kiln-sys`, not sprinkling FFI.
- On non-Windows platforms the same API is implemented with safe std calls. For now
  `replace_file` copies the target's mode and owner to the replacement, then renames. SELinux
  labels and other xattrs are a Linux follow-up in P1-04.

## Alternatives considered

- **Per-module `#[allow(unsafe_code)]` anywhere:** the unsafe surface would grow unnoticed.
- **A rename-based crate:** rejected by evidence (I-25).
- **Shelling out to PowerShell / `cmd`** for the write: slower, harder to make atomic, and a
  command-injection surface in a privileged process.
