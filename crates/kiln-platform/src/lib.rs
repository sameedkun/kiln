//! Platform adapter interfaces and the portable pieces they share.
//!
//! Each adapter turns a desired state into [`kiln_ops::Operation`]s; it never changes the
//! system directly. Per-OS implementations arrive in Phase 1 and must pass the shared contract
//! tests (apply → verify → revert → original state).

pub mod hosts;
pub mod hosts_op;
#[cfg(windows)]
pub mod trust;
pub mod user_path;

use std::net::IpAddr;

use kiln_core::{HostName, Tld};
use kiln_ops::Operation;

/// One name → loopback address mapping managed by Kiln.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DnsEntry {
    pub name: HostName,
    pub ip: IpAddr,
}

/// Makes site names under the managed TLD resolve locally.
///
/// Implementations: hosts block (Windows default, Linux fallback), NRPT + stub (Windows
/// opt-in), `/etc/resolver` (macOS), `systemd-resolved` / `NetworkManager` (Linux).
pub trait DnsProvider {
    /// Short identifier shown in `kiln doctor`, e.g. `hosts-block`.
    fn id(&self) -> &'static str;

    /// Whether the provider resolves arbitrary subdomains without listing each name.
    fn supports_wildcards(&self) -> bool;

    /// Operation that makes exactly `entries` resolve, replacing Kiln's previous set.
    fn set_entries(&self, tld: &Tld, entries: &[DnsEntry]) -> Box<dyn Operation>;
}

/// Installs and removes Kiln's local CA in the OS (and browser) trust stores.
pub trait TrustStore {
    fn id(&self) -> &'static str;

    /// `der` is the CA certificate. Implementations must refuse certificates that do not
    /// carry Kiln's subject marker.
    fn install_ca(&self, der: &[u8]) -> Box<dyn Operation>;

    fn remove_ca(&self, sha256_thumbprint: &str) -> Box<dyn Operation>;
}

/// Adds Kiln's shim directory to the user's PATH (never the machine PATH).
pub trait PathManager {
    fn prepend_user_path(&self, dir: &std::path::Path) -> Box<dyn Operation>;
}
