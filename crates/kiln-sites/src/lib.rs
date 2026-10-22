//! Which sites Kiln serves ([ADR 0015]):
//!
//! - [`driver`]: a project's framework and document root (Laravel → `public/`, …).
//! - [`resolve`]: links plus the subfolders of parked folders, with precedence rules.
//!
//! This crate only reads the file system; the state database and the web server config are
//! the callers' business.
//!
//! [ADR 0015]: ../../../docs/adr/0015-parked-folders-and-drivers.md

pub mod driver;
pub mod resolve;

pub use driver::{Driver, detect};
pub use resolve::{Link, Resolved, Site, Skipped, Source, parked_parent, resolve};
