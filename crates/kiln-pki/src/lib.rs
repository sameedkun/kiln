//! Kiln's local certificate authority ([ADR 0010](../../../docs/adr/0010-local-certificate-authority.md)).
//!
//! - [`authority`]: generating the name-constrained root and the short-lived intermediate.
//! - [`store`]: the on-disk CA directory; the root key is only ever stored protected
//!   (DPAPI on Windows) and never handed to the web server.

pub mod authority;
pub mod store;

pub use authority::{
    INTERMEDIATE_DAYS, Intermediate, PkiError, ROOT_DAYS, ROOT_MARKER, Root, describe,
    generate_root, issue_intermediate, sha1_thumbprint, sha256_fingerprint,
};
pub use store::{CaStore, CertInfo, KeyProtector, default_protector};
