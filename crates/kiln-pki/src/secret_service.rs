//! The root key in the desktop's Secret Service ([ADR 0021] §1–4): GNOME Keyring, `KWallet`,
//! `KeePassXC`, through `org.freedesktop.secrets` on the user's session bus.
//!
//! The key is the secret of one item in the default collection; `root.key.protected` holds
//! only a reference to it. Items use the `plain` session, as libsecret does on the user's
//! private bus. A locked collection is unlocked by the desktop's own dialog.
//!
//! [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md

use std::collections::HashMap;
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::store::KeyProtector;

const SERVICE: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const DEFAULT_COLLECTION: &str = "/org/freedesktop/secrets/aliases/default";
const LABEL: &str = "Kiln local CA root key";
/// What `root.key.protected` starts with when the key is in the Secret Service.
const REFERENCE: &[u8] = b"kiln-secret-service/1 ";
/// How long Kiln waits for the user to answer the desktop's unlock dialog.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("Secret Service: {e}"))
}

fn no_prompt(path: &ObjectPath<'_>) -> bool {
    path.as_str() == "/"
}

/// The key in the Secret Service when one is running, else (servers, sessions without a
/// desktop) in a `0600` file, as [`crate::store::PermissionsOnly`] keeps it. Both kinds of
/// `root.key.protected` can be read back, whichever way a key was stored.
#[derive(Debug)]
pub struct DesktopKeyring {
    use_service: bool,
}

impl DesktopKeyring {
    /// Checks once whether a Secret Service with a default collection answers, and whether
    /// it can be used now: unlocked, or locked with a desktop session to show the unlock
    /// dialog in. Over SSH to a desktop nobody is logged in to, the keyring is locked and
    /// nobody could answer, so the key goes to a `0600` file (and `kiln ca init` says so).
    #[must_use]
    pub fn detect() -> Self {
        let usable = Session::open().is_ok_and(|s| {
            s.has_default_collection() && (graphical_session() || !s.default_locked())
        });
        Self {
            use_service: usable,
        }
    }

    /// Whether `blob` is a reference to a Secret Service item.
    #[must_use]
    pub fn is_reference(blob: &[u8]) -> bool {
        blob.starts_with(REFERENCE)
    }
}

impl KeyProtector for DesktopKeyring {
    fn protect(&self, key: &[u8]) -> io::Result<Vec<u8>> {
        if !self.use_service {
            return Ok(key.to_vec());
        }
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|e| io::Error::other(e.to_string()))?;
        let id = id.iter().fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        });
        Session::open()?.store(&id, key)?;
        let mut blob = REFERENCE.to_vec();
        blob.extend_from_slice(id.as_bytes());
        blob.push(b'\n');
        Ok(blob)
    }

    fn unprotect(&self, blob: &[u8]) -> io::Result<Vec<u8>> {
        match reference_id(blob) {
            Some(id) => Session::open()?.load(id),
            None => Ok(blob.to_vec()),
        }
    }

    fn encrypted_at_rest(&self) -> bool {
        self.use_service
    }

    fn describe(&self, blob: &[u8]) -> &'static str {
        if Self::is_reference(blob) {
            "in your desktop keyring (Secret Service), encrypted with your login password"
        } else {
            "protected by file permissions only (there was no usable keyring when it was created)"
        }
    }

    fn forget(&self, blob: &[u8]) -> io::Result<()> {
        match reference_id(blob) {
            Some(id) => Session::open()?.delete(id),
            None => Ok(()),
        }
    }
}

fn reference_id(blob: &[u8]) -> Option<&str> {
    let rest = blob.strip_prefix(REFERENCE)?;
    let id = std::str::from_utf8(rest).ok()?.trim();
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// Whether this process runs in a desktop session that can show the keyring's dialog.
fn graphical_session() -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty()))
}

fn attributes(id: &str) -> HashMap<&str, &str> {
    HashMap::from([
        ("application", "kiln"),
        ("kiln-item", "ca-root-key"),
        ("kiln-id", id),
    ])
}

/// One connection and `plain` session with the Secret Service.
struct Session {
    conn: Connection,
    path: OwnedObjectPath,
}

impl Session {
    fn open() -> io::Result<Self> {
        let conn = Connection::session().map_err(err)?;
        let service = Proxy::new(
            &conn,
            SERVICE,
            SERVICE_PATH,
            "org.freedesktop.Secret.Service",
        )
        .map_err(err)?;
        let (_, path): (OwnedValue, OwnedObjectPath) = service
            .call("OpenSession", &("plain", Value::from("")))
            .map_err(err)?;
        Ok(Self { conn, path })
    }

    fn proxy<'a>(&'a self, path: &'a str, interface: &'a str) -> io::Result<Proxy<'a>> {
        Proxy::new(&self.conn, SERVICE, path, interface).map_err(err)
    }

    fn service(&self) -> io::Result<Proxy<'_>> {
        self.proxy(SERVICE_PATH, "org.freedesktop.Secret.Service")
    }

    fn has_default_collection(&self) -> bool {
        self.service()
            .and_then(|s| {
                s.call::<_, _, OwnedObjectPath>("ReadAlias", &("default",))
                    .map_err(err)
            })
            .is_ok_and(|path| !no_prompt(&path))
    }

    /// Shows the desktop's dialog for `prompt` (unless it is `/`, no dialog needed) and
    /// returns its result, or an error if the user dismissed it.
    fn complete(&self, prompt: &ObjectPath<'_>) -> io::Result<Option<OwnedValue>> {
        if no_prompt(prompt) {
            return Ok(None);
        }
        let proxy = self.proxy(prompt.as_str(), "org.freedesktop.Secret.Prompt")?;
        let mut completed = proxy.receive_signal("Completed").map_err(err)?;
        proxy.call::<_, _, ()>("Prompt", &("",)).map_err(err)?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(completed.next());
        });
        let message = rx
            .recv_timeout(PROMPT_TIMEOUT)
            .map_err(|_| err("the keyring's unlock dialog wasn't answered in time"))?
            .ok_or_else(|| err("the keyring closed its dialog"))?;
        let (dismissed, result): (bool, OwnedValue) = message.body().deserialize().map_err(err)?;
        if dismissed {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Secret Service: the keyring stayed locked (the dialog was dismissed)",
            ));
        }
        Ok(Some(result))
    }

    fn default_locked(&self) -> bool {
        self.proxy(DEFAULT_COLLECTION, "org.freedesktop.Secret.Collection")
            .and_then(|c| c.get_property::<bool>("Locked").map_err(err))
            .unwrap_or(true)
    }

    /// Unlocks a locked `object` (a collection or an item) through the desktop's dialog.
    fn unlock(&self, object: OwnedObjectPath) -> io::Result<()> {
        // The dialog shows on the desktop; without one, nobody could answer it.
        if !graphical_session() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Secret Service: the keyring is locked and there is no desktop session here to unlock it; run this from a terminal on the desktop, or log in to the desktop first",
            ));
        }
        let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) = self
            .service()?
            .call("Unlock", &(vec![object],))
            .map_err(err)?;
        self.complete(&prompt).map(|_| ())
    }

    fn store(&self, id: &str, key: &[u8]) -> io::Result<()> {
        if self.default_locked() {
            self.unlock(OwnedObjectPath::try_from(DEFAULT_COLLECTION).map_err(err)?)?;
        }
        let properties = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Value::from(LABEL)),
            (
                "org.freedesktop.Secret.Item.Attributes",
                Value::from(attributes(id)),
            ),
        ]);
        let secret: Secret = (
            self.path.clone(),
            Vec::new(),
            key.to_vec(),
            "application/octet-stream".to_owned(),
        );
        let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = self
            .proxy(DEFAULT_COLLECTION, "org.freedesktop.Secret.Collection")?
            .call("CreateItem", &(properties, secret, false))
            .map_err(err)?;
        self.complete(&prompt).map(|_| ())
    }

    /// The item for `id`, unlocked; `None` if there is none.
    fn find(&self, id: &str) -> io::Result<Option<OwnedObjectPath>> {
        let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) = self
            .service()?
            .call("SearchItems", &(attributes(id),))
            .map_err(err)?;
        if let Some(item) = unlocked.into_iter().next() {
            return Ok(Some(item));
        }
        let Some(item) = locked.into_iter().next() else {
            return Ok(None);
        };
        self.unlock(item.clone())?;
        Ok(Some(item))
    }

    fn load(&self, id: &str) -> io::Result<Vec<u8>> {
        let item = self.find(id)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Secret Service: Kiln's root key item is missing from the keyring",
            )
        })?;
        let (secret,): (Secret,) = self
            .proxy(item.as_str(), "org.freedesktop.Secret.Item")?
            .call("GetSecret", &(&self.path,))
            .map_err(err)?;
        Ok(secret.2)
    }

    fn delete(&self, id: &str) -> io::Result<()> {
        let Some(item) = self.find(id)? else {
            return Ok(());
        };
        let prompt: OwnedObjectPath = self
            .proxy(item.as_str(), "org.freedesktop.Secret.Item")?
            .call("Delete", &())
            .map_err(err)?;
        self.complete(&prompt).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_recognized_and_keys_are_not() {
        let blob = b"kiln-secret-service/1 00112233445566778899aabbccddeeff\n";
        assert!(DesktopKeyring::is_reference(blob));
        assert_eq!(reference_id(blob), Some("00112233445566778899aabbccddeeff"));
        assert_eq!(reference_id(b"kiln-secret-service/1 short\n"), None);
        // A key stored in a file (no Secret Service) is DER, never the reference prefix.
        assert!(!DesktopKeyring::is_reference(&[0x30, 0x81, 0x87, 0x02]));
        let file_only = DesktopKeyring { use_service: false };
        assert_eq!(
            file_only.unprotect(&[0x30, 0x01]).unwrap(),
            vec![0x30, 0x01]
        );
        assert_eq!(file_only.protect(&[0x30, 0x01]).unwrap(), vec![0x30, 0x01]);
    }

    /// Against the real desktop keyring; run by hand in the Linux test VM with a logged-in,
    /// unlocked desktop session:
    /// `KILN_TEST_SECRET_SERVICE=1 cargo test -p kiln-pki secret_service -- --ignored`
    #[test]
    #[ignore = "needs a desktop session's Secret Service"]
    fn stores_reads_and_forgets_a_key_in_the_keyring() {
        if std::env::var_os("KILN_TEST_SECRET_SERVICE").is_none() {
            return;
        }
        let keyring = DesktopKeyring::detect();
        assert!(keyring.encrypted_at_rest(), "no Secret Service found");
        let blob = keyring.protect(b"not really a key").unwrap();
        assert!(DesktopKeyring::is_reference(&blob));
        assert_eq!(keyring.unprotect(&blob).unwrap(), b"not really a key");
        keyring.forget(&blob).unwrap();
        assert_eq!(
            keyring.unprotect(&blob).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        keyring.forget(&blob).unwrap();
    }
}
