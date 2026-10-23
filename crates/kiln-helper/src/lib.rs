//! Kiln's privileged helper ([ADR 0009](../../../docs/adr/0009-windows-privileged-helper.md)).
//!
//! - [`proto`]: framed, HMAC-authenticated, ordered messages; the fixed operation list.
//! - [`validate`]: the helper re-validates every request with Kiln's domain types.
//! - [`hosts`]: the hosts-file write policy (keep ACLs, refuse read-only, retry locks).
//! - [`system_trust`]: Kiln's root in the Linux system store (ADR 0021).
//! - [`server`] / [`client`]: the session over a private named pipe and UAC launch (Windows),
//!   or over the stdin/stdout of a helper started by `pkexec` or `sudo` (Linux, ADR 0020).
//! - [`session`]: a lazily launched, shared session implementing
//!   `kiln_platform::hosts_op::PrivilegedHosts` (one prompt per batch).

pub mod hosts;
pub mod proto;
pub mod validate;

pub mod client;
pub mod server;
pub mod session;
pub mod system_trust;
