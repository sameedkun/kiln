//! Trusting Kiln's root CA in one NSS database ([ADR 0021] §5–6): the user's
//! `~/.pki/nssdb` (Chrome and other Chromium browsers) or a Firefox profile.
//!
//! `NssTrustOp` adds the root with `certutil -A -t "C,,"` (trusted to issue server
//! certificates only) and removes it on undo, but only if Kiln added it. No administrator
//! rights are involved. Where Kiln had to create `~/.pki/nssdb` first, undo removes the
//! database again if the root was all that was in it.
//!
//! [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md

use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`NssTrustOp`].
pub const NSS_TRUST_KIND: &str = "trust.ca.nss";

/// The files of an NSS database in `sql:` format.
const DB_FILES: [&str; 3] = ["cert9.db", "key4.db", "pkcs11.txt"];

fn failed(e: impl std::fmt::Display) -> OpError {
    OpError::Failed(e.to_string())
}

/// The `certutil` commands Kiln needs, so the operation can be tested without NSS.
pub trait NssTool: std::fmt::Debug {
    /// Whether `db` has a certificate under `nickname`.
    fn contains(&self, db: &Path, nickname: &str) -> Result<bool, String>;
    /// Adds `pem` as a CA trusted for TLS servers.
    fn add(&self, db: &Path, nickname: &str, pem: &str) -> Result<(), String>;
    fn delete(&self, db: &Path, nickname: &str) -> Result<(), String>;
    /// Creates an empty database with no password, as Chrome does.
    fn create(&self, db: &Path) -> Result<(), String>;
    /// How many certificates `db` holds.
    fn count(&self, db: &Path) -> Result<usize, String>;
}

/// `certutil` from `libnss3-tools` / `nss-tools`, found on `PATH`.
#[derive(Debug, Clone)]
pub struct Certutil {
    exe: PathBuf,
}

impl Certutil {
    /// `None` if `certutil` isn't installed.
    #[must_use]
    pub fn find() -> Option<Self> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("certutil"))
            .find(|exe| exe.is_file())
            .map(|exe| Self { exe })
    }

    fn run(&self, db: &Path, args: &[&str]) -> Result<std::process::Output, String> {
        Command::new(&self.exe)
            .arg("-d")
            .arg(format!("sql:{}", db.display()))
            .args(args)
            .output()
            .map_err(|e| format!("cannot run {}: {e}", self.exe.display()))
    }

    fn check(output: &std::process::Output, what: &str) -> Result<(), String> {
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "certutil couldn't {what}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

impl NssTool for Certutil {
    fn contains(&self, db: &Path, nickname: &str) -> Result<bool, String> {
        Ok(self.run(db, &["-L", "-n", nickname])?.status.success())
    }

    fn add(&self, db: &Path, nickname: &str, pem: &str) -> Result<(), String> {
        let file = std::env::temp_dir().join(format!("kiln-root-{}.crt", std::process::id()));
        std::fs::write(&file, pem).map_err(|e| e.to_string())?;
        let file_arg = file.to_string_lossy().into_owned();
        let output = self.run(db, &["-A", "-t", "C,,", "-n", nickname, "-i", &file_arg]);
        let _ = std::fs::remove_file(&file);
        Self::check(&output?, "add the certificate")
    }

    fn delete(&self, db: &Path, nickname: &str) -> Result<(), String> {
        Self::check(
            &self.run(db, &["-D", "-n", nickname])?,
            "remove the certificate",
        )
    }

    fn create(&self, db: &Path) -> Result<(), String> {
        std::fs::create_dir_all(db).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(db, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        Self::check(
            &self.run(db, &["-N", "--empty-password"])?,
            "create the database",
        )
    }

    fn count(&self, db: &Path) -> Result<usize, String> {
        let output = self.run(db, &["-L"])?;
        Self::check(&output, "list the database")?;
        // A header of three lines (title, column names, blank), then one line per cert.
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(3)
            .filter(|l| !l.trim().is_empty())
            .count())
    }
}

/// Stands in for `certutil` where it isn't installed: every command fails and says what to
/// install, so journal entries can still be read (and undone once it is installed).
#[derive(Debug, Clone, Copy, Default)]
pub struct MissingCertutil;

/// What to install for `certutil`, per distribution family ([ADR 0021] §6).
///
/// [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md
pub const CERTUTIL_HINT: &str = "certutil isn't installed; install it with `sudo apt install libnss3-tools` (Debian/Ubuntu), `sudo dnf install nss-tools` (Fedora), `sudo zypper install mozilla-nss-tools` (openSUSE) or `sudo pacman -S nss` (Arch)";

impl NssTool for MissingCertutil {
    fn contains(&self, _db: &Path, _nickname: &str) -> Result<bool, String> {
        Err(CERTUTIL_HINT.into())
    }
    fn add(&self, _db: &Path, _nickname: &str, _pem: &str) -> Result<(), String> {
        Err(CERTUTIL_HINT.into())
    }
    fn delete(&self, _db: &Path, _nickname: &str) -> Result<(), String> {
        Err(CERTUTIL_HINT.into())
    }
    fn create(&self, _db: &Path) -> Result<(), String> {
        Err(CERTUTIL_HINT.into())
    }
    fn count(&self, _db: &Path) -> Result<usize, String> {
        Err(CERTUTIL_HINT.into())
    }
}

/// `certutil` if installed, else [`MissingCertutil`].
#[must_use]
pub fn nss_tool() -> Rc<dyn NssTool> {
    match Certutil::find() {
        Some(certutil) => Rc::new(certutil),
        None => Rc::new(MissingCertutil),
    }
}

/// The nickname Kiln gives the root with this SHA-256 (`AB:CD:…` or plain hex) in an NSS
/// database: unique per root, recognisable in a browser's certificate manager.
#[must_use]
pub fn nickname_for(sha256: &str) -> String {
    let hex = crate::trust_system::sha256_hex(sha256);
    format!(
        "Kiln Local Development CA {}",
        hex.get(..16).unwrap_or(&hex)
    )
}

/// Trusts a Kiln root in the NSS database at `db`.
#[derive(Debug, Clone)]
pub struct NssTrustOp {
    db: PathBuf,
    /// Which browsers use it, for previews: "Chrome", "Firefox (profile x)".
    label: String,
    pem: String,
    sha256: String,
    /// Create the database if it is missing (`~/.pki/nssdb` before Chrome first ran).
    create_db: bool,
    tool: Rc<dyn NssTool>,
}

impl NssTrustOp {
    #[must_use]
    pub fn new(
        db: PathBuf,
        label: impl Into<String>,
        pem: impl Into<String>,
        sha256: impl Into<String>,
        create_db: bool,
        tool: Rc<dyn NssTool>,
    ) -> Self {
        Self {
            db,
            label: label.into(),
            pem: pem.into(),
            sha256: sha256.into(),
            create_db,
            tool,
        }
    }

    pub fn from_params(params: &Value, tool: Rc<dyn NssTool>) -> Result<Self, OpError> {
        let text = |key: &str| {
            params[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| failed(format!("params.{key} missing")))
        };
        Ok(Self::new(
            PathBuf::from(text("db")?),
            text("label")?,
            text("pem")?,
            text("sha256")?,
            params["create_db"].as_bool().unwrap_or(false),
            tool,
        ))
    }

    /// The nickname Kiln uses for this root: unique per root, recognisable in a browser's
    /// certificate manager.
    #[must_use]
    pub fn nickname(&self) -> String {
        nickname_for(&self.sha256)
    }

    fn db_exists(&self) -> bool {
        self.db.join("cert9.db").is_file()
    }

    fn present(&self) -> Result<bool, OpError> {
        if !self.db_exists() {
            return Ok(false);
        }
        self.tool
            .contains(&self.db, &self.nickname())
            .map_err(failed)
    }
}

impl Operation for NssTrustOp {
    fn kind(&self) -> &'static str {
        NSS_TRUST_KIND
    }

    fn params(&self) -> Value {
        json!({
            "db": self.db.to_string_lossy(),
            "label": self.label,
            "pem": self.pem,
            "sha256": self.sha256,
            "create_db": self.create_db,
        })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let present = self.present()?;
        let summary = if present {
            format!("{} already trusts Kiln's CA", self.label)
        } else {
            format!(
                "Trust Kiln's local CA in {}; it can only vouch for development names",
                self.label
            )
        };
        Ok(Preview {
            summary,
            changes: vec![Change {
                target: format!("NSS database {}", self.db.display()),
                before: if present {
                    Some(self.nickname())
                } else if self.db_exists() {
                    None
                } else {
                    Some("(no database yet; Kiln creates it as Chrome would)".into())
                },
                after: Some(format!("{} (trusted for TLS servers)", self.nickname())),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        let db_existed = self.db_exists();
        if !db_existed && !self.create_db {
            return Err(failed(format!(
                "{} has no NSS database at {}",
                self.label,
                self.db.display()
            )));
        }
        Ok(Receipt(json!({
            "was_present": self.present()?,
            "db_existed": db_existed,
        })))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        if !self.db_exists() {
            self.tool.create(&self.db).map_err(failed)?;
        }
        if self.present()? {
            return Ok(());
        }
        self.tool
            .add(&self.db, &self.nickname(), &self.pem)
            .map_err(failed)
    }

    fn verify(&self) -> Result<(), OpError> {
        if self.present()? {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "Kiln's CA is not in {}",
                self.db.display()
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, _force: bool) -> Result<(), OpError> {
        if receipt.0["was_present"].as_bool().unwrap_or(true) {
            return Ok(()); // trusted before Kiln touched it: leave it
        }
        if self.present()? {
            self.tool
                .delete(&self.db, &self.nickname())
                .map_err(failed)?;
        }
        // A database Kiln created and nobody else used since goes again.
        let created = !receipt.0["db_existed"].as_bool().unwrap_or(true);
        if created && self.db_exists() && self.tool.count(&self.db).map_err(failed)? == 0 {
            for file in DB_FILES {
                let path = self.db.join(file);
                if path.exists() {
                    std::fs::remove_file(&path).map_err(failed)?;
                }
            }
            let _ = std::fs::remove_dir(&self.db);
        }
        Ok(())
    }
}

/// An NSS database a browser uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NssDb {
    pub path: PathBuf,
    /// For people: "Chrome and other Chromium browsers", "Firefox profile x".
    pub label: String,
    /// Missing, and to be created (`~/.pki/nssdb` with a Chromium browser installed).
    pub create: bool,
}

/// The NSS databases under `home` ([ADR 0021] §5). `~/.pki/nssdb` is listed even when
/// missing if `chromium_installed` (Chrome creates it on first start; Kiln can too).
///
/// [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md
#[must_use]
pub fn nss_databases(home: &Path, chromium_installed: bool) -> Vec<NssDb> {
    let mut out = Vec::new();
    let user_db = home.join(".pki").join("nssdb");
    let exists = user_db.join("cert9.db").is_file();
    if exists || chromium_installed {
        out.push(NssDb {
            path: user_db,
            label: "Chrome and other Chromium browsers".into(),
            create: !exists,
        });
    }
    let snap_chromium = home.join("snap/chromium/current/.pki/nssdb");
    if snap_chromium.join("cert9.db").is_file() {
        out.push(NssDb {
            path: snap_chromium,
            label: "Chromium (snap)".into(),
            create: false,
        });
    }
    for (base, kind) in [
        (home.join(".mozilla/firefox"), "Firefox"),
        (
            home.join("snap/firefox/common/.mozilla/firefox"),
            "Firefox (snap)",
        ),
        (
            home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
            "Firefox (Flatpak)",
        ),
    ] {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut profiles: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("cert9.db").is_file())
            .collect();
        profiles.sort();
        for profile in profiles {
            let name = profile
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(NssDb {
                label: format!("{kind} profile {name}"),
                path: profile,
                create: false,
            });
        }
    }
    out
}

/// Lets the engine rebuild [`NssTrustOp`]s from the journal.
pub fn register_nss_trust(registry: &mut Registry, tool: Rc<dyn NssTool>) {
    registry.register(NSS_TRUST_KIND, move |params| {
        Ok(Box::new(NssTrustOp::from_params(params, tool.clone())?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};

    /// NSS databases in memory: db path → nicknames. `create` also writes the db files, so
    /// the operation's existence checks see them.
    #[derive(Debug, Default)]
    struct FakeNss {
        dbs: RefCell<BTreeMap<PathBuf, BTreeSet<String>>>,
    }

    impl FakeNss {
        fn with_db(db: &Path, nicknames: &[&str]) -> Self {
            let fake = Self::default();
            fake.create(db).unwrap();
            for n in nicknames {
                fake.add(db, n, "pem").unwrap();
            }
            fake
        }
    }

    impl NssTool for FakeNss {
        fn contains(&self, db: &Path, nickname: &str) -> Result<bool, String> {
            Ok(self
                .dbs
                .borrow()
                .get(db)
                .is_some_and(|s| s.contains(nickname)))
        }
        fn add(&self, db: &Path, nickname: &str, _pem: &str) -> Result<(), String> {
            self.dbs
                .borrow_mut()
                .get_mut(db)
                .ok_or("no database")?
                .insert(nickname.into());
            Ok(())
        }
        fn delete(&self, db: &Path, nickname: &str) -> Result<(), String> {
            self.dbs
                .borrow_mut()
                .get_mut(db)
                .ok_or("no database")?
                .remove(nickname);
            Ok(())
        }
        fn create(&self, db: &Path) -> Result<(), String> {
            std::fs::create_dir_all(db).unwrap();
            for file in DB_FILES {
                std::fs::write(db.join(file), b"").unwrap();
            }
            self.dbs.borrow_mut().insert(db.to_owned(), BTreeSet::new());
            Ok(())
        }
        fn count(&self, db: &Path) -> Result<usize, String> {
            Ok(self.dbs.borrow().get(db).map_or(0, BTreeSet::len))
        }
    }

    fn op(db: &Path, create_db: bool, tool: Rc<dyn NssTool>) -> NssTrustOp {
        NssTrustOp::new(
            db.to_owned(),
            "Chrome",
            "-----BEGIN CERTIFICATE-----",
            "ab".repeat(32),
            create_db,
            tool,
        )
    }

    #[test]
    fn adds_the_root_and_undo_removes_only_what_kiln_added() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("nssdb");
        let fake = Rc::new(FakeNss::with_db(&db, &["Someone else's CA"]));
        let op = op(&db, false, fake.clone());
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        op.verify().unwrap();
        assert!(fake.contains(&db, &op.nickname()).unwrap());

        op.revert(&receipt, false).unwrap();
        assert!(!fake.contains(&db, &op.nickname()).unwrap());
        assert!(fake.contains(&db, "Someone else's CA").unwrap());
        assert!(db.join("cert9.db").is_file(), "a database Kiln found stays");
        op.revert(&receipt, false).unwrap(); // idempotent
    }

    #[test]
    fn a_root_already_trusted_is_left_on_undo() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("nssdb");
        let fake = Rc::new(FakeNss::default());
        fake.create(&db).unwrap();
        let op = op(&db, false, fake.clone());
        fake.add(&db, &op.nickname(), "pem").unwrap();
        let receipt = op.prepare().unwrap();
        assert!(op.preview().unwrap().summary.contains("already trusts"));
        op.apply(&receipt).unwrap();
        op.revert(&receipt, false).unwrap();
        assert!(fake.contains(&db, &op.nickname()).unwrap());
    }

    #[test]
    fn a_database_kiln_created_goes_again_unless_something_else_moved_in() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(".pki").join("nssdb");
        let fake = Rc::new(FakeNss::default());
        assert!(
            op(&db, false, fake.clone()).prepare().is_err(),
            "only created when asked"
        );
        let op1 = op(&db, true, fake.clone());
        let receipt = op1.prepare().unwrap();
        op1.apply(&receipt).unwrap();
        op1.revert(&receipt, false).unwrap();
        assert!(!db.exists(), "created by Kiln, empty again: removed");

        // Chrome ran in between and stored its own certificate: the database stays.
        op1.apply(&receipt).unwrap();
        fake.add(&db, "A client certificate", "pem").unwrap();
        op1.revert(&receipt, false).unwrap();
        assert!(db.join("cert9.db").is_file());
        assert!(fake.contains(&db, "A client certificate").unwrap());
    }

    #[test]
    fn databases_are_found_where_browsers_keep_them() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        assert_eq!(nss_databases(h, false), vec![]);
        let chrome = nss_databases(h, true);
        assert_eq!(chrome.len(), 1);
        assert!(chrome[0].create, "Chrome installed, never started");

        for profile in [
            ".mozilla/firefox/abc.default-release",
            "snap/firefox/common/.mozilla/firefox/51cgketh.default",
            "snap/firefox/common/.mozilla/firefox/Crash Reports",
        ] {
            std::fs::create_dir_all(h.join(profile)).unwrap();
        }
        std::fs::write(h.join(".mozilla/firefox/abc.default-release/cert9.db"), b"").unwrap();
        std::fs::write(
            h.join("snap/firefox/common/.mozilla/firefox/51cgketh.default/cert9.db"),
            b"",
        )
        .unwrap();
        std::fs::create_dir_all(h.join(".pki/nssdb")).unwrap();
        std::fs::write(h.join(".pki/nssdb/cert9.db"), b"").unwrap();

        let labels: Vec<String> = nss_databases(h, false)
            .into_iter()
            .map(|db| format!("{}{}", db.label, if db.create { " (create)" } else { "" }))
            .collect();
        assert_eq!(
            labels,
            [
                "Chrome and other Chromium browsers",
                "Firefox profile abc.default-release",
                "Firefox (snap) profile 51cgketh.default",
            ],
            "folders without cert9.db (Crash Reports) aren't profiles"
        );
    }

    #[test]
    fn params_round_trip_through_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let fake: Rc<dyn NssTool> = Rc::new(FakeNss::default());
        let original = op(&dir.path().join("p"), true, fake.clone());
        let rebuilt = NssTrustOp::from_params(&original.params(), fake).unwrap();
        assert_eq!(rebuilt.params(), original.params());
        assert_eq!(
            rebuilt.nickname(),
            "Kiln Local Development CA abababababababab"
        );
    }
}
