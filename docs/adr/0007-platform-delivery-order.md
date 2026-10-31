# 0007. Platform delivery order: Windows and Linux first, macOS from Phase 3

- **Status:** accepted; amended 2026-10-31: macOS moves out of Phase 3 into its own Phase 4b (see the end)
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

## Amendment (2026-10-31, Phase 3 planning)

macOS moves out of Phase 3, which becomes services on Windows and Linux
([ADR 0035](0035-services.md)). macOS gets its own phase, **Phase 4b**, after the GUI and
before Phase 5's installers. The phase numbers 4, 5 and 6 that other ADRs cite stay as they
were. Two reasons:

- **Testing:** the VM scenarios of [ADR 0024](0024-integration-test-vms.md) are what show
  each phase's exit criteria. Apple allows macOS VMs only on Apple hardware, and the
  project has none yet. GitHub's macOS runners have no desktop session, so they can't show
  the trust prompts, the keychain or a login item.
- **Signing:** notarization needs an Apple Developer Program account, which doesn't exist
  yet. Until then, macOS builds are unsigned and for development only. Signing and
  notarization stay with Phase 5's installers.

CI keeps building and testing the platform-independent crates on macOS, so portability
regressions still show up early. Adapter interfaces still must not encode Windows or Linux
assumptions.
