//! The Caddy binary Kiln runs: official release archives, pinned by SHA-512 ([ADR 0011] §9–10).
//!
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha512};

/// File name Kiln runs Caddy under, so tools that kill `caddy.exe` by image name leave it
/// alone ([I-11]).
///
/// [I-11]: ../../../docs/issues.md#i-11
pub const EXE_NAME: &str = if cfg!(windows) {
    "kiln-caddy.exe"
} else {
    "kiln-caddy"
};

/// One official Caddy release archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    pub version: &'static str,
    /// `std::env::consts::OS` / `ARCH` values it runs on.
    pub os: &'static str,
    pub arch: &'static str,
    /// File name on the GitHub release page.
    pub archive: &'static str,
    /// SHA-512 of the archive, lowercase hex, from Caddy's signed `checksums.txt`.
    pub sha512: &'static str,
    /// Path of the executable inside the archive.
    pub executable: &'static str,
}

impl Release {
    #[must_use]
    pub fn download_url(&self) -> String {
        format!(
            "https://github.com/caddyserver/caddy/releases/download/v{}/{}",
            self.version, self.archive
        )
    }
}

/// Pinned releases. Add new versions by copying the line for the archive from the release's
/// `caddy_<version>_checksums.txt` after checking its signature.
pub const CADDY_RELEASES: &[Release] = &[
    Release {
        version: "2.11.7",
        os: "windows",
        arch: "x86_64",
        archive: "caddy_2.11.7_windows_amd64.zip",
        sha512: "c308154504e53755958ffc62d8a2657ab17eefd6dd25c45dbb2c3d93d29519971f77ca652f7d0f2bcb7f980b9ba3822478865e29969d0272d7613550a43152b1",
        executable: "caddy.exe",
    },
    Release {
        version: "2.11.7",
        os: "windows",
        arch: "aarch64",
        archive: "caddy_2.11.7_windows_arm64.zip",
        sha512: "5e232f5fd68968c288034d98f8b4a6b16f246da6d5f1d3a8c7543b83327f1de0bf5d01d627768fbfd1dd254d7d53eb94589a40b126c29def10ac01365a3fe982",
        executable: "caddy.exe",
    },
];

/// The newest pinned release for this OS and CPU, if there is one.
#[must_use]
pub fn release_for_this_platform() -> Option<&'static Release> {
    CADDY_RELEASES
        .iter()
        .find(|r| r.os == std::env::consts::OS && r.arch == std::env::consts::ARCH)
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
    #[error(
        "{archive} is not a Caddy release Kiln knows (SHA-512 {actual}); expected one of: {expected}"
    )]
    Unverified {
        archive: PathBuf,
        actual: String,
        expected: String,
    },
    #[error("no pinned Caddy release for {os}/{arch} yet")]
    UnsupportedPlatform { os: String, arch: String },
    #[error("{archive} has no {executable}: {detail}")]
    BadArchive {
        archive: PathBuf,
        executable: &'static str,
        detail: String,
    },
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> InstallError {
    let context = context.into();
    move |source| InstallError::Io { context, source }
}

fn sha512_hex(data: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha512::digest(data)
        .iter()
        .fold(String::with_capacity(128), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Checks `archive` against the releases pinned for this platform and installs its Caddy
/// executable as `dest`.
pub fn install(archive: &Path, dest: &Path) -> Result<Release, InstallError> {
    let candidates: Vec<Release> = CADDY_RELEASES
        .iter()
        .filter(|r| r.os == std::env::consts::OS && r.arch == std::env::consts::ARCH)
        .copied()
        .collect();
    if candidates.is_empty() {
        return Err(InstallError::UnsupportedPlatform {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        });
    }
    install_from(archive, dest, &candidates)
}

/// [`install`] against an explicit list of acceptable releases.
pub fn install_from(
    archive: &Path,
    dest: &Path,
    candidates: &[Release],
) -> Result<Release, InstallError> {
    let data = fs::read(archive).map_err(io_err(format!("cannot read {}", archive.display())))?;
    let actual = sha512_hex(&data);
    let Some(release) = candidates.iter().find(|r| r.sha512 == actual).copied() else {
        return Err(InstallError::Unverified {
            archive: archive.to_owned(),
            actual,
            expected: candidates
                .iter()
                .map(|r| r.archive)
                .collect::<Vec<_>>()
                .join(", "),
        });
    };

    let bad = |detail: String| InstallError::BadArchive {
        archive: archive.to_owned(),
        executable: release.executable,
        detail,
    };
    let mut zip = zip::ZipArchive::new(io::Cursor::new(data)).map_err(|e| bad(e.to_string()))?;
    let mut entry = zip
        .by_name(release.executable)
        .map_err(|e| bad(e.to_string()))?;

    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(io_err(format!("cannot create {}", dir.display())))?;
    let temp = dest.with_extension("partial");
    let result = (|| {
        let mut out =
            File::create(&temp).map_err(io_err(format!("cannot create {}", temp.display())))?;
        let mut buffer = Vec::new();
        entry
            .read_to_end(&mut buffer)
            .map_err(|e| bad(e.to_string()))?;
        out.write_all(&buffer)
            .and_then(|()| out.sync_all())
            .map_err(io_err(format!("cannot write {}", temp.display())))?;
        drop(out);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))
                .map_err(io_err(format!("cannot mark {} executable", temp.display())))?;
        }
        kiln_sys::rename_replace(&temp, dest).map_err(io_err(format!(
            "cannot replace {} (stop the web server first if it is running)",
            dest.display()
        )))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map(|()| release)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn archive_with(dir: &Path, name: &str, contents: &[u8]) -> (PathBuf, String) {
        let path = dir.join("caddy.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("LICENSE", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"Apache-2.0").unwrap();
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(contents).unwrap();
        zip.finish().unwrap();
        let hash = sha512_hex(&fs::read(&path).unwrap());
        (path, hash)
    }

    fn pinned(hash: String) -> Release {
        Release {
            version: "0.0.1",
            os: "test",
            arch: "test",
            archive: "caddy_test.zip",
            sha512: Box::leak(hash.into_boxed_str()),
            executable: "caddy.exe",
        }
    }

    #[test]
    fn installs_the_executable_from_a_pinned_archive() {
        let dir = tempfile::tempdir().unwrap();
        let (archive, hash) = archive_with(dir.path(), "caddy.exe", b"MZ fake caddy");
        let dest = dir.path().join("bin").join(EXE_NAME);
        let release = install_from(&archive, &dest, &[pinned(hash.clone())]).unwrap();
        assert_eq!(release.version, "0.0.1");
        assert_eq!(fs::read(&dest).unwrap(), b"MZ fake caddy");
        // Reinstalling replaces it in place.
        install_from(&archive, &dest, &[pinned(hash)]).unwrap();
        assert!(!dest.with_extension("partial").exists());
    }

    #[test]
    fn refuses_archives_that_are_not_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let (archive, _) = archive_with(dir.path(), "caddy.exe", b"MZ tampered");
        let dest = dir.path().join(EXE_NAME);
        let err = install_from(&archive, &dest, &[pinned("00".repeat(64))]).unwrap_err();
        assert!(matches!(err, InstallError::Unverified { .. }), "{err}");
        assert!(!dest.exists());
    }

    #[test]
    fn a_pinned_archive_without_the_executable_installs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (archive, hash) = archive_with(dir.path(), "README.md", b"no binary here");
        let dest = dir.path().join(EXE_NAME);
        let err = install_from(&archive, &dest, &[pinned(hash)]).unwrap_err();
        assert!(matches!(err, InstallError::BadArchive { .. }), "{err}");
        assert!(!dest.exists());
    }

    #[test]
    fn pins_are_well_formed_and_point_at_github_releases() {
        for release in CADDY_RELEASES {
            assert_eq!(release.sha512.len(), 128, "{}", release.archive);
            assert!(
                release
                    .sha512
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
            assert!(release.archive.contains(release.version));
            assert!(
                release
                    .download_url()
                    .starts_with("https://github.com/caddyserver/caddy/releases/download/v")
            );
        }
    }
}
