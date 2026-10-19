//! Domain types shared across Kiln.
//!
//! Phase 0 only defines the validated names that later modules (sites, DNS, certificates)
//! build on. Everything here is pure and platform independent.

mod names;

pub use names::{HostName, NameError, Tld};
