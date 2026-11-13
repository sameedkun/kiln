# Third-party notices

Kiln is licensed under Apache-2.0. This file lists third-party material that is included in, or
adapted into, this repository. Rust crate dependencies are tracked by `Cargo.lock` and checked by
`cargo deny` (see `deny.toml`). Their notices will be generated into release artifacts.

| Component | Source | License | Used for |
|---|---|---|---|
| Contributor Covenant 2.1 | https://www.contributor-covenant.org | CC BY 4.0 | `CODE_OF_CONDUCT.md` |
| SQLite (bundled via `libsqlite3-sys`) | https://sqlite.org | Public domain | `kiln-state` database engine, compiled into the binaries |
| Inter (via `@fontsource-variable/inter`) | https://rsms.me/inter | SIL OFL 1.1 | The desktop app's text font, bundled ([ADR 0051](docs/adr/0051-desktop-visual-design.md)) |
| Cascadia Mono (via `@fontsource/cascadia-mono`) | https://github.com/microsoft/cascadia-code | SIL OFL 1.1 | The desktop app's font for paths, ports and logs, bundled |
| Fluent UI System Icons (`@fluentui/react-icons`) | https://github.com/microsoft/fluentui-system-icons | MIT | The desktop app's icons |
| React, React DOM | https://react.dev | MIT | The desktop app's frontend |
| Tauri 2 and its plugins (opener, single instance, dialog, and the file-system plugin it uses) | https://tauri.app | MIT OR Apache-2.0 | The desktop app's window, tray and WebView2 bridge |
| cssparser, cssparser-macros, selectors, dtoa-short (through Tauri) | https://github.com/servo | MPL-2.0 | Reading the app's built pages at build time; used unmodified, source on crates.io |
| option-ext (through Tauri's `dirs`) | https://github.com/soc/option-ext | MPL-2.0 | Used unmodified, source on crates.io |

The desktop app's other npm packages are tracked by `apps/desktop/package-lock.json`, and
`npm audit` runs on CI. Its Rust crates are tracked by `apps/desktop/src-tauri/Cargo.lock` and
checked by `cargo deny` with `apps/desktop/src-tauri/deny.toml`.

## Considered, not used

| Component | Source | License | Notes |
|---|---|---|---|
| Laravel Valet site drivers | https://github.com/laravel/valet | MIT | Considered for framework detection (P1-10). Not used: Kiln's drivers only detect the document root, from each framework's documented layout ([ADR 0015](docs/adr/0015-parked-folders-and-drivers.md)). No Valet code is included, so no notice is needed. |
