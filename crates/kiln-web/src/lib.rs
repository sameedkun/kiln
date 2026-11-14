//! Kiln's web server integration ([ADR 0005], [ADR 0011]):
//!
//! - [`config`]: Caddy JSON generated from typed structs. Sites are HTTP unless marked secure.
//! - [`release`]: the pinned Caddy release archives and installing the binary as `kiln-caddy`.
//! - [`process`]: starting, checking, reloading and stopping Caddy in the background.
//! - [`admin`]: the admin API client over its Unix socket (ADR 0012).
//! - [`issued`]: clearing site certificates issued from an earlier intermediate (I-57).
//! - [`proxy`]: proxy targets, loopback only (ADR 0032).
//!
//! This crate knows nothing about the state database; callers build a [`WebConfig`] from it.
//!
//! [ADR 0005]: ../../../docs/adr/0005-web-server.md
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md

pub mod admin;
pub mod config;
pub mod issued;
pub mod process;
pub mod proxy;
pub mod release;

pub use admin::{AdminClient, AdminError};
pub use config::{CaFiles, ConfigError, DEBUG_POOL_SUFFIX, LanShare, SiteSpec, WebConfig};
pub use process::{Status, WebError, WebServer};
pub use proxy::{ProxyTarget, TargetError};
pub use release::{EXE_NAME, InstallError, Release};
