//! Kiln's local certificate authority ([ADR 0010](../../../docs/adr/0010-local-certificate-authority.md)).
//!
//! - [`authority`]: generating the name-constrained root and the short-lived intermediate.
//! - [`store`]: the on-disk CA directory; the root key is only ever stored protected
//!   (DPAPI on Windows, the desktop's Secret Service on Linux) and never handed to the web
//!   server.
//! - `secret_service` (Linux): the Secret Service backend
//!   ([ADR 0021](../../../docs/adr/0021-linux-ca-key-and-trust.md)).

pub mod authority;
#[cfg(target_os = "linux")]
pub mod secret_service;
pub mod store;

pub use authority::{
    INTERMEDIATE_DAYS, Intermediate, PkiError, ROOT_DAYS, ROOT_MARKER, Root, describe,
    generate_root, issue_intermediate, kiln_root_tld, sha1_thumbprint, sha256_fingerprint,
};
pub use store::{CaStore, CertInfo, KeyProtector, default_protector, pem_to_der};
