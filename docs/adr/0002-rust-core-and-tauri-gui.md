# 0002. Rust for core, CLI and helper; Tauri for the GUI

- **Status:** accepted
- **Date:** 2026-10-19

## Context

Kiln needs a long-running per-user daemon, a CLI, a privileged helper and a tray GUI on
Windows, macOS and Linux. Existing tools in this space combine several runtimes, for example an
Electron app plus a PHP CLI plus a .NET service, which means large installs (~150 MB+), several
toolchains, and a privileged component written in a different stack from the rest.

## Decision

- The daemon (`kilnd`), CLI (`kiln`) and privileged helper (`kiln-helper`) are written in
  Rust, in one Cargo workspace that shares the domain and operation crates.
- The GUI is a Tauri 2 app that talks to the daemon only through the local API. It has no
  direct system access.
- The toolchain is pinned in `rust-toolchain.toml` (currently 1.99.1, edition 2024).
  `unsafe_code` is forbidden workspace-wide. FFI to OS APIs goes through vetted crates
  (`windows-rs`, etc.), and any exception needs an ADR.

## Consequences

- Single static binaries per OS. Memory safety for the privileged helper.
- GUI install size is around 10 MB instead of around 150 MB.
- Rust experience is required for core contributions; the UI layer remains web technology.
- Windows development needs the MSVC Build Tools.

## Alternatives considered

- **Go:** simpler concurrency and onboarding, but weaker ergonomics for Windows ACL and named
  pipes, and a GC runtime in the helper.
- **Electron GUI:** familiar, but heavy, and it duplicates logic outside the daemon.
- **Flutter desktop:** reuses team skills, but tray and menu-bar support is less mature.
