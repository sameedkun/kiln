//! The hosts-file write the helper performs (ADR 0006, ADR 0009 §11).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use kiln_platform::{DnsEntry, hosts};

use crate::proto::ErrorCode;

/// Backoff for sharing violations: 50, 100, 200, 400, 800 ms.
const RETRY_DELAYS_MS: [u64; 5] = [50, 100, 200, 400, 800];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub created: bool,
    /// The file was removed because it would have been empty and the caller asked for that
    /// (undoing a `created`).
    pub deleted: bool,
    pub unchanged: bool,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
}

fn failure(code: ErrorCode, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
    }
}

/// Windows `ERROR_SHARING_VIOLATION` / `ERROR_LOCK_VIOLATION`.
fn is_busy(e: &io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33))
}

fn with_retry<T>(mut attempt: impl FnMut() -> io::Result<T>) -> Result<T, Failure> {
    let mut delays = RETRY_DELAYS_MS.iter();
    loop {
        match attempt() {
            Ok(v) => return Ok(v),
            Err(e) if is_busy(&e) => match delays.next() {
                Some(ms) => sleep(Duration::from_millis(*ms)),
                None => {
                    return Err(failure(
                        ErrorCode::HostsBusy,
                        format!("the hosts file stayed locked by another program: {e}"),
                    ));
                }
            },
            Err(e) => return Err(failure(ErrorCode::Io, e.to_string())),
        }
    }
}

fn len(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// Makes Kiln's block in the hosts file at `path` contain exactly `entries` (empty removes
/// it). Preserves every other byte, keeps the file's ACLs (`ReplaceFileW`), refuses a
/// read-only file, retries while another program holds it locked, and creates a missing
/// file (reported as `created`).
///
/// With `remove_file_if_empty`, a file that would end up with **zero bytes** is deleted
/// instead of left empty. That is how reverting a `created` restores "no hosts file"; a file
/// with any other content is never deleted.
pub fn set_block(
    path: &Path,
    entries: &[DnsEntry],
    remove_file_if_empty: bool,
) -> Result<Outcome, Failure> {
    let current = with_retry(|| match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    })?;

    let Some(current) = current else {
        if entries.is_empty() {
            return Ok(Outcome {
                created: false,
                deleted: false,
                unchanged: true,
                bytes_before: 0,
                bytes_after: 0,
            });
        }
        let updated = hosts::upsert_block(b"", entries)
            .map_err(|e| failure(ErrorCode::InvalidRequest, e.to_string()))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| failure(ErrorCode::Io, e.to_string()))?;
        file.write_all(&updated)
            .and_then(|()| file.sync_all())
            .map_err(|e| failure(ErrorCode::Io, e.to_string()))?;
        return Ok(Outcome {
            created: true,
            deleted: false,
            unchanged: false,
            bytes_before: 0,
            bytes_after: len(&updated),
        });
    };

    let updated = hosts::upsert_block(&current, entries).map_err(|e| {
        let code = if matches!(e, hosts::HostsError::NotLoopback { .. }) {
            ErrorCode::InvalidRequest
        } else {
            ErrorCode::HostsMalformed
        };
        failure(code, e.to_string())
    })?;
    let delete = remove_file_if_empty && updated.is_empty();
    if updated == current && !delete {
        return Ok(Outcome {
            created: false,
            deleted: false,
            unchanged: true,
            bytes_before: len(&current),
            bytes_after: len(&current),
        });
    }
    let readonly = fs::metadata(path)
        .map_err(|e| failure(ErrorCode::Io, e.to_string()))?
        .permissions()
        .readonly();
    if readonly {
        return Err(failure(
            ErrorCode::HostsReadOnly,
            "the hosts file is marked read-only; another tool or an administrator may have locked it on purpose",
        ));
    }
    if delete {
        with_retry(|| fs::remove_file(path))?;
    } else {
        with_retry(|| kiln_sys::write_replacing(path, &updated))?;
    }
    Ok(Outcome {
        created: false,
        deleted: delete,
        unchanged: false,
        bytes_before: len(&current),
        bytes_after: len(&updated),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::{HostName, Tld};

    fn entries(names: &[&str]) -> Vec<DnsEntry> {
        names
            .iter()
            .map(|n| DnsEntry {
                name: HostName::new(n, &Tld::default()).unwrap(),
                ip: "127.0.0.1".parse().unwrap(),
            })
            .collect()
    }

    const ORIGINAL: &[u8] = b"# hosts\r\n127.0.0.1 localhost\r\n";

    #[test]
    fn writes_then_removes_restoring_the_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        fs::write(&path, ORIGINAL).unwrap();

        let out = set_block(&path, &entries(&["blog.test"]), false).unwrap();
        assert!(!out.created && !out.unchanged);
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("127.0.0.1 blog.test\r\n")
        );

        assert!(
            set_block(&path, &entries(&["blog.test"]), false)
                .unwrap()
                .unchanged,
            "idempotent"
        );
        set_block(&path, &[], false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), ORIGINAL);
    }

    #[test]
    fn creates_a_missing_file_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        assert!(
            set_block(&path, &[], false).unwrap().unchanged,
            "nothing to remove, nothing created"
        );
        assert!(!path.exists());
        assert!(
            set_block(&path, &entries(&["a.test"]), false)
                .unwrap()
                .created
        );
        assert!(path.exists());
    }

    #[test]
    fn undoing_a_creation_deletes_only_an_otherwise_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        assert!(
            set_block(&path, &entries(&["a.test"]), false)
                .unwrap()
                .created
        );
        let out = set_block(&path, &[], true).unwrap();
        assert!(out.deleted);
        assert!(!path.exists(), "back to no hosts file");

        // Never deleted when anything else is in the file, even if asked.
        fs::write(&path, ORIGINAL).unwrap();
        set_block(&path, &entries(&["a.test"]), false).unwrap();
        assert!(!set_block(&path, &[], true).unwrap().deleted);
        assert_eq!(fs::read(&path).unwrap(), ORIGINAL);
    }

    #[test]
    fn refuses_a_read_only_file_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        fs::write(&path, ORIGINAL).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms.clone()).unwrap();

        let err = set_block(&path, &entries(&["a.test"]), false).unwrap_err();
        assert_eq!(err.code, ErrorCode::HostsReadOnly);
        assert_eq!(fs::read(&path).unwrap(), ORIGINAL);

        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&path, perms).unwrap();
    }

    #[test]
    fn ambiguous_markers_are_reported_not_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        fs::write(&path, format!("{}\n", hosts::END_MARKER)).unwrap();
        assert_eq!(
            set_block(&path, &entries(&["a.test"]), false)
                .unwrap_err()
                .code,
            ErrorCode::HostsMalformed
        );
    }

    /// Another program holding the file without sharing makes the helper wait and retry.
    #[cfg(windows)]
    #[test]
    fn waits_out_a_short_lock_and_reports_a_long_one() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        fs::write(&path, ORIGINAL).unwrap();

        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let release = std::thread::spawn(move || {
            sleep(Duration::from_millis(120));
            drop(lock);
        });
        assert!(
            !set_block(&path, &entries(&["a.test"]), false)
                .unwrap()
                .unchanged
        );
        release.join().unwrap();

        let _held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert_eq!(
            set_block(&path, &entries(&["b.test"]), false)
                .unwrap_err()
                .code,
            ErrorCode::HostsBusy
        );
    }
}
