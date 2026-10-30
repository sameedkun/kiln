//! The site certificates Caddy issued from Kiln's intermediate (I-57).
//!
//! Caddy keeps them in `storage/certificates/<CA_ID>/` and in memory, across config reloads,
//! until each nears expiry. After Kiln's CA changes (a new root, or a re-issued
//! intermediate), Caddy would go on serving certificates signed by the old intermediate,
//! which browsers reject once only the new root is trusted. So Kiln records which
//! intermediate the stored certificates came from, and clears them when it changes; a
//! running server then has to restart, since a reload keeps the in-memory copies.

use std::fs;
use std::io;
use std::path::Path;

use crate::config::CA_ID;

/// File in Caddy's storage recording the SHA-256 of the intermediate it issued from.
const MARKER: &str = "kiln-intermediate.sha256";

/// Whether the certificates in `storage_dir` were issued from the intermediate with
/// `intermediate_sha256`. Storage with no certificates at all counts as matching.
#[must_use]
pub fn issued_from(storage_dir: &Path, intermediate_sha256: &str) -> bool {
    if !storage_dir.join("certificates").join(CA_ID).exists() {
        return true;
    }
    fs::read_to_string(storage_dir.join(MARKER)).is_ok_and(|m| m.trim() == intermediate_sha256)
}

/// Whether Caddy has stored a certificate for `host` from Kiln's CA. Caddy issues them in
/// the background after a config load, so a newly secured site has none for a moment (I-69).
#[must_use]
pub fn has_certificate(storage_dir: &Path, host: &str) -> bool {
    storage_dir
        .join("certificates")
        .join(CA_ID)
        .join(host)
        .join(format!("{host}.crt"))
        .is_file()
}

/// Deletes the certificates Caddy issued from an earlier intermediate, and records
/// `intermediate_sha256` as the one the next certificates come from. Only Kiln's CA's
/// folder is touched.
pub fn forget_other_issuers(storage_dir: &Path, intermediate_sha256: &str) -> io::Result<()> {
    if !issued_from(storage_dir, intermediate_sha256) {
        let certificates = storage_dir.join("certificates").join(CA_ID);
        match fs::remove_dir_all(&certificates) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    fs::create_dir_all(storage_dir)?;
    fs::write(storage_dir.join(MARKER), intermediate_sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificates_from_another_intermediate_are_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().join("storage");
        let site = storage.join("certificates").join(CA_ID).join("blog.test");
        assert!(issued_from(&storage, "AA"), "nothing issued yet");
        assert!(!has_certificate(&storage, "blog.test"));

        fs::create_dir_all(&site).unwrap();
        fs::write(site.join("blog.test.crt"), "old").unwrap();
        assert!(has_certificate(&storage, "blog.test"));
        assert!(
            !issued_from(&storage, "AA"),
            "issued before Kiln recorded anything"
        );
        forget_other_issuers(&storage, "AA").unwrap();
        assert!(!site.exists());

        fs::create_dir_all(&site).unwrap();
        fs::write(site.join("blog.test.crt"), "from AA").unwrap();
        forget_other_issuers(&storage, "AA").unwrap();
        assert!(site.exists(), "same intermediate: kept");

        assert!(!issued_from(&storage, "BB"));
        forget_other_issuers(&storage, "BB").unwrap();
        assert!(!site.exists(), "new intermediate: cleared");
        assert!(issued_from(&storage, "BB"));

        // Other CAs' and ACME folders are never touched.
        let other = storage.join("certificates").join("acme.example");
        fs::create_dir_all(&other).unwrap();
        fs::create_dir_all(&site).unwrap();
        forget_other_issuers(&storage, "CC").unwrap();
        assert!(other.exists());
    }
}
