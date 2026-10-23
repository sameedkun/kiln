//! Trusting Kiln's root in the Linux system store ([ADR 0021] §7), for command-line tools:
//! curl, PHP, Node, git. Opt-in (`kiln ca trust --system`), since it affects every user.
//!
//! Reading the anchor folder needs no privileges; adding or removing the root goes through
//! a [`PrivilegedTrust`] implementation (the helper behind polkit), which checks the
//! certificate again and runs the distribution's update tool.
//!
//! [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md

use std::path::{Path, PathBuf};
use std::rc::Rc;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`SystemTrustOp`].
pub const SYSTEM_TRUST_KIND: &str = "trust.ca.system";

/// A distribution's folder of extra trust anchors, and the program (with its arguments)
/// that rebuilds the system store from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchors {
    pub dir: PathBuf,
    pub update: PathBuf,
    pub update_args: &'static [&'static str],
}

/// The anchor folder of the system rooted at `root` (`/` in production): the first of
/// Fedora/RHEL, Arch, Debian/Ubuntu, openSUSE whose folder and update tool both exist.
#[must_use]
pub fn detect_anchors(root: &Path) -> Option<Anchors> {
    const KNOWN: [(&str, &str, &[&str]); 4] = [
        (
            "etc/pki/ca-trust/source/anchors",
            "usr/bin/update-ca-trust",
            &["extract"],
        ),
        (
            "etc/ca-certificates/trust-source/anchors",
            "usr/bin/trust",
            &["extract-compat"],
        ),
        (
            "usr/local/share/ca-certificates",
            "usr/sbin/update-ca-certificates",
            &[],
        ),
        (
            "etc/pki/trust/anchors",
            "usr/sbin/update-ca-certificates",
            &[],
        ),
    ];
    KNOWN.iter().find_map(|(dir, update, args)| {
        let dir = root.join(dir);
        let update = root.join(update);
        (dir.is_dir() && update.is_file()).then_some(Anchors {
            dir,
            update,
            update_args: args,
        })
    })
}

/// `AB:CD:…` (Kiln's fingerprint format) or plain hex → 64 lowercase hex digits.
#[must_use]
pub fn sha256_hex(fingerprint: &str) -> String {
    fingerprint
        .chars()
        .filter(char::is_ascii_hexdigit)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The file Kiln writes for a root into the anchor folder.
#[must_use]
pub fn anchor_file_name(sha256: &str) -> String {
    let hex = sha256_hex(sha256);
    format!("kiln-{}.crt", hex.get(..16).unwrap_or(&hex))
}

/// The privileged part: adding a root (PEM) to the system store, or removing the one with
/// this SHA-256. Implementations check the certificate themselves.
pub trait PrivilegedTrust {
    fn trust_system_ca(&self, pem: &str) -> Result<(), OpError>;
    fn untrust_system_ca(&self, sha256: &str) -> Result<(), OpError>;
}

/// Adds a Kiln root to the system store.
pub struct SystemTrustOp {
    pem: String,
    sha256: String,
    subject: String,
    anchors: Option<Anchors>,
    writer: Rc<dyn PrivilegedTrust>,
}

impl std::fmt::Debug for SystemTrustOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemTrustOp")
            .field("sha256", &self.sha256)
            .field("subject", &self.subject)
            .field("anchors", &self.anchors)
            .finish_non_exhaustive()
    }
}

fn failed(e: impl std::fmt::Display) -> OpError {
    OpError::Failed(e.to_string())
}

impl SystemTrustOp {
    /// `anchors` is where the distribution keeps extra roots ([`detect_anchors`]); `None`
    /// where Kiln doesn't know the distribution.
    #[must_use]
    pub fn new(
        pem: impl Into<String>,
        sha256: &str,
        subject: impl Into<String>,
        anchors: Option<Anchors>,
        writer: Rc<dyn PrivilegedTrust>,
    ) -> Self {
        Self {
            pem: pem.into(),
            sha256: sha256_hex(sha256),
            subject: subject.into(),
            anchors,
            writer,
        }
    }

    pub fn from_params(
        params: &Value,
        anchors: Option<Anchors>,
        writer: Rc<dyn PrivilegedTrust>,
    ) -> Result<Self, OpError> {
        let text = |key: &str| {
            params[key]
                .as_str()
                .ok_or_else(|| failed(format!("params.{key} missing")))
        };
        Ok(Self::new(
            text("pem")?,
            text("sha256")?,
            text("subject")?,
            anchors,
            writer,
        ))
    }

    fn anchors(&self) -> Result<&Anchors, OpError> {
        self.anchors.as_ref().ok_or_else(|| {
            failed("Kiln doesn't know where this Linux distribution keeps its trusted roots (it knows Debian/Ubuntu, Fedora/RHEL, Arch and openSUSE)")
        })
    }

    fn file(&self) -> Result<PathBuf, OpError> {
        Ok(self.anchors()?.dir.join(anchor_file_name(&self.sha256)))
    }

    fn present(&self) -> Result<bool, OpError> {
        Ok(self.file()?.is_file())
    }
}

impl Operation for SystemTrustOp {
    fn kind(&self) -> &'static str {
        SYSTEM_TRUST_KIND
    }

    fn params(&self) -> Value {
        json!({ "pem": self.pem, "sha256": self.sha256, "subject": self.subject })
    }

    fn scope(&self) -> Scope {
        Scope::Machine
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let present = self.present()?;
        Ok(Preview {
            summary: if present {
                "Kiln's CA is already in the system store".into()
            } else {
                "Trust Kiln's local CA machine-wide for command-line tools (curl, PHP, Node, git); it can only vouch for development names".into()
            },
            changes: vec![Change {
                target: self.file()?.display().to_string(),
                before: present.then(|| self.subject.clone()),
                after: Some(self.subject.clone()),
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
        self.writer.trust_system_ca(&self.pem)
    }

    fn verify(&self) -> Result<(), OpError> {
        if self.present()? {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "{} doesn't exist",
                self.file()?.display()
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, _force: bool) -> Result<(), OpError> {
        if receipt.0["was_present"].as_bool().unwrap_or(true) || !self.present()? {
            return Ok(());
        }
        self.writer.untrust_system_ca(&self.sha256)
    }
}

/// Lets the engine rebuild [`SystemTrustOp`]s from the journal.
pub fn register_system_trust(
    registry: &mut Registry,
    anchors: Option<Anchors>,
    writer: Rc<dyn PrivilegedTrust>,
) {
    registry.register(SYSTEM_TRUST_KIND, move |params| {
        Ok(Box::new(SystemTrustOp::from_params(
            params,
            anchors.clone(),
            writer.clone(),
        )?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Writes and removes anchor files directly, recording calls (the helper's job).
    struct FakeHelper {
        dir: PathBuf,
        calls: RefCell<Vec<String>>,
    }

    impl PrivilegedTrust for FakeHelper {
        fn trust_system_ca(&self, pem: &str) -> Result<(), OpError> {
            self.calls.borrow_mut().push("trust".into());
            std::fs::write(self.dir.join(anchor_file_name(&"ab".repeat(32))), pem).map_err(failed)
        }
        fn untrust_system_ca(&self, sha256: &str) -> Result<(), OpError> {
            self.calls.borrow_mut().push(format!("untrust {sha256}"));
            std::fs::remove_file(self.dir.join(anchor_file_name(sha256))).map_err(failed)
        }
    }

    fn debian(root: &Path) -> Anchors {
        std::fs::create_dir_all(root.join("usr/local/share/ca-certificates")).unwrap();
        std::fs::create_dir_all(root.join("usr/sbin")).unwrap();
        std::fs::write(root.join("usr/sbin/update-ca-certificates"), b"").unwrap();
        detect_anchors(root).unwrap()
    }

    #[test]
    fn distributions_are_told_apart_by_their_anchor_folders() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(detect_anchors(root.path()), None);
        let anchors = debian(root.path());
        assert!(anchors.dir.ends_with("usr/local/share/ca-certificates"));
        assert_eq!(anchors.update_args, &[] as &[&str]);

        // Fedora's layout wins where both exist.
        std::fs::create_dir_all(root.path().join("etc/pki/ca-trust/source/anchors")).unwrap();
        std::fs::create_dir_all(root.path().join("usr/bin")).unwrap();
        std::fs::write(root.path().join("usr/bin/update-ca-trust"), b"").unwrap();
        assert_eq!(
            detect_anchors(root.path()).unwrap().update_args,
            &["extract"]
        );
        assert_eq!(
            anchor_file_name("AB:CD:EF:01:23:45:67:89:AA:BB:CC"),
            "kiln-abcdef0123456789.crt"
        );
    }

    #[test]
    fn trusts_once_and_undo_removes_only_what_kiln_added() {
        let root = tempfile::tempdir().unwrap();
        let anchors = debian(root.path());
        let helper = Rc::new(FakeHelper {
            dir: anchors.dir.clone(),
            calls: RefCell::default(),
        });
        let op = SystemTrustOp::new(
            "PEM",
            &"AB".repeat(32),
            "CN=Kiln",
            Some(anchors.clone()),
            helper.clone(),
        );
        assert_eq!(op.scope(), Scope::Machine, "goes through the helper");
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        op.verify().unwrap();
        op.apply(&receipt).unwrap(); // already there: no second privileged call
        op.revert(&receipt, false).unwrap();
        op.revert(&receipt, false).unwrap(); // idempotent
        assert_eq!(
            *helper.calls.borrow(),
            ["trust".to_owned(), format!("untrust {}", "ab".repeat(32))]
        );

        // Already trusted before Kiln: left alone on undo.
        std::fs::write(anchors.dir.join(anchor_file_name(&"ab".repeat(32))), "PEM").unwrap();
        let receipt = op.prepare().unwrap();
        op.revert(&receipt, false).unwrap();
        assert!(op.present().unwrap());

        let rebuilt = SystemTrustOp::from_params(&op.params(), Some(anchors), helper).unwrap();
        assert_eq!(rebuilt.params(), op.params());
    }

    #[test]
    fn an_unknown_distribution_is_explained() {
        let helper = Rc::new(FakeHelper {
            dir: PathBuf::new(),
            calls: RefCell::default(),
        });
        let op = SystemTrustOp::new("PEM", "ab", "CN=Kiln", None, helper);
        let err = op.prepare().unwrap_err().to_string();
        assert!(err.contains("Debian/Ubuntu"), "{err}");
    }
}
