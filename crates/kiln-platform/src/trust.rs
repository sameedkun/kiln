//! Trusting Kiln's root CA for the current Windows user (ADR 0010 §7–9).
//!
//! `TrustCaOp` adds the root to one of the current user's certificate stores (`Root` in
//! production, where Windows shows its own confirmation dialog) and removes it on undo, but
//! only if Kiln added it. No administrator rights are involved.

use std::fmt::Write as _;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use kiln_sys::wincrypt::CertStore;
use serde_json::{Value, json};

/// Journal kind of [`TrustCaOp`].
pub const TRUST_CA_KIND: &str = "trust.ca.user";

/// Certificates whose subject lacks this are refused by the store wrapper.
pub const KILN_ROOT_MARKER: &str = "Kiln Local Development CA";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn failed(e: impl std::fmt::Display) -> OpError {
    OpError::Failed(e.to_string())
}

/// Adds a Kiln root certificate to the current user's `store`.
#[derive(Debug, Clone)]
pub struct TrustCaOp {
    store: String,
    cert_der: Vec<u8>,
    sha1: [u8; 20],
    subject: String,
}

impl TrustCaOp {
    /// `sha1` is the certificate's thumbprint; `subject` is shown in previews.
    #[must_use]
    pub fn new(
        store: impl Into<String>,
        cert_der: Vec<u8>,
        sha1: [u8; 20],
        subject: impl Into<String>,
    ) -> Self {
        Self {
            store: store.into(),
            cert_der,
            sha1,
            subject: subject.into(),
        }
    }

    pub fn from_params(params: &Value) -> Result<Self, OpError> {
        let text = |key: &str| {
            params[key]
                .as_str()
                .ok_or_else(|| failed(format!("params.{key} missing")))
        };
        let cert_der =
            unhex(text("cert_der")?).ok_or_else(|| failed("params.cert_der is not hex"))?;
        let sha1: [u8; 20] = unhex(text("sha1")?)
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| failed("params.sha1 is not a 20-byte hex thumbprint"))?;
        Ok(Self::new(text("store")?, cert_der, sha1, text("subject")?))
    }

    fn open(&self) -> Result<CertStore, OpError> {
        CertStore::open_current_user(&self.store).map_err(failed)
    }

    fn present(&self) -> Result<bool, OpError> {
        self.open()?.contains(&self.sha1).map_err(failed)
    }
}

impl Operation for TrustCaOp {
    fn kind(&self) -> &'static str {
        TRUST_CA_KIND
    }

    fn params(&self) -> Value {
        json!({
            "store": self.store,
            "cert_der": hex(&self.cert_der),
            "sha1": hex(&self.sha1),
            "subject": self.subject,
        })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let present = self.present()?;
        Ok(Preview {
            summary: if present {
                format!("Kiln's CA is already trusted in your {} store", self.store)
            } else {
                format!(
                    "Trust Kiln's local CA for your Windows account (store: {}); it can only vouch for development names",
                    self.store
                )
            },
            changes: vec![Change {
                target: format!(r"Cert:\CurrentUser\{}", self.store),
                before: present.then(|| self.subject.clone()),
                after: Some(format!("{} (SHA-1 {})", self.subject, hex(&self.sha1))),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        Ok(Receipt(json!({ "was_present": self.present()? })))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        if self.present()? {
            return Ok(());
        }
        // Windows shows its own "install this certificate?" dialog for the Root store.
        self.open()?
            .add(&self.cert_der, KILN_ROOT_MARKER)
            .map(drop)
            .map_err(failed)
    }

    fn verify(&self) -> Result<(), OpError> {
        if self.present()? {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "Kiln's CA is not in Cert:\\CurrentUser\\{} (was the Windows prompt declined?)",
                self.store
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, _force: bool) -> Result<(), OpError> {
        if receipt.0["was_present"].as_bool().unwrap_or(true) {
            return Ok(()); // trusted before Kiln touched it: leave it
        }
        self.open()?.remove(&self.sha1).map(drop).map_err(failed)
    }
}

/// Lets the engine rebuild [`TrustCaOp`]s from the journal.
pub fn register_trust_ca(registry: &mut Registry) {
    registry.register(TRUST_CA_KIND, |params| {
        Ok(Box::new(TrustCaOp::from_params(params)?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::Tld;
    use kiln_ops::{Engine, MemoryJournal};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A private store per test: no Windows dialogs, removed afterwards.
    struct TestStore(String);
    impl TestStore {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            Self(format!(
                "KilnTest-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
        }
    }
    impl Drop for TestStore {
        fn drop(&mut self) {
            let _ = CertStore::delete_current_user(&self.0);
        }
    }

    fn kiln_root() -> (Vec<u8>, [u8; 20], String) {
        let root =
            kiln_pki::generate_root(&Tld::default(), "test@box", time::OffsetDateTime::now_utc())
                .unwrap();
        let sha1 = kiln_pki::sha1_thumbprint(&root.cert_der);
        let (subject, _) = kiln_pki::describe(&root.cert_der).unwrap();
        (root.cert_der, sha1, subject)
    }

    fn engine() -> Engine<MemoryJournal> {
        let mut registry = Registry::default();
        register_trust_ca(&mut registry);
        Engine::new(MemoryJournal::default(), registry)
    }

    #[test]
    fn trust_then_undo_from_the_journal() {
        let store = TestStore::new();
        let (der, sha1, subject) = kiln_root();
        let op = TrustCaOp::new(&store.0, der, sha1, subject);
        let mut engine = engine();
        engine
            .apply(&[Box::new(op.clone()) as Box<dyn Operation>])
            .unwrap();
        assert!(
            CertStore::open_current_user(&store.0)
                .unwrap()
                .contains(&sha1)
                .unwrap()
        );

        engine.revert_all(false).unwrap(); // rebuilt from params via the registry
        assert!(
            !CertStore::open_current_user(&store.0)
                .unwrap()
                .contains(&sha1)
                .unwrap()
        );
    }

    #[test]
    fn a_certificate_trusted_before_kiln_is_left_alone() {
        let store = TestStore::new();
        let (der, sha1, subject) = kiln_root();
        CertStore::open_current_user(&store.0)
            .unwrap()
            .add(&der, KILN_ROOT_MARKER)
            .unwrap();
        let mut engine = engine();
        engine
            .apply(&[Box::new(TrustCaOp::new(&store.0, der, sha1, subject)) as Box<dyn Operation>])
            .unwrap();
        engine.revert_all(false).unwrap();
        assert!(
            CertStore::open_current_user(&store.0)
                .unwrap()
                .contains(&sha1)
                .unwrap(),
            "undo only removes what Kiln added"
        );
    }

    #[test]
    fn foreign_certificates_are_refused_by_the_store_wrapper() {
        let store = TestStore::new();
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::default();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Totally Legit Root");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let evil = params.self_signed(&key).unwrap().der().to_vec();
        let sha1 = kiln_pki::sha1_thumbprint(&evil);

        let op = TrustCaOp::new(&store.0, evil, sha1, "Totally Legit Root");
        let receipt = op.prepare().unwrap();
        assert!(op.apply(&receipt).is_err());
        assert!(
            !CertStore::open_current_user(&store.0)
                .unwrap()
                .contains(&sha1)
                .unwrap()
        );
    }

    #[test]
    fn params_round_trip_and_reject_garbage() {
        let (der, sha1, subject) = kiln_root();
        let op = TrustCaOp::new("Root", der, sha1, subject);
        let rebuilt = TrustCaOp::from_params(&op.params()).unwrap();
        assert_eq!(rebuilt.params(), op.params());
        assert!(
            TrustCaOp::from_params(
                &json!({ "store": "Root", "cert_der": "zz", "sha1": "00", "subject": "x" })
            )
            .is_err()
        );
    }
}
