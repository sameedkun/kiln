use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Atomically replaces the existing file `target` with `replacement`, keeping `target`'s
/// security metadata.
///
/// - **Windows:** `ReplaceFileW`, which keeps the target's security descriptor (including
///   explicit ACEs), attributes and alternate data streams. A rename would drop explicit ACEs
///   (spike 02, I-25).
/// - **Unix:** copies the target's mode and owner onto `replacement`, then renames.
///   `SELinux` labels and other `xattrs` are not copied yet (P1-04).
///
/// Both files must be on the same volume, and `target` must exist: a missing target is an
/// `io::ErrorKind::NotFound` error, and creating files is the caller's decision. On success
/// `replacement` no longer exists. On failure `target` is unchanged and `replacement` is left
/// in place for the caller to clean up.
pub fn replace_file(target: &Path, replacement: &Path) -> io::Result<()> {
    if !target.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} does not exist", target.display()),
        ));
    }
    platform::replace_file(target, replacement)
}

/// Writes `contents` to a new temporary file next to `target`, flushes it to disk and swaps
/// it in with [`replace_file`]. Readers see either the old or the new contents, never a mix.
pub fn write_replacing(target: &Path, contents: &[u8]) -> io::Result<()> {
    let temp = temp_path_for(target)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        replace_file(target, &temp)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn temp_path_for(target: &Path) -> io::Result<PathBuf> {
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no file name"))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut temp_name = name.to_os_string();
    temp_name.push(format!(".kiln-{}-{nanos}.tmp", std::process::id()));
    Ok(target.with_file_name(temp_name))
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub(super) fn replace_file(target: &Path, replacement: &Path) -> io::Result<()> {
        let target = wide(target);
        let replacement = wide(replacement);
        // SAFETY: `target` and `replacement` are NUL-terminated UTF-16 buffers that outlive the
        // call; the backup name, exclusion and reserved arguments are documented as optional
        // and may be null; flags 0 requests default behaviour.
        let ok = unsafe {
            ReplaceFileW(
                target.as_ptr(),
                replacement.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{MetadataExt, chown};
    use std::path::Path;

    pub(super) fn replace_file(target: &Path, replacement: &Path) -> io::Result<()> {
        let meta = fs::metadata(target)?;
        fs::set_permissions(replacement, meta.permissions())?;
        // Changing the owner needs privileges; when we have them (the helper), keep the
        // original owner, otherwise the file already belongs to us.
        match chown(replacement, Some(meta.uid()), Some(meta.gid())) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {}
            Err(e) => return Err(e),
        }
        fs::rename(replacement, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_contents_and_consumes_the_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hosts");
        let replacement = dir.path().join("hosts.new");
        fs::write(&target, b"old").unwrap();
        fs::write(&replacement, b"new").unwrap();
        replace_file(&target, &replacement).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(!replacement.exists());
    }

    #[test]
    fn missing_target_is_not_found_and_nothing_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("missing");
        let replacement = dir.path().join("missing.new");
        fs::write(&replacement, b"new").unwrap();
        let err = replace_file(&target, &replacement).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(!target.exists());
        assert!(
            replacement.exists(),
            "caller keeps the replacement on failure"
        );
    }

    #[test]
    fn write_replacing_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hosts");
        fs::write(&target, b"old").unwrap();
        write_replacing(&target, b"new contents").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new contents");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("hosts")]);
    }

    #[test]
    fn write_replacing_cleans_up_when_the_target_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hosts");
        assert_eq!(
            write_replacing(&target, b"x").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn unix_keeps_the_target_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hosts");
        fs::write(&target, b"old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        write_replacing(&target, b"new").unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    /// Regression for I-25: an explicit deny ACE on the target survives replacement, while a
    /// plain rename (what other crates do) drops it.
    #[cfg(windows)]
    #[test]
    fn windows_keeps_explicit_aces_where_rename_drops_them() {
        use std::process::Command;

        fn deny_guests_write(path: &Path) {
            let status = Command::new("icacls")
                .arg(path)
                .args(["/deny", "*S-1-5-32-546:(W)"])
                .output()
                .unwrap();
            assert!(status.status.success(), "icacls failed: {status:?}");
        }
        fn has_guest_deny(path: &Path) -> bool {
            let out = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!("(Get-Acl -LiteralPath '{}').Sddl", path.display()),
                ])
                .output()
                .unwrap();
            let sddl = String::from_utf8_lossy(&out.stdout);
            sddl.contains("(D;") && sddl.contains(";;;BG)")
        }

        let dir = tempfile::tempdir().unwrap();

        let target = dir.path().join("kept");
        fs::write(&target, b"old").unwrap();
        deny_guests_write(&target);
        assert!(has_guest_deny(&target));
        write_replacing(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(
            has_guest_deny(&target),
            "ReplaceFileW must keep the explicit ACE"
        );

        let renamed = dir.path().join("dropped");
        fs::write(&renamed, b"old").unwrap();
        deny_guests_write(&renamed);
        let temp = dir.path().join("dropped.new");
        fs::write(&temp, b"new").unwrap();
        fs::rename(&temp, &renamed).unwrap();
        assert!(
            !has_guest_deny(&renamed),
            "rename is expected to drop it (I-25)"
        );
    }
}
