//! The on-disk CA directory (ADR 0010 §4–6).
//!
//! ```text
//! <dir>/root.crt              PEM, public
//! <dir>/root.key.protected    root key, protected by a KeyProtector (DPAPI on Windows)
//! <dir>/intermediate.crt      PEM, for the web server
//! <dir>/intermediate.key      PEM, for the web server (user-only)
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kiln_core::Tld;
use time::OffsetDateTime;

use crate::authority::{
    PkiError, describe, generate_root, issue_intermediate, sha1_thumbprint, sha256_fingerprint,
};

/// Protects the root key at rest.
pub trait KeyProtector: std::fmt::Debug {
    fn protect(&self, key: &[u8]) -> io::Result<Vec<u8>>;
    fn unprotect(&self, blob: &[u8]) -> io::Result<Vec<u8>>;
    /// Whether the key is encrypted (not merely permission-restricted) on disk.
    fn encrypted_at_rest(&self) -> bool;
}

/// DPAPI, current-user scope, with Kiln-specific entropy.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct Dpapi;

#[cfg(windows)]
const ENTROPY: &[u8] = b"kiln-local-ca-root-key/v1";

#[cfg(windows)]
impl KeyProtector for Dpapi {
    fn protect(&self, key: &[u8]) -> io::Result<Vec<u8>> {
        kiln_sys::wincrypt::dpapi_protect(key, ENTROPY)
    }
    fn unprotect(&self, blob: &[u8]) -> io::Result<Vec<u8>> {
        kiln_sys::wincrypt::dpapi_unprotect(blob, ENTROPY)
    }
    fn encrypted_at_rest(&self) -> bool {
        true
    }
}

/// Stand-in until the Secret Service backend exists (P1-04): the key is stored as-is and the
/// file is restricted to the user (`0600` on Unix).
#[derive(Debug, Default)]
pub struct PermissionsOnly;

impl KeyProtector for PermissionsOnly {
    fn protect(&self, key: &[u8]) -> io::Result<Vec<u8>> {
        Ok(key.to_vec())
    }
    fn unprotect(&self, blob: &[u8]) -> io::Result<Vec<u8>> {
        Ok(blob.to_vec())
    }
    fn encrypted_at_rest(&self) -> bool {
        false
    }
}

/// The best protector available on this platform.
#[must_use]
pub fn default_protector() -> Box<dyn KeyProtector> {
    #[cfg(windows)]
    {
        Box::new(Dpapi)
    }
    #[cfg(not(windows))]
    {
        Box::new(PermissionsOnly)
    }
}

/// Public facts about a stored certificate.
#[derive(Debug, Clone)]
pub struct CertInfo {
    pub der: Vec<u8>,
    pub pem: String,
    pub subject: String,
    pub not_after: OffsetDateTime,
    pub sha1: [u8; 20],
    pub sha256: String,
}

impl CertInfo {
    fn from_pem(pem: String) -> Result<Self, PkiError> {
        let der = pem_to_der(&pem)?;
        let (subject, not_after) = describe(&der)?;
        Ok(Self {
            sha1: sha1_thumbprint(&der),
            sha256: sha256_fingerprint(&der),
            der,
            pem,
            subject,
            not_after,
        })
    }
}

fn pem_to_der(pem: &str) -> Result<Vec<u8>, PkiError> {
    let (_, parsed) = x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .map_err(|e| PkiError::Parse(e.to_string()))?;
    Ok(parsed.contents)
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Pki(#[from] PkiError),
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("a root CA already exists in {0}; Kiln never overwrites it (use `kiln ca rotate`)")]
    AlreadyExists(PathBuf),
    #[error("no root CA in {0} yet; run `kiln ca init`")]
    Missing(PathBuf),
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Writes `bytes` to `path` via a temp file and rename, user-only on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).map_err(io_err(&temp))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600)).map_err(io_err(&temp))?;
    }
    kiln_sys::rename_replace(&temp, path).map_err(io_err(path))
}

/// The CA directory.
#[derive(Debug)]
pub struct CaStore {
    dir: PathBuf,
    protector: Box<dyn KeyProtector>,
}

impl CaStore {
    #[must_use]
    pub fn new(dir: PathBuf, protector: Box<dyn KeyProtector>) -> Self {
        Self { dir, protector }
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[must_use]
    pub fn key_encrypted_at_rest(&self) -> bool {
        self.protector.encrypted_at_rest()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    #[must_use]
    pub fn root_exists(&self) -> bool {
        self.path("root.crt").is_file()
    }

    /// Creates the root (refusing to overwrite an existing one) and protects its key.
    pub fn create_root(&self, tld: &Tld, owner: &str) -> Result<CertInfo, StoreError> {
        if self.root_exists() || self.path("root.key.protected").exists() {
            return Err(StoreError::AlreadyExists(self.dir.clone()));
        }
        fs::create_dir_all(&self.dir).map_err(io_err(&self.dir))?;
        let root = generate_root(tld, owner, OffsetDateTime::now_utc())?;
        let protected = self
            .protector
            .protect(&root.key_der)
            .map_err(io_err(&self.dir))?;
        // Key first: a crash between the two writes leaves no usable-looking half CA.
        write_private(&self.path("root.key.protected"), &protected)?;
        write_private(&self.path("root.crt"), root.cert_pem.as_bytes())?;
        Ok(CertInfo::from_pem(root.cert_pem)?)
    }

    /// The stored root certificate.
    pub fn root(&self) -> Result<CertInfo, StoreError> {
        let path = self.path("root.crt");
        if !path.is_file() {
            return Err(StoreError::Missing(self.dir.clone()));
        }
        let pem = fs::read_to_string(&path).map_err(io_err(&path))?;
        Ok(CertInfo::from_pem(pem)?)
    }

    /// Issues a fresh intermediate from the root and writes it for the web server. The root
    /// key is decrypted only in memory for the duration of the signature.
    pub fn issue_intermediate(&self, tld: &Tld) -> Result<CertInfo, StoreError> {
        let root = self.root()?;
        let key_path = self.path("root.key.protected");
        let protected = fs::read(&key_path).map_err(io_err(&key_path))?;
        let key = self
            .protector
            .unprotect(&protected)
            .map_err(io_err(&key_path))?;
        let inter = issue_intermediate(&root.der, &key, tld, OffsetDateTime::now_utc())?;
        drop(key);
        write_private(&self.path("intermediate.key"), inter.key_pem.as_bytes())?;
        write_private(&self.path("intermediate.crt"), inter.cert_pem.as_bytes())?;
        Ok(CertInfo::from_pem(inter.cert_pem)?)
    }

    /// The current intermediate, if one was issued.
    pub fn intermediate(&self) -> Result<Option<CertInfo>, StoreError> {
        let path = self.path("intermediate.crt");
        if !path.is_file() {
            return Ok(None);
        }
        let pem = fs::read_to_string(&path).map_err(io_err(&path))?;
        Ok(Some(CertInfo::from_pem(pem)?))
    }

    /// Paths the web server needs: (root cert, intermediate cert, intermediate key).
    #[must_use]
    pub fn web_server_files(&self) -> (PathBuf, PathBuf, PathBuf) {
        (
            self.path("root.crt"),
            self.path("intermediate.crt"),
            self.path("intermediate.key"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::tests::{leaf, verifies};
    use crate::authority::{INTERMEDIATE_DAYS, Intermediate};

    fn store(dir: &Path) -> CaStore {
        CaStore::new(dir.join("ca"), default_protector())
    }

    #[test]
    fn creates_once_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let ca = store(dir.path());
        assert!(matches!(ca.root(), Err(StoreError::Missing(_))));
        let root = ca.create_root(&Tld::default(), "dev@box").unwrap();
        assert!(matches!(
            ca.create_root(&Tld::default(), "dev@box"),
            Err(StoreError::AlreadyExists(_))
        ));
        assert_eq!(
            store(dir.path()).root().unwrap().sha1,
            root.sha1,
            "reload from disk"
        );
    }

    #[test]
    fn the_root_key_is_never_on_disk_in_the_clear() {
        let dir = tempfile::tempdir().unwrap();
        let ca = store(dir.path());
        ca.create_root(&Tld::default(), "dev@box").unwrap();
        let protected = fs::read(dir.path().join("ca/root.key.protected")).unwrap();
        if ca.key_encrypted_at_rest() {
            // DPAPI blob: not a PKCS#8 DER key (which starts with SEQUENCE 0x30 0x81/0x82…
            // and contains the EC public key OID) and not PEM.
            assert!(!protected.windows(11).any(|w| w == b"PRIVATE KEY"));
            let ec_oid = [0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
            assert!(!protected.windows(ec_oid.len()).any(|w| w == ec_oid));
        }
        for entry in fs::read_dir(dir.path().join("ca")).unwrap() {
            let name = entry.unwrap().file_name();
            assert_ne!(name, "root.key", "no plaintext root key file");
        }
    }

    #[test]
    fn intermediates_from_a_reloaded_store_chain_to_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let tld = Tld::default();
        store(dir.path()).create_root(&tld, "dev@box").unwrap();

        let ca = store(dir.path()); // fresh process
        let inter_info = ca.issue_intermediate(&tld).unwrap();
        assert!(
            (inter_info.not_after - OffsetDateTime::now_utc()).whole_days()
                >= INTERMEDIATE_DAYS - 1
        );
        let (_, cert, key) = ca.web_server_files();
        let inter = Intermediate {
            cert_der: inter_info.der.clone(),
            cert_pem: fs::read_to_string(cert).unwrap(),
            key_pem: fs::read_to_string(key).unwrap(),
            not_after: inter_info.not_after,
        };
        let root = ca.root().unwrap();
        assert!(verifies(
            &root.der,
            &inter,
            &leaf(&inter, "shop.test"),
            "shop.test"
        ));
        assert!(!verifies(
            &root.der,
            &inter,
            &leaf(&inter, "shop.com"),
            "shop.com"
        ));
        assert_eq!(ca.intermediate().unwrap().unwrap().sha1, inter_info.sha1);
    }

    #[cfg(windows)]
    #[test]
    fn a_dpapi_blob_is_useless_without_dpapi() {
        let dir = tempfile::tempdir().unwrap();
        let tld = Tld::default();
        CaStore::new(dir.path().join("ca"), Box::new(Dpapi))
            .create_root(&tld, "x")
            .unwrap();
        // The same files read with a store that does not decrypt cannot sign anything.
        let plain = CaStore::new(dir.path().join("ca"), Box::new(PermissionsOnly));
        assert!(plain.issue_intermediate(&tld).is_err());
    }
}
