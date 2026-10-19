# Contributing to Kiln

Thanks for your interest! Kiln is pre-alpha; the architecture is settling through the ADRs in
`docs/adr`. Please open an issue to discuss larger changes before writing code.

## Clean-room rule

Kiln is an independent implementation. **Do not copy code, configuration templates, text,
icons or other assets from proprietary local-development tools** (for example Laravel Herd),
and do not submit work derived from decompiling them. Open-source projects may be reused under
their licenses, with attribution recorded in `THIRD_PARTY_NOTICES.md`. Reviewers will ask about
the origin of non-trivial code.

## Development

- Toolchain: pinned in `rust-toolchain.toml`.
- Before pushing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
- New dependencies must pass `cargo deny check` (permissive licenses only; see `deny.toml`).

## Changes that touch the system

Anything that changes state outside Kiln's own data directory (DNS, hosts file, trust stores,
PATH, services, firewall) **must** be implemented as a `kiln_ops::Operation` with:

1. a side-effect-free `preview` and `prepare`,
2. an idempotent `revert` that refuses drifted targets unless forced,
3. contract tests showing apply → verify → revert restores the original state.

## Decisions

Significant design choices are recorded as ADRs (`docs/adr/NNNN-title.md`, template in
`0000-template.md`). Propose an ADR in the same pull request as the change it justifies.

## Licensing of contributions

By contributing, you agree your contributions are licensed under Apache-2.0 (inbound =
outbound, per section 5 of the license). Sign off commits with `git commit -s`
([DCO](https://developercertificate.org/)).
