# Third-party notices

Kiln is licensed under Apache-2.0. This file lists third-party material that is included in, or
adapted into, this repository. Rust crate dependencies are tracked by `Cargo.lock` and checked by
`cargo deny` (see `deny.toml`). Their notices will be generated into release artifacts.

| Component | Source | License | Used for |
|---|---|---|---|
| Contributor Covenant 2.1 | https://www.contributor-covenant.org | CC BY 4.0 | `CODE_OF_CONDUCT.md` |
| SQLite (bundled via `libsqlite3-sys`) | https://sqlite.org | Public domain | `kiln-state` database engine, compiled into the binaries |

## Considered, not used

| Component | Source | License | Notes |
|---|---|---|---|
| Laravel Valet site drivers | https://github.com/laravel/valet | MIT | Considered for framework detection (P1-10). Not used: Kiln's drivers only detect the document root, from each framework's documented layout ([ADR 0015](docs/adr/0015-parked-folders-and-drivers.md)). No Valet code is included, so no notice is needed. |
