# 0007. Platform delivery order: Windows and Linux first, macOS from Phase 3

- **Status:** accepted
- **Date:** 2026-10-19

## Context

Building all three platforms at once raises MVP effort by about 40% and needs Apple signing
and notarization from day one. On Windows, existing tools rely on machine-wide changes and lack
PHP-FPM, so our design is most differentiated there. Linux has no polished equivalent at all.

## Decision

Phases 1–2 deliver Windows and Linux. macOS adapters (resolver file, Keychain, launchd,
`SMAppService` helper, notarization) start in Phase 3, once the adapter contract tests have
proven out. CI builds and tests the platform-independent crates on macOS from the start, so
portability regressions show up early.

## Consequences

macOS users wait for Phase 3. Adapter interfaces must not encode Windows or Linux assumptions,
and the shared contract tests enforce this.
