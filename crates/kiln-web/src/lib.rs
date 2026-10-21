//! Kiln's web server integration ([ADR 0005], [ADR 0011]):
//!
//! - [`config`]: Caddy JSON generated from typed structs. Sites are HTTP unless marked secure.
//! - [`release`]: the pinned Caddy release archives and installing the binary as `kiln-caddy`.
//! - [`process`]: starting, checking and stopping Caddy in the background.
//!
//! This crate knows nothing about the state database; callers build a [`WebConfig`] from it.
//!
//! [ADR 0005]: ../../../docs/adr/0005-web-server.md
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md

pub mod config;
pub mod process;
pub mod release;

pub use config::{CaFiles, ConfigError, SiteSpec, WebConfig};
pub use process::{Status, WebError, WebServer};
pub use release::{EXE_NAME, InstallError, Release};
