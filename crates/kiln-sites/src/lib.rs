//! Which sites Kiln serves ([ADR 0015], [ADR 0027]):
//!
//! - [`driver`]: a project's framework and document root (Laravel → `public/`, …).
//! - [`resolve`]: links plus the subfolders of parked folders, with precedence rules, and the
//!   settings and aliases each project's `kiln.toml` adds.
//! - [`manifest`]: `kiln.toml` itself.
//! - [`herd`]: reading a Laravel Herd `herd.yml`, to migrate it.
//!
//! This crate only reads the file system; the state database and the web server config are
//! the callers' business.
//!
//! [ADR 0015]: ../../../docs/adr/0015-parked-folders-and-drivers.md
//! [ADR 0027]: ../../../docs/adr/0027-project-config-kiln-toml.md

pub mod driver;
pub mod herd;
pub mod manifest;
pub mod resolve;

pub use driver::{Driver, detect};
pub use manifest::Manifest;
pub use resolve::{Link, Resolved, Site, Skipped, Source, parked_parent, resolve};
