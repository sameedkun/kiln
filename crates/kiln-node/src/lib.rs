//! Node runtimes ([ADR 0028]): nodejs.org's release list, version specs, signature-checked
//! downloads, and the installs in Kiln's folder.
//!
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

pub mod index;
pub mod install;
pub mod keys;
pub mod pgp;
pub mod spec;

pub use index::{NodeRelease, Platform};
pub use install::{InstalledNode, installed};
pub use spec::Spec;
