# Developer setup

## Requirements

| Platform | Needs |
|---|---|
| Windows 10/11 | Visual Studio 2022 Build Tools (or Community) with the **C++ workload** and a Windows SDK (tested: 10.0.26100); rustup with the **MSVC** host |
| Linux | rustup, a C toolchain (`build-essential` / `gcc`) |
| macOS | rustup, Xcode Command Line Tools |

The Rust version is pinned in `rust-toolchain.toml` (1.99.1). Install it explicitly rather
than relying on auto-install ([I-09](issues.md#i-09)):

```sh
rustup toolchain install 1.99.1 --profile minimal --component clippy rustfmt
```

The GNU toolchain on Windows is **not** supported: its bundled `dlltool` can't link current
`windows-sys` ([I-06](issues.md#i-06)).

Optional: `cargo install cargo-deny --locked` to run the license and advisory checks locally.

`kiln-state` compiles SQLite from source (`rusqlite` with `bundled`), so a C compiler is
required on every platform. That's MSVC on Windows; the toolchains listed above cover this.

## Where things are

| Crate | Role | `unsafe` |
|---|---|---|
| `kiln-core` | Domain types (TLD, host names) | forbidden |
| `kiln-ops` | Operation engine and journals; `testing` feature exports the journal contract suite | forbidden |
| `kiln-platform` | Adapter traits, hosts block editor, `HostsBlockOp` (+ `PrivilegedHosts` trait) | forbidden |
| `kiln-state` | SQLite state store (`StateStore`), JSONL import | forbidden |
| `kiln-sys` | Safe wrappers over OS FFI (`replace_file`, …) | **allowed here only** ([ADR 0008](adr/0008-unsafe-confined-to-kiln-sys.md)) |
| `kiln-helper` | Privileged helper: protocol, validation, hosts policy, server/client, lazy `HelperSession`, `kiln-helper` binary | forbidden |
| `kiln-cli` | The `kiln` binary (`plan hosts`, `state info`, `hosts set`, `hosts clear`, `undo`) | forbidden |

## Everyday commands

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

More property-test cases (CI uses 512):

```sh
PROPTEST_CASES=20000 cargo test --workspace
```

Read-only preview against the real hosts file (writes nothing):

```sh
cargo run -p kiln-cli -- plan hosts blog shop
```

## Windows shell notes (Git Bash)

- Use `MSYS_NO_PATHCONV=1` for tools taking `/CN=…`-style arguments ([I-07](issues.md#i-07)).
- Use `/c/...` paths in `PATH`, and `%windir%\System32\whoami.exe` for Windows groups ([I-08](issues.md#i-08)).

## Running the spikes

Spikes live in `spikes/` (outside the Cargo workspace). Downloads go to each spike's
git-ignored `.cache/` and are checksum- or signature-verified.

| Spike | How |
|---|---|
| Web server | `spikes/web-server/run-all.sh`, then `run-php.sh` (needs ports 8080/8443 free) |
| Windows DNS, Part 1 | Build `spikes/windows-dns/hosts-writer` with `RUSTFLAGS="-C target-feature=+crt-static"`, copy it to `sandbox/tools/`, then `wsb start -c (Get-Content sandbox\part1.wsb -Raw)` and `wsb exec … -r System` ([I-21](issues.md#i-21)) |
| Windows DNS, Part 2 | `vm/new-vm.ps1 -Iso <Win11.iso>` (Hyper-V Administrators, ~30 GB free on the target drive), then `vm/run-part2.ps1`. Use `vm/screen.ps1` to see a VM's display without a console |

Rules: never use ports 80/443, never stop processes you didn't start (match by executable path,
not name), never edit the host machine's hosts file, NRPT table or trust stores. Anything that
does goes in Sandbox or a VM.

## Current machine footprint (from Phase 0 work)

Things that exist on the development machine because of this project, and how to remove them:

| Item | Location | Remove with |
|---|---|---|
| Spike 02 test VM (~20 GB) | `E:\VMs\kiln-spike02` (Hyper-V VM `kiln-spike02`) | `Remove-VM kiln-spike02 -Force`, then delete the folder |
| Empty folder | `D:\VMs` | delete manually |
| Spike caches (PHP, Caddy, nginx, Laravel app, payloads, VM credential) | `spikes/*/.cache/` | delete the folders |
| `cargo-deny` | `%USERPROFILE%\.cargo\bin` | `cargo uninstall cargo-deny` |
| Hyper-V, Windows Sandbox, Hyper-V Administrators membership | Windows features / local group | enabled by the user for spikes |

Not changed by any Phase 0 work: the host hosts file, PATH, certificate stores, firewall,
services, and the existing Laravel Herd installation.
