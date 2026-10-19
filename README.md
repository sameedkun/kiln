# Kiln

[![CI](https://github.com/sameedkun/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/sameedkun/kiln/actions/workflows/ci.yml)

> **Pre-alpha. Phase 0 (foundations).** Not usable yet. "Kiln" is a working name.

Kiln is a free, open-source, cross-platform local development environment for PHP. It serves
your projects at `https://<name>.test`, with per-project PHP versions, trusted local HTTPS, and
managed services. It runs on Windows, Linux and macOS.

## Principles

- **Every system change is explicit and reversible.** DNS, certificates, PATH and service
  registrations go through one operation engine that previews, journals, verifies and can
  undo each change. `kiln uninstall` restores what was there before.
- **Unprivileged by default.** The few operations that need admin rights go through a tiny,
  auditable helper with a fixed list of operations. There is no generic "run as admin".
- **Loopback only.** Nothing listens beyond `127.0.0.1` / `::1` unless you opt in per site.
- **Coexistence.** Kiln never stops, modifies or deletes PHP, nginx, databases or other dev
  tools it did not install.

## Repository layout

| Path | Contents |
|---|---|
| `crates/kiln-core` | Domain types (validated TLDs and host names; sites, runtimes and services next) |
| `crates/kiln-ops` | Operation engine: prepare → journal → apply → verify, batch rollback, undo, crash recovery |
| `crates/kiln-platform` | Platform adapter interfaces, plus the portable hosts-file block editor |
| `crates/kiln-cli` | The `kiln` binary. Currently only `kiln plan hosts` (read-only preview) |
| `docs/adr` | Architecture decision records |
| `docs/spikes` | Phase 0 spike plans and their results |

## Building

Requires the Rust toolchain pinned in `rust-toolchain.toml` (rustup installs it automatically).
On Windows, use the MSVC host with Visual Studio Build Tools.

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Try the read-only preview (nothing is written):

```sh
cargo run -p kiln-cli -- plan hosts blog shop
```

## License

Apache-2.0. See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
