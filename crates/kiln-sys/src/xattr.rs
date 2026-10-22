//! Extended attributes on Linux ([ADR 0020] §8). Copying them onto a replacement keeps a
//! file's `SELinux` label (`security.selinux`), which matters for `/etc/hosts` on Fedora.
//!
//! Every call works on the path itself, never following a symlink at it (`l*xattr`).
//!
//! [ADR 0020]: ../../../docs/adr/0020-linux-dns-and-privilege.md

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
}

fn c_name(name: &[u8]) -> io::Result<CString> {
    CString::new(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "attribute name contains a NUL byte",
        )
    })
}

/// The file system doesn't support extended attributes at all.
fn unsupported(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::ENOTSUP)
}

/// A size returned by an `*xattr` call, or the error it reported.
fn checked(result: libc::ssize_t) -> io::Result<usize> {
    usize::try_from(result).map_err(|_| io::Error::last_os_error())
}

/// Names of `path`'s extended attributes; empty where the file system has none.
pub fn list(path: &Path) -> io::Result<Vec<Vec<u8>>> {
    let path = c_path(path)?;
    loop {
        // SAFETY: `path` is NUL-terminated; a null buffer of size 0 only asks for the size.
        let needed = unsafe { libc::llistxattr(path.as_ptr(), std::ptr::null_mut(), 0) };
        let needed = match checked(needed) {
            Ok(0) => return Ok(Vec::new()),
            Ok(n) => n,
            Err(e) if unsupported(&e) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut buf = vec![0u8; needed];
        // SAFETY: `buf` is writable for `buf.len()` bytes and outlives the call.
        let got = unsafe { libc::llistxattr(path.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        match checked(got) {
            Ok(n) => {
                buf.truncate(n);
                return Ok(buf
                    .split(|&b| b == 0)
                    .filter(|name| !name.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect());
            }
            // An attribute was added between the two calls; ask again.
            Err(e) if e.raw_os_error() == Some(libc::ERANGE) => {}
            Err(e) => return Err(e),
        }
    }
}

/// The value of attribute `name` on `path`.
pub fn get(path: &Path, name: &[u8]) -> io::Result<Vec<u8>> {
    let path = c_path(path)?;
    let name = c_name(name)?;
    loop {
        // SAFETY: both strings are NUL-terminated; a null buffer of size 0 only asks the size.
        let needed =
            unsafe { libc::lgetxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) };
        let mut buf = vec![0u8; checked(needed)?];
        // SAFETY: `buf` is writable for `buf.len()` bytes and outlives the call.
        let got = unsafe {
            libc::lgetxattr(
                path.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
            )
        };
        match checked(got) {
            Ok(n) => {
                buf.truncate(n);
                return Ok(buf);
            }
            Err(e) if e.raw_os_error() == Some(libc::ERANGE) => {}
            Err(e) => return Err(e),
        }
    }
}

/// Sets attribute `name` on `path` to `value`, creating or replacing it.
pub fn set(path: &Path, name: &[u8], value: &[u8]) -> io::Result<()> {
    let path = c_path(path)?;
    let name = c_name(name)?;
    // SAFETY: both strings are NUL-terminated and `value` is readable for its length.
    let result = unsafe {
        libc::lsetxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Copies every extended attribute of `from` onto `to`. An attribute this process may not
/// set (a `security.*` or `trusted.*` one without root) is skipped: then the file belongs to
/// this user and needs no system label.
pub fn copy_all(from: &Path, to: &Path) -> io::Result<()> {
    for name in list(from)? {
        let value = get(from, &name)?;
        match set(to, &name, &value) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied || unsupported(&e) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
