//! Kiln's privileged helper ([ADR 0009](../../../docs/adr/0009-windows-privileged-helper.md)).
//!
//! - [`proto`]: framed, HMAC-authenticated, ordered messages; the fixed operation list.
//! - [`validate`]: the helper re-validates every request with Kiln's domain types.
//! - [`hosts`]: the hosts-file write policy (keep ACLs, refuse read-only, retry locks).
//! - `server` / `client` (Windows): the session over a private named pipe, and UAC launch.

pub mod hosts;
pub mod proto;
pub mod validate;

#[cfg(windows)]
pub mod client;
#[cfg(windows)]
pub mod server;
