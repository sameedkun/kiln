# 0003. Apache-2.0 license, permissive dependencies, runtime-downloaded copyleft services

- **Status:** accepted
- **Date:** 2026-10-19

## Context

We want broad adoption, including by companies, and a clear patent grant. Kiln also manages
third-party services whose licenses vary. Since 2024, Redis is under RSALv2/SSPL/AGPL. MinIO is
AGPL, MongoDB is SSPL, and Typesense is GPL-3.

## Decision

1. Kiln is licensed under **Apache-2.0**. Contributions are inbound = outbound, with DCO
   sign-off.
2. Rust dependencies must use licenses in the `deny.toml` allow-list (permissive only),
   enforced in CI by `cargo deny`.
3. Default service choices are permissive: **Valkey** (BSD) as the Redis-compatible cache, and
   no MinIO by default (evaluate SeaweedFS/Garage).
4. Copyleft or source-available services are **downloaded from the vendor at runtime**, on
   explicit user request, and never redistributed in Kiln artifacts.
5. Open-source code that is reused (for example upstream laravel/valet drivers, MIT) is
   attributed in `THIRD_PARTY_NOTICES.md`. Nothing is taken from proprietary tools (see the
   clean-room rule in `CONTRIBUTING.md`).

## Consequences

Release artifacts stay redistributable without copyleft obligations. Some services need a
download step and a license notice in the UI.
