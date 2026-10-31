//! Installing a service's pinned release ([ADR 0036] §3–6): the archive is checked against
//! its pinned SHA-256 again, and its executable is installed under the provider's
//! Kiln-specific name in the version's `bin` folder.
//!
//! [ADR 0036]: ../../../docs/adr/0036-service-framework.md

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{Release, ServiceProvider};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
    #[error("{archive} doesn't match its pinned SHA-256 (got {actual}, expected {expected})")]
    Unverified {
        archive: String,
        actual: String,
        expected: String,
    },
    #[error("{archive} has no {executable}: {detail}")]
    BadArchive {
        archive: String,
        executable: String,
        detail: String,
    },
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> InstallError {
    let context = context.into();
    move |source| InstallError::Io { context, source }
}

fn sha256_hex(data: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(data)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Checks `archive` against `release` and installs the executable it holds into `bin`.
/// Returns the installed executable.
pub fn install(
    provider: &dyn ServiceProvider,
    release: &Release,
    archive: &Path,
    bin: &Path,
) -> Result<PathBuf, InstallError> {
    let data = fs::read(archive).map_err(io_err(format!("cannot read {}", archive.display())))?;
    let actual = sha256_hex(&data);
    if actual != release.sha256 {
        return Err(InstallError::Unverified {
            archive: release.archive.to_owned(),
            actual,
            expected: release.sha256.to_owned(),
        });
    }
    let executable = read_file(release.archive, data, release.executable).map_err(|detail| {
        InstallError::BadArchive {
            archive: release.archive.to_owned(),
            executable: release.executable.to_owned(),
            detail,
        }
    })?;

    fs::create_dir_all(bin).map_err(io_err(format!("cannot create {}", bin.display())))?;
    let dest = provider.executable(bin);
    let temp = dest.with_extension("partial");
    fs::write(&temp, &executable).map_err(io_err(format!("cannot write {}", temp.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))
            .map_err(io_err(format!("cannot make {} executable", temp.display())))?;
    }
    kiln_sys::rename_replace(&temp, &dest)
        .map_err(io_err(format!("cannot install {}", dest.display())))?;
    Ok(dest)
}

/// The file at `inside` in a `.zip` or `.tar.gz` archive named `archive_name`.
fn read_file(archive_name: &str, data: Vec<u8>, inside: &str) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    if archive_name.ends_with(".tar.gz") {
        let gz = flate2::read::GzDecoder::new(io::Cursor::new(data));
        let mut tar = tar::Archive::new(gz);
        for entry in tar.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let is_it = entry.header().entry_type().is_file()
                && entry
                    .path()
                    .is_ok_and(|p| p == Path::new(inside) || p == Path::new(".").join(inside));
            if is_it {
                entry.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
                return Ok(buffer);
            }
        }
        return Err("not in the archive".into());
    }
    let mut zip = zip::ZipArchive::new(io::Cursor::new(data)).map_err(|e| e.to_string())?;
    let mut entry = zip.by_name(inside).map_err(|e| e.to_string())?;
    entry.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Write as _;

    use crate::{InstanceDirs, Launch, PortRole};

    struct Fake(&'static [Release]);

    impl ServiceProvider for Fake {
        fn kind(&self) -> &'static str {
            "fake"
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn license(&self) -> &'static str {
            "MIT"
        }
        fn ports(&self) -> &'static [PortRole] {
            &[]
        }
        fn releases(&self) -> &'static [Release] {
            self.0
        }
        fn executable_name(&self) -> &'static str {
            "kiln-fake"
        }
        fn init_data(&self, _: &InstanceDirs) -> io::Result<()> {
            Ok(())
        }
        fn launch(&self, _: &InstanceDirs, _: &BTreeMap<String, u16>) -> Launch {
            Launch::default()
        }
    }

    fn zip_with(name: &str, content: &[u8]) -> Vec<u8> {
        let mut out = io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content).unwrap();
        zip.finish().unwrap();
        out.into_inner()
    }

    fn release(sha256: &'static str) -> Release {
        Release {
            version: "1.0.0",
            os: "windows",
            arch: "x86_64",
            archive: "fake.zip",
            url: "https://example.invalid/fake.zip",
            sha256,
            executable: "fake.exe",
        }
    }

    #[test]
    fn installs_the_executable_under_its_kiln_name_only_if_the_archive_matches() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("fake.zip");
        fs::write(&archive, zip_with("fake.exe", b"binary")).unwrap();
        let digest: &'static str = Box::leak(sha256_hex(&fs::read(&archive).unwrap()).into());
        let good = release(digest);
        let provider = Fake(&[]);
        let bin = dir.path().join("bin").join("fake").join("1.0.0");

        let installed = install(&provider, &good, &archive, &bin).unwrap();
        assert_eq!(installed, provider.executable(&bin));
        assert!(installed.ends_with(format!("kiln-fake{}", crate::EXE_SUFFIX)));
        assert_eq!(fs::read(&installed).unwrap(), b"binary");

        let other = release("0000000000000000000000000000000000000000000000000000000000000000");
        let err = install(&provider, &other, &archive, &dir.path().join("other")).unwrap_err();
        assert!(matches!(err, InstallError::Unverified { .. }), "{err}");
        assert!(!dir.path().join("other").exists(), "nothing installed");

        let missing = Release {
            executable: "nope.exe",
            ..good
        };
        let err = install(&provider, &missing, &archive, &bin).unwrap_err();
        assert!(matches!(err, InstallError::BadArchive { .. }), "{err}");
    }
}
