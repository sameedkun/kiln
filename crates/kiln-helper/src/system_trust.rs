//! Adding Kiln's root to the Linux system store and removing it ([ADR 0021] §7). Runs as
//! root, so it trusts nothing from the client: the certificate must be a Kiln root limited
//! by critical name constraints to one development TLD, the file name comes from its own
//! SHA-256, and the folder and update tool are the distribution's, found by
//! `kiln_platform::trust_system::detect_anchors`.
//!
//! [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use kiln_core::Tld;
use kiln_platform::trust_system::{Anchors, anchor_file_name, sha256_hex};

use crate::hosts::Failure;
use crate::proto::ErrorCode;

fn failure(code: ErrorCode, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
    }
}

fn anchors(anchors: Option<&Anchors>) -> Result<&Anchors, Failure> {
    anchors.ok_or_else(|| {
        failure(
            ErrorCode::InvalidRequest,
            "this system has no trust-anchor folder Kiln knows",
        )
    })
}

/// Runs the distribution's update tool with a cleared environment.
fn rebuild(anchors: &Anchors) -> Result<(), Failure> {
    let output = Command::new(&anchors.update)
        .args(anchors.update_args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .output()
        .map_err(|e| {
            failure(
                ErrorCode::Io,
                format!("cannot run {}: {e}", anchors.update.display()),
            )
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure(
            ErrorCode::Io,
            format!(
                "{} failed: {}",
                anchors.update.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
    }
}

/// Checks `pem` is a Kiln root for an allowed TLD and returns its DER and SHA-256 hex.
fn validated(pem: &str) -> Result<(Vec<u8>, String), Failure> {
    let invalid = |e: &dyn std::fmt::Display| failure(ErrorCode::InvalidRequest, e.to_string());
    let der = kiln_pki::pem_to_der(pem).map_err(|e| invalid(&e))?;
    let tld = kiln_pki::kiln_root_tld(&der).map_err(|e| invalid(&e))?;
    Tld::new(&tld).map_err(|e| invalid(&e))?;
    let sha256 = sha256_hex(&kiln_pki::sha256_fingerprint(&der));
    Ok((der, sha256))
}

/// Writes the root into the anchor folder (as PEM, mode 0644) and rebuilds the store.
/// Returns the file and whether anything changed.
pub fn trust(anchors_found: Option<&Anchors>, pem: &str) -> Result<(PathBuf, bool), Failure> {
    let (der, sha256) = validated(pem)?;
    let anchors = anchors(anchors_found)?;
    let file = anchors.dir.join(anchor_file_name(&sha256));
    // Written from the parsed DER, not the client's text: nothing else can ride along.
    let canonical = pem_from_der(&der);
    if fs::read_to_string(&file).is_ok_and(|existing| existing == canonical) {
        return Ok((file, false));
    }
    let io = |e: std::io::Error| failure(ErrorCode::Io, format!("{}: {e}", file.display()));
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&file)
        .map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        out.set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(io)?;
    }
    out.write_all(canonical.as_bytes())
        .and_then(|()| out.sync_all())
        .map_err(io)?;
    drop(out);
    rebuild(anchors)?;
    Ok((file, true))
}

/// Removes Kiln's file for the root with `sha256` and rebuilds the store.
pub fn untrust(anchors_found: Option<&Anchors>, sha256: &str) -> Result<(PathBuf, bool), Failure> {
    let hex = sha256_hex(sha256);
    if hex.len() != 64 || hex.len() != sha256.len() {
        return Err(failure(
            ErrorCode::InvalidRequest,
            "the SHA-256 must be 64 hex digits",
        ));
    }
    let anchors = anchors(anchors_found)?;
    let file = anchors.dir.join(anchor_file_name(&hex));
    match fs::remove_file(&file) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((file, false)),
        Err(e) => {
            return Err(failure(ErrorCode::Io, format!("{}: {e}", file.display())));
        }
    }
    rebuild(anchors)?;
    Ok((file, true))
}

fn pem_from_der(der: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::with_capacity(der.len().div_ceil(3) * 4);
    for chunk in der.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                b64.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                b64.push('=');
            }
        }
    }
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).unwrap_or_default());
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A Debian-like root where the update tool is `true` (or `false`, to fail).
    fn system(root: &Path, update_ok: bool) -> Anchors {
        let dir = root.join("usr/local/share/ca-certificates");
        fs::create_dir_all(&dir).unwrap();
        Anchors {
            dir,
            update: PathBuf::from(if update_ok { "true" } else { "false" }),
            update_args: &[],
        }
    }

    fn kiln_root() -> String {
        kiln_pki::generate_root(&Tld::default(), "dev@box", time::OffsetDateTime::now_utc())
            .unwrap()
            .cert_pem
    }

    #[cfg(unix)]
    #[test]
    fn trusts_a_kiln_root_and_removes_it_again() {
        let root = tempfile::tempdir().unwrap();
        let anchors = system(root.path(), true);
        let pem = kiln_root();
        let (file, changed) = trust(Some(&anchors), &pem).unwrap();
        assert!(changed);
        assert_eq!(
            kiln_pki::pem_to_der(&fs::read_to_string(&file).unwrap()).unwrap(),
            kiln_pki::pem_to_der(&pem).unwrap(),
            "the written file is the same certificate"
        );
        assert!(!trust(Some(&anchors), &pem).unwrap().1, "idempotent");

        let sha = sha256_hex(&kiln_pki::sha256_fingerprint(
            &kiln_pki::pem_to_der(&pem).unwrap(),
        ));
        assert_eq!(untrust(Some(&anchors), &sha).unwrap(), (file.clone(), true));
        assert!(!file.exists());
        assert!(!untrust(Some(&anchors), &sha).unwrap().1);
    }

    #[test]
    fn refuses_anything_but_a_constrained_kiln_root() {
        let root = tempfile::tempdir().unwrap();
        let anchors = system(root.path(), true);
        for pem in [
            "not a certificate",
            "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
        ] {
            assert_eq!(
                trust(Some(&anchors), pem).unwrap_err().code,
                ErrorCode::InvalidRequest
            );
        }
        // A real CA for a public TLD would fail the TLD check; an unconstrained one the
        // name-constraints check (both covered in kiln-pki); nothing was written here.
        assert_eq!(fs::read_dir(&anchors.dir).unwrap().count(), 0);
        // Paths can't be smuggled through the fingerprint.
        assert_eq!(
            untrust(Some(&anchors), "../../etc/passwd")
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(trust(None, &kiln_root()).is_err(), "unknown distribution");
    }

    #[test]
    fn pem_round_trips_through_der() {
        let pem = kiln_root();
        let der = kiln_pki::pem_to_der(&pem).unwrap();
        assert_eq!(kiln_pki::pem_to_der(&pem_from_der(&der)).unwrap(), der);
    }
}
