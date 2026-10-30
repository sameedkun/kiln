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
///   On Linux its extended attributes too, including the `SELinux` label (ADR 0020 §8).
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

/// Renames `from` to `to` (same volume), replacing `to` if it exists, for Kiln's *own* files.
///
/// Use this instead of `std::fs::rename`. On Windows, recent `std` renames with the POSIX
/// semantics of `FileRenameInfoEx`, which some machines reject inside user-profile folders
/// with "cannot move the file to a different disk drive" (os error 17) even for a rename
/// within one folder (I-44). This uses classic `MoveFileExW` (`REPLACE_EXISTING |
/// WRITE_THROUGH`), which works there. To replace a file whose ACL must be kept, use
/// [`replace_file`] instead.
pub fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
    platform::rename_replace(from, to)
}

/// Writes `contents` to a new temporary file next to `target`, flushes it to disk and swaps
/// it in with [`replace_file`]. Readers see either the old or the new contents, never a mix.
///
/// On Windows this can fail with "Access is denied" inside the user profile on some machines
/// (I-44); use it for system files such as `hosts`, and [`rename_replace`] for Kiln's own files.
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

    pub(super) fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
        use windows_sys::Win32::Foundation::{ERROR_NOT_SAME_DEVICE, ERROR_SHARING_VIOLATION};
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_COPY_ALLOWED, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from = wide(from);
        let to = wide(to);
        let attempt = |flags| {
            // SAFETY: both are NUL-terminated UTF-16 buffers that outlive the call.
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) } == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        };
        let base = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
        // I-67: a scanner or a starting PHP may hold `to` open for a moment without delete
        // sharing, which fails the move with "Access is denied" or a sharing violation. Wait
        // for it, up to about two seconds.
        let attempt = |flags| {
            let mut delay = std::time::Duration::from_millis(20);
            loop {
                match attempt(flags) {
                    Err(e)
                        if delay.as_millis() < 1000
                            && (e.kind() == io::ErrorKind::PermissionDenied
                                || e.raw_os_error()
                                    == i32::try_from(ERROR_SHARING_VIOLATION).ok()) =>
                    {
                        std::thread::sleep(delay);
                        delay *= 2;
                    }
                    other => return other,
                }
            }
        };
        match attempt(base) {
            // I-44: some machines report same-folder paths in the user profile as being on
            // different devices. Let Windows copy and delete instead; WRITE_THROUGH makes it
            // durable, but it is not atomic, so callers must tolerate an interrupted move.
            Err(e) if e.raw_os_error() == i32::try_from(ERROR_NOT_SAME_DEVICE).ok() => {
                attempt(base | MOVEFILE_COPY_ALLOWED)
            }
            other => other,
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{MetadataExt, chown};
    use std::path::Path;

    pub(super) fn rename_replace(from: &Path, to: &Path) -> io::Result<()> {
        #[allow(clippy::disallowed_methods)] // the Unix implementation of rename_replace
        fs::rename(from, to)
    }

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
        #[cfg(target_os = "linux")]
        crate::xattr::copy_all(target, replacement)?;
        #[allow(clippy::disallowed_methods)] // metadata copied above; this is the swap
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
    fn rename_replace_creates_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, b"one").unwrap();
        rename_replace(&a, &b).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"one");
        fs::write(&a, b"two").unwrap();
        rename_replace(&a, &b).unwrap();
        assert_eq!(fs::read(&b).unwrap(), b"two");
        assert!(!a.exists());
    }

    /// Regression for I-44: on some Windows machines `std::fs::rename` fails with os error 17
    /// inside the user profile (outside `%TEMP%`). Kiln's state lives there, so exercise both
    /// helpers in a real profile folder.
    #[cfg(windows)]
    #[test]
    fn renames_and_replacements_work_inside_the_user_profile() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let Some(base) = std::env::var_os("LOCALAPPDATA") else {
            return;
        };
        let dir = PathBuf::from(base).join(format!(
            "kiln-sys-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let result = std::panic::catch_unwind(|| {
            let (tmp, fin) = (dir.join("key.tmp"), dir.join("key.final"));
            fs::write(&tmp, b"k1").unwrap();
            rename_replace(&tmp, &fin).unwrap();
            fs::write(&tmp, b"k2").unwrap();
            rename_replace(&tmp, &fin).unwrap();
            assert_eq!(fs::read(&fin).unwrap(), b"k2");
            // Note: `write_replacing` (ReplaceFileW) is *not* exercised here. On the machine
            // that surfaced I-44 it fails with "Access is denied" inside the profile too; it is
            // only used for the system hosts file, where it was verified end to end (I-42).
        });
        let _ = fs::remove_dir_all(&dir);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
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

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_keeps_extended_attributes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("hosts");
        fs::write(&target, b"old").unwrap();
        if let Err(e) = crate::xattr::set(&target, b"user.kiln-test", b"label") {
            eprintln!("skipped: this file system has no user xattrs ({e})");
            return;
        }
        write_replacing(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(
            crate::xattr::get(&target, b"user.kiln-test").unwrap(),
            b"label"
        );
        assert!(
            crate::xattr::list(&target)
                .unwrap()
                .contains(&b"user.kiln-test".to_vec())
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
        /// The file's DACL as SDDL, via `icacls /save` (locale-independent, no PowerShell; the
        /// first CI run showed `Get-Acl` output can't be relied on in runners).
        fn sddl(path: &Path) -> String {
            let saved = path.with_extension("acl");
            let out = Command::new("icacls")
                .arg(path)
                .arg("/save")
                .arg(&saved)
                .output()
                .unwrap();
            assert!(out.status.success(), "icacls /save failed: {out:?}");
            // icacls writes UTF-16LE: "<name>\r\n<SDDL>\r\n".
            let bytes = fs::read(&saved).unwrap();
            fs::remove_file(&saved).unwrap();
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            String::from_utf16_lossy(&units)
        }
        fn has_guest_deny(path: &Path) -> bool {
            let sddl = sddl(path);
            // An explicit (non-inherited) deny ACE for BUILTIN\Guests: "(D;;<rights>;;;BG)".
            sddl.split('(')
                .any(|ace| ace.starts_with("D;;") && ace.trim_end().ends_with(";;;BG)"))
        }

        let dir = tempfile::tempdir().unwrap();

        let target = dir.path().join("kept");
        fs::write(&target, b"old").unwrap();
        deny_guests_write(&target);
        assert!(has_guest_deny(&target), "setup: {}", sddl(&target));
        write_replacing(&target, b"new").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(
            has_guest_deny(&target),
            "ReplaceFileW must keep the explicit ACE: {}",
            sddl(&target)
        );

        let renamed = dir.path().join("dropped");
        fs::write(&renamed, b"old").unwrap();
        deny_guests_write(&renamed);
        let temp = dir.path().join("dropped.new");
        fs::write(&temp, b"new").unwrap();
        #[allow(clippy::disallowed_methods)] // demonstrating what a plain rename does (I-25)
        fs::rename(&temp, &renamed).unwrap();
        assert!(
            !has_guest_deny(&renamed),
            "rename is expected to drop it (I-25)"
        );
    }
}
