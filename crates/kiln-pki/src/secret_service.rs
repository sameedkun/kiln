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
use std::pin::Pin;
use std::sync::mpsc;
use std::time::Duration;

use futures_core::Stream;
use zbus::proxy::SignalStream;
use zbus::{Connection, Message, ObjectPath, OwnedObjectPath, OwnedVariant, Proxy, Variant};

use crate::store::KeyProtector;

const SERVICE: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const DEFAULT_COLLECTION: &str = "/org/freedesktop/secrets/aliases/default";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const PROMPT_IFACE: &str = "org.freedesktop.Secret.Prompt";
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

/// The next message of `stream`, or `None` when it ends.
async fn next(stream: &mut SignalStream<'_>) -> Option<Message> {
    std::future::poll_fn(|cx| Stream::poll_next(Pin::new(&mut *stream), cx)).await
}

/// One connection and `plain` session with the Secret Service. Every call is driven to
/// completion on the calling thread (`zbus::block_on`); Kiln runs no async runtime.
struct Session {
    conn: Connection,
    path: OwnedObjectPath,
}

impl Session {
    fn open() -> io::Result<Self> {
        zbus::block_on(async {
            let conn = Connection::session().await?;
            let (_, path): (OwnedVariant, OwnedObjectPath) =
                Proxy::new(&conn, SERVICE, SERVICE_PATH, SERVICE_IFACE)
                    .await?
                    .call("OpenSession", &("plain", Variant::from("")))
                    .await?;
            Ok::<_, zbus::Error>(Self { conn, path })
        })
        .map_err(err)
    }

    fn has_default_collection(&self) -> bool {
        zbus::block_on(async {
            Proxy::new(&self.conn, SERVICE, SERVICE_PATH, SERVICE_IFACE)
                .await?
                .call::<_, _, OwnedObjectPath>("ReadAlias", &("default",))
                .await
        })
        .is_ok_and(|path| !no_prompt(&path))
    }

    /// Shows the desktop's dialog for `prompt` (unless it is `/`, no dialog needed) and
    /// returns its result, or an error if the user dismissed it. The wait runs on its own
    /// thread so it can time out.
    fn complete(&self, prompt: &ObjectPath<'_>) -> io::Result<Option<OwnedVariant>> {
        if no_prompt(prompt) {
            return Ok(None);
        }
        let conn = self.conn.clone();
        let path = OwnedObjectPath::from(prompt.clone());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = zbus::block_on(async move {
                let proxy = Proxy::new(&conn, SERVICE, path.as_str(), PROMPT_IFACE).await?;
                let mut completed = proxy.receive_signal("Completed").await?;
                proxy.call::<_, _, ()>("Prompt", &("",)).await?;
                let message = next(&mut completed)
                    .await
                    .ok_or_else(|| zbus::Error::Failure("the keyring closed its dialog".into()))?;
                message.body().deserialize::<(bool, OwnedVariant)>()
            });
            let _ = tx.send(result);
        });
        let (dismissed, result) = rx
            .recv_timeout(PROMPT_TIMEOUT)
            .map_err(|_| err("the keyring's unlock dialog wasn't answered in time"))?
            .map_err(err)?;
        if dismissed {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Secret Service: the keyring stayed locked (the dialog was dismissed)",
            ));
        }
        Ok(Some(result))
    }

    fn default_locked(&self) -> bool {
        zbus::block_on(async {
            Proxy::new(&self.conn, SERVICE, DEFAULT_COLLECTION, COLLECTION_IFACE)
                .await?
                .get_property::<bool>("Locked")
                .await
        })
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
        let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) = zbus::block_on(async {
            Proxy::new(&self.conn, SERVICE, SERVICE_PATH, SERVICE_IFACE)
                .await?
                .call("Unlock", &(vec![object],))
                .await
        })
        .map_err(err)?;
        self.complete(&prompt).map(|_| ())
    }

    fn store(&self, id: &str, key: &[u8]) -> io::Result<()> {
        if self.default_locked() {
            self.unlock(OwnedObjectPath::try_from(DEFAULT_COLLECTION).map_err(err)?)?;
        }
        let properties = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Variant::from(LABEL)),
            (
                "org.freedesktop.Secret.Item.Attributes",
                Variant::from(attributes(id)),
            ),
        ]);
        let secret: Secret = (
            self.path.clone(),
            Vec::new(),
            key.to_vec(),
            "application/octet-stream".to_owned(),
        );
        let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = zbus::block_on(async {
            Proxy::new(&self.conn, SERVICE, DEFAULT_COLLECTION, COLLECTION_IFACE)
                .await?
                .call("CreateItem", &(properties, secret, false))
                .await
        })
        .map_err(err)?;
        self.complete(&prompt).map(|_| ())
    }

    /// The item for `id`, unlocked; `None` if there is none.
    fn find(&self, id: &str) -> io::Result<Option<OwnedObjectPath>> {
        let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            zbus::block_on(async {
                Proxy::new(&self.conn, SERVICE, SERVICE_PATH, SERVICE_IFACE)
                    .await?
                    .call("SearchItems", &(attributes(id),))
                    .await
            })
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
        let (secret,): (Secret,) = zbus::block_on(async {
            Proxy::new(&self.conn, SERVICE, item.as_str(), ITEM_IFACE)
                .await?
                .call("GetSecret", &(&self.path,))
                .await
        })
        .map_err(err)?;
        Ok(secret.2)
    }

    fn delete(&self, id: &str) -> io::Result<()> {
        let Some(item) = self.find(id)? else {
            return Ok(());
        };
        let prompt: OwnedObjectPath = zbus::block_on(async {
            Proxy::new(&self.conn, SERVICE, item.as_str(), ITEM_IFACE)
                .await?
                .call("Delete", &())
                .await
        })
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
