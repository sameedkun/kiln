//! Windows crypto wrappers for the local CA ([ADR 0010]): DPAPI for the root key, and the
//! current user's certificate stores for trust.
//!
//! [ADR 0010]: ../../../docs/adr/0010-local-certificate-authority.md

use std::ffi::{OsStr, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_FIND_SHA1_HASH, CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_STORE_ADD_NEW,
    CERT_STORE_DELETE_FLAG, CERT_STORE_PROV_SYSTEM_W, CERT_SYSTEM_STORE_CURRENT_USER,
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CertAddCertificateContextToStore,
    CertCloseStore, CertCreateCertificateContext, CertDeleteCertificateFromStore,
    CertFindCertificateInStore, CertFreeCertificateContext, CertGetNameStringW, CertOpenStore,
    CryptProtectData, CryptUnprotectData, HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
};

/// `CRYPT_E_EXISTS`: the certificate is already in the store.
const CRYPT_E_EXISTS: i32 = 0x8009_2005_u32.cast_signed();

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn blob(bytes: &[u8]) -> io::Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "buffer too large"))?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

/// Copies a DPAPI output blob into a `Vec` and frees the OS allocation.
fn take_blob(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: on success DPAPI returns `cbData` initialised bytes at `pbData`.
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    // SAFETY: DPAPI allocated `pbData` with LocalAlloc; it is freed exactly once, here.
    unsafe { LocalFree(out.pbData.cast::<c_void>()) };
    bytes
}

/// Encrypts `data` so only the current Windows user on this machine can decrypt it.
/// `entropy` is an extra secret the caller must supply again to decrypt.
pub fn dpapi_protect(data: &[u8], entropy: &[u8]) -> io::Result<Vec<u8>> {
    let input = blob(data)?;
    let extra = blob(entropy)?;
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: input/entropy blobs point to live slices for the duration of the call (DPAPI
    // only reads them); the optional description, reserved and prompt arguments are null;
    // `out` is a valid out pointer.
    let ok = unsafe {
        CryptProtectData(
            &raw const input,
            std::ptr::null(),
            &raw const extra,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut out,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(take_blob(out))
}

/// Decrypts a [`dpapi_protect`] blob. Fails for another user, another machine, a modified blob,
/// or the wrong `entropy`.
pub fn dpapi_unprotect(protected: &[u8], entropy: &[u8]) -> io::Result<Vec<u8>> {
    let input = blob(protected)?;
    let extra = blob(entropy)?;
    let mut out = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: as in `dpapi_protect`; the description out-parameter is not requested (null).
    let ok = unsafe {
        CryptUnprotectData(
            &raw const input,
            std::ptr::null_mut(),
            &raw const extra,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &raw mut out,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(take_blob(out))
}

/// A certificate context owned by us; freed on drop.
struct Context(*const CERT_CONTEXT);

impl Drop for Context {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the context came from a Cert* call that transferred ownership to us and
            // has not been freed (deletion consumes it via `into_raw`).
            unsafe { CertFreeCertificateContext(self.0) };
        }
    }
}

impl Context {
    fn into_raw(mut self) -> *const CERT_CONTEXT {
        std::mem::replace(&mut self.0, std::ptr::null())
    }
}

const ENCODING: u32 = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;

/// One of the current user's system certificate stores (`Root`, `My`, or a private test
/// store), opened read/write. Changes to `Root` make Windows show a confirmation dialog.
#[derive(Debug)]
pub struct CertStore {
    handle: HCERTSTORE,
}

impl CertStore {
    /// Opens (creating if needed) the current user's system store `name`.
    pub fn open_current_user(name: &str) -> io::Result<Self> {
        let name_w = wide(name);
        // SAFETY: CERT_STORE_PROV_SYSTEM_W takes a NUL-terminated UTF-16 store name as `pvPara`,
        // which outlives the call.
        let handle = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                0,
                0,
                CERT_SYSTEM_STORE_CURRENT_USER,
                name_w.as_ptr().cast::<c_void>(),
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    /// Deletes the current user's system store `name` (for test stores; never `Root`).
    pub fn delete_current_user(name: &str) -> io::Result<()> {
        if name.eq_ignore_ascii_case("Root") || name.eq_ignore_ascii_case("My") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing to delete a standard store",
            ));
        }
        let name_w = wide(name);
        // SAFETY: as in `open_current_user`; with CERT_STORE_DELETE_FLAG no handle is returned.
        unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                0,
                0,
                CERT_SYSTEM_STORE_CURRENT_USER | CERT_STORE_DELETE_FLAG,
                name_w.as_ptr().cast::<c_void>(),
            )
        };
        Ok(())
    }

    fn find(&self, sha1: &[u8; 20]) -> io::Result<Option<Context>> {
        let hash = blob(sha1)?;
        // SAFETY: valid store handle; `hash` points to 20 live bytes; no previous context.
        let ctx = unsafe {
            CertFindCertificateInStore(
                self.handle,
                ENCODING,
                0,
                CERT_FIND_SHA1_HASH,
                (&raw const hash).cast::<c_void>(),
                std::ptr::null(),
            )
        };
        Ok((!ctx.is_null()).then(|| Context(ctx)))
    }

    /// Whether a certificate with this SHA-1 thumbprint is in the store.
    pub fn contains(&self, sha1: &[u8; 20]) -> io::Result<bool> {
        Ok(self.find(sha1)?.is_some())
    }

    /// Adds the DER certificate if missing. Refuses certificates whose subject display name
    /// does not contain `required_marker`, so this path can't be used to trust arbitrary
    /// certificates. Returns `false` if it was already present.
    pub fn add(&self, der: &[u8], required_marker: &str) -> io::Result<bool> {
        let len = u32::try_from(der.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "certificate too large"))?;
        // SAFETY: `der` is a live buffer of `len` bytes.
        let ctx = unsafe { CertCreateCertificateContext(ENCODING, der.as_ptr(), len) };
        if ctx.is_null() {
            return Err(io::Error::last_os_error());
        }
        let ctx = Context(ctx);
        let mut name = vec![0u16; 512];
        let capacity = u32::try_from(name.len()).unwrap_or(0);
        // SAFETY: valid context; `name` is a writable buffer of `capacity` units.
        let written = unsafe {
            CertGetNameStringW(
                ctx.0,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                std::ptr::null(),
                name.as_mut_ptr(),
                capacity,
            )
        };
        name.truncate((written as usize).saturating_sub(1));
        let subject = String::from_utf16_lossy(&name);
        if !subject.contains(required_marker) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("refusing to add {subject:?}: not a {required_marker}"),
            ));
        }
        // SAFETY: valid store and context; no output context requested.
        let ok = unsafe {
            CertAddCertificateContextToStore(
                self.handle,
                ctx.0,
                CERT_STORE_ADD_NEW,
                std::ptr::null_mut(),
            )
        };
        if ok != 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(CRYPT_E_EXISTS) {
            Ok(false)
        } else {
            Err(error)
        }
    }

    /// Removes the certificate with this thumbprint. Returns `false` if it was not present.
    pub fn remove(&self, sha1: &[u8; 20]) -> io::Result<bool> {
        let Some(ctx) = self.find(sha1)? else {
            return Ok(false);
        };
        // SAFETY: valid context found in this store; CertDeleteCertificateFromStore always
        // frees it, so ownership is released first.
        if unsafe { CertDeleteCertificateFromStore(ctx.into_raw()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(true)
    }
}

impl Drop for CertStore {
    fn drop(&mut self) {
        // SAFETY: the handle is open and owned by `self`; closed exactly once.
        unsafe { CertCloseStore(self.handle, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trips_and_detects_tampering_or_wrong_entropy() {
        let secret = b"-----BEGIN PRIVATE KEY----- not really";
        let blob = dpapi_protect(secret, b"kiln-ca").unwrap();
        assert!(
            !blob.windows(secret.len()).any(|w| w == secret),
            "ciphertext"
        );
        assert_eq!(dpapi_unprotect(&blob, b"kiln-ca").unwrap(), secret);
        assert!(dpapi_unprotect(&blob, b"other").is_err());
        let mut tampered = blob.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xFF;
        assert!(dpapi_unprotect(&tampered, b"kiln-ca").is_err());
    }

    #[test]
    fn standard_stores_cannot_be_deleted() {
        assert!(CertStore::delete_current_user("Root").is_err());
        assert!(CertStore::delete_current_user("my").is_err());
    }
}
