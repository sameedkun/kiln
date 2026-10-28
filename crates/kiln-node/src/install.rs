//! Installing a Node release into Kiln's folder ([ADR 0028] §2–3): signed checksums, a verified
//! download, extraction that can't escape its folder, and a check that `node` starts.
//!
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use kiln_fetch::{Expected, FetchError, Fetcher};

use crate::index::{NodeRelease, Platform, checksum_for};
use crate::pgp::{self, PgpError};
use crate::spec::{Version, parse_version};

/// Present only while an install is in progress; a folder holding it isn't a usable Node.
pub const INCOMPLETE_MARKER: &str = ".kiln-install-incomplete";
/// What Kiln records about an install, beside it.
pub const INFO_FILE: &str = "kiln-node.json";
/// Upper bound for one extracted release (a real one is about 200 MB).
pub const MAX_EXTRACTED: u64 = 1024 * 1024 * 1024;
const SHASUMS_LIMIT: u64 = 256 * 1024;
const SIGNATURE_LIMIT: u64 = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error("{url}: {source}")]
    Signature { url: String, source: PgpError },
    #[error("{0} isn't listed in its release's signed SHASUMS256.txt")]
    NotListed(String),
    #[error("{archive}: {detail}")]
    Archive { archive: PathBuf, detail: String },
    #[error("Node {version} failed its start-up check: {detail}")]
    Check { version: String, detail: String },
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> InstallError {
    let context = context.into();
    move |source| InstallError::Io { context, source }
}

/// A Node in Kiln's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledNode {
    pub version: String,
    pub numbers: Version,
    /// The LTS codename (lower case), if it is an LTS release.
    pub lts: Option<String>,
    pub dir: PathBuf,
}

impl InstalledNode {
    /// The folder holding `node` (and, on Windows, `npm.cmd` and the rest).
    #[must_use]
    pub fn bin_dir(&self) -> PathBuf {
        if cfg!(windows) {
            self.dir.clone()
        } else {
            self.dir.join("bin")
        }
    }

    #[must_use]
    pub fn node_exe(&self) -> PathBuf {
        self.bin_dir()
            .join(format!("node{}", std::env::consts::EXE_SUFFIX))
    }

    /// The folder holding the bundled packages (npm, corepack).
    #[must_use]
    pub fn modules_dir(&self) -> PathBuf {
        if cfg!(windows) {
            self.dir.join("node_modules")
        } else {
            self.dir.join("lib").join("node_modules")
        }
    }
}

/// The result of [`install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub node: InstalledNode,
    /// The key that signed the checksums (`None` if it was already installed).
    pub signer: Option<String>,
    pub already: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Info {
    version: String,
    lts: Option<String>,
}

/// Every complete install under `root`, oldest first.
#[must_use]
pub fn installed(root: &Path) -> Vec<InstalledNode> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<InstalledNode> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| read_installed(&entry.path()))
        .collect();
    found.sort_by_key(|n| n.numbers);
    found
}

/// The install in `dir`, if it is complete.
#[must_use]
pub fn read_installed(dir: &Path) -> Option<InstalledNode> {
    if dir.join(INCOMPLETE_MARKER).exists() {
        return None;
    }
    let info: Info = serde_json::from_slice(&fs::read(dir.join(INFO_FILE)).ok()?).ok()?;
    let numbers = parse_version(&info.version)?;
    let node = InstalledNode {
        version: info.version,
        numbers,
        lts: info.lts,
        dir: dir.to_owned(),
    };
    node.node_exe().is_file().then_some(node)
}

/// `node --version`'s check, replaceable in tests.
pub type Check = dyn Fn(&Path, &str) -> Result<(), InstallError>;

/// Checks the signed checksums, downloads (or reuses the verified cache), extracts and checks
/// `release` into `<root>/<version>`.
pub fn install(
    fetcher: &Fetcher,
    release: &NodeRelease,
    platform: Platform,
    root: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    check: &Check,
) -> Result<Installed, InstallError> {
    let dir = root.join(&release.version);
    if let Some(node) = read_installed(&dir) {
        return Ok(Installed {
            node,
            signer: None,
            already: true,
        });
    }
    let archive_name = platform.archive(&release.version);
    let (expected, signer) = signed_checksum(fetcher, &release.folder_url(), &archive_name)?;
    let url = format!("{}{archive_name}", release.folder_url());
    let archive = fetcher.fetch(&url, &archive_name, &expected, progress)?;

    remove_dir_if_present(&dir)?;
    fs::create_dir_all(&dir).map_err(io_err(format!("cannot create {}", dir.display())))?;
    let marker = dir.join(INCOMPLETE_MARKER);
    fs::write(&marker, b"kiln: install in progress\n")
        .map_err(io_err(format!("cannot write {}", marker.display())))?;
    let result = (|| {
        let top = platform.top_folder(&release.version);
        if platform.extension == "zip" {
            extract_zip(&archive, &top, &dir)?;
        } else {
            extract_tar_gz(&archive, &top, &dir)?;
        }
        let info = Info {
            version: release.version.clone(),
            lts: release.lts.clone(),
        };
        let json = serde_json::to_vec_pretty(&info).map_err(|e| InstallError::Io {
            context: "cannot write kiln-node.json".into(),
            source: io::Error::other(e),
        })?;
        fs::write(dir.join(INFO_FILE), json).map_err(io_err("cannot write kiln-node.json"))?;
        let node = InstalledNode {
            version: release.version.clone(),
            numbers: release.numbers,
            lts: release.lts.clone(),
            dir: dir.clone(),
        };
        check(&node.node_exe(), &release.version)?;
        fs::remove_file(&marker).map_err(io_err(format!("cannot remove {}", marker.display())))?;
        Ok(node)
    })();
    match result {
        Ok(node) => Ok(Installed {
            node,
            signer: Some(signer),
            already: false,
        }),
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// `archive`'s SHA-256 from the release's `SHASUMS256.txt`, once its signature checks out
/// against Node's release keys. Also returns the signing key's fingerprint.
pub fn signed_checksum(
    fetcher: &Fetcher,
    folder_url: &str,
    archive: &str,
) -> Result<(Expected, String), InstallError> {
    let sums_url = format!("{folder_url}SHASUMS256.txt");
    let sig_url = format!("{sums_url}.sig");
    let sums = fetcher.get_bytes(&sums_url, SHASUMS_LIMIT)?;
    let sig = fetcher.get_bytes(&sig_url, SIGNATURE_LIMIT)?;
    let signature_err = |source| InstallError::Signature {
        url: sig_url.clone(),
        source,
    };
    let keys = crate::keys::release_keys().map_err(signature_err)?;
    let signer = pgp::verify_detached(&keys, &sums, &sig).map_err(signature_err)?;
    // Only now is the file's content worth reading.
    let text = String::from_utf8_lossy(&sums);
    let hash =
        checksum_for(&text, archive).ok_or_else(|| InstallError::NotListed(archive.into()))?;
    let expected = Expected::sha256(hash)?;
    Ok((expected, pgp::hex(&signer)))
}

fn remove_dir_if_present(dir: &Path) -> Result<(), InstallError> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(io_err(format!("cannot remove {}", dir.display()))(e))
        }
        _ => Ok(()),
    }
}

/// `path` inside the archive, without its top folder `top`, as a safe relative path. `None`
/// for the top folder itself.
fn inner_path(path: &Path, top: &str) -> Result<Option<PathBuf>, String> {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(first)) if first == top => {}
        _ => return Err(format!("{} isn't inside {top}/", path.display())),
    }
    let mut inner = PathBuf::new();
    for component in components {
        match component {
            Component::Normal(part) => inner.push(part),
            Component::CurDir => {}
            _ => return Err(format!("unsafe path {}", path.display())),
        }
    }
    Ok((!inner.as_os_str().is_empty()).then_some(inner))
}

/// Copies one file's bytes, keeping the total under [`MAX_EXTRACTED`].
fn write_capped(
    reader: &mut dyn Read,
    target: &Path,
    total: &mut u64,
    bad: &dyn Fn(String) -> InstallError,
) -> Result<(), InstallError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(io_err(format!("cannot create {}", parent.display())))?;
    }
    let mut out =
        File::create(target).map_err(io_err(format!("cannot create {}", target.display())))?;
    let budget = MAX_EXTRACTED - *total;
    let written = io::copy(&mut reader.take(budget + 1), &mut out)
        .map_err(|e| bad(format!("{}: {e}", target.display())))?;
    if written > budget {
        return Err(bad(format!("expands to more than {MAX_EXTRACTED} bytes")));
    }
    *total += written;
    Ok(())
}

/// Extracts a Windows `.zip` whose entries all sit under `top/`, without that folder.
pub fn extract_zip(archive: &Path, top: &str, dest: &Path) -> Result<(), InstallError> {
    let bad = |detail: String| InstallError::Archive {
        archive: archive.to_owned(),
        detail,
    };
    let file = File::open(archive).map_err(io_err(format!("cannot open {}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(e.to_string()))?;
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| bad(e.to_string()))?;
        let Some(name) = entry.enclosed_name() else {
            return Err(bad(format!("unsafe path {:?}", entry.name())));
        };
        if entry.is_symlink() {
            return Err(bad(format!("symbolic link {:?}", entry.name())));
        }
        let Some(relative) = inner_path(&name, top).map_err(bad)? else {
            continue;
        };
        let target = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(io_err(format!("cannot create {}", target.display())))?;
        } else {
            write_capped(&mut entry, &target, &mut total, &bad)?;
        }
    }
    Ok(())
}

/// Extracts a Linux `.tar.gz` whose entries all sit under `top/`, without that folder. Relative
/// symbolic links that stay inside are recreated; any other link is refused.
pub fn extract_tar_gz(archive: &Path, top: &str, dest: &Path) -> Result<(), InstallError> {
    let bad = |detail: String| InstallError::Archive {
        archive: archive.to_owned(),
        detail,
    };
    let file = File::open(archive).map_err(io_err(format!("cannot open {}", archive.display())))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut total = 0u64;
    for entry in tar.entries().map_err(|e| bad(e.to_string()))? {
        let mut entry = entry.map_err(|e| bad(e.to_string()))?;
        let name = entry.path().map_err(|e| bad(e.to_string()))?.into_owned();
        let Some(relative) = inner_path(&name, top).map_err(bad)? else {
            continue;
        };
        let target = dest.join(&relative);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target)
                .map_err(io_err(format!("cannot create {}", target.display())))?;
        } else if kind.is_file() {
            let mode = entry.header().mode().unwrap_or(0o644);
            write_capped(&mut entry, &target, &mut total, &bad)?;
            set_mode(&target, mode)?;
        } else if kind.is_symlink() {
            let link = entry
                .link_name()
                .map_err(|e| bad(e.to_string()))?
                .ok_or_else(|| bad(format!("{}: a link without a target", name.display())))?
                .into_owned();
            if !stays_inside(&relative, &link) {
                return Err(bad(format!(
                    "{} links outside the archive ({})",
                    name.display(),
                    link.display()
                )));
            }
            make_symlink(&link, &target).map_err(bad)?;
        } else {
            return Err(bad(format!("{}: unexpected entry type", name.display())));
        }
    }
    Ok(())
}

/// Whether a link at `relative` (inside the folder) pointing to `link` resolves inside the
/// folder, judged on the names alone.
fn stays_inside(relative: &Path, link: &Path) -> bool {
    let mut depth: Vec<&std::ffi::OsStr> = Vec::new();
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        match component {
            Component::Normal(part) => depth.push(part),
            _ => return false,
        }
    }
    for component in link.components() {
        match component {
            Component::Normal(part) => depth.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if depth.pop().is_none() {
                    return false;
                }
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), InstallError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o755)).map_err(io_err(format!(
        "cannot set permissions on {}",
        path.display()
    )))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // the same signature as on Unix
fn set_mode(_path: &Path, _mode: u32) -> Result<(), InstallError> {
    Ok(())
}

#[cfg(unix)]
fn make_symlink(link: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::os::unix::fs::symlink(link, target)
        .map_err(|e| format!("cannot create link {}: {e}", target.display()))
}

#[cfg(not(unix))]
fn make_symlink(_link: &Path, target: &Path) -> Result<(), String> {
    Err(format!(
        "{}: symbolic links are only expected in the Linux archives",
        target.display()
    ))
}

/// Runs `node --version` and requires `v<version>`.
pub fn check_runs(node: &Path, version: &str) -> Result<(), InstallError> {
    let output = Command::new(node)
        .arg("--version")
        .output()
        .map_err(io_err(format!("cannot run {}", node.display())))?;
    let failed = |detail: String| InstallError::Check {
        version: version.to_owned(),
        detail,
    };
    let text = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(failed(format!(
            "exit code {:?}: {}{}",
            output.status.code(),
            text.trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if text.trim() != format!("v{version}") {
        return Err(failed(format!("it reports {}", text.trim())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{LINUX_X64, WIN_X64};
    use std::io::Write;

    fn release(version: &str) -> NodeRelease {
        NodeRelease {
            version: version.into(),
            numbers: parse_version(version).unwrap(),
            lts: Some("jod".into()),
            date: String::new(),
            files: vec![],
        }
    }

    #[allow(clippy::unnecessary_wraps)] // must match the check signature
    fn ok_check(_: &Path, _: &str) -> Result<(), InstallError> {
        Ok(())
    }

    fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
        use zip::write::SimpleFileOptions;
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                zip.start_file(*name, SimpleFileOptions::default()).unwrap();
                zip.write_all(data).unwrap();
            }
        }
        zip.finish().unwrap();
    }

    /// A `.tar.gz` of `(path, contents or link target, is_link)`.
    fn tar_gz_with(path: &Path, entries: &[(&str, &str, bool)]) {
        let gz =
            flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        for (name, data, is_link) in entries {
            let mut header = tar::Header::new_gnu();
            if *is_link {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_mode(0o777);
                tar.append_link(&mut header, name, data).unwrap();
            } else {
                header.set_size(data.len() as u64);
                header.set_mode(0o755);
                tar.append_data(&mut header, name, data.as_bytes()).unwrap();
            }
        }
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn zip_extraction_drops_the_top_folder_and_refuses_strangers() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("n.zip");
        let top = WIN_X64.top_folder("22.23.4");
        zip_with(
            &archive,
            &[
                (&format!("{top}/"), b""),
                (&format!("{top}/node.exe"), b"MZ"),
                (&format!("{top}/node_modules/npm/bin/npm-cli.js"), b"js"),
            ],
        );
        let out = dir.path().join("out");
        extract_zip(&archive, &top, &out).unwrap();
        assert_eq!(fs::read(out.join("node.exe")).unwrap(), b"MZ");
        assert!(out.join("node_modules/npm/bin/npm-cli.js").is_file());

        let other = dir.path().join("other.zip");
        zip_with(&other, &[("elsewhere/node.exe", b"MZ")]);
        assert!(matches!(
            extract_zip(&other, &top, &dir.path().join("o2")),
            Err(InstallError::Archive { .. })
        ));
    }

    #[test]
    fn tar_links_must_stay_inside() {
        assert!(stays_inside(
            Path::new("bin/npm"),
            Path::new("../lib/node_modules/npm/bin/npm-cli.js")
        ));
        assert!(!stays_inside(
            Path::new("bin/npm"),
            Path::new("../../etc/passwd")
        ));
        assert!(!stays_inside(
            Path::new("bin/npm"),
            Path::new("/usr/bin/npm")
        ));
        assert!(stays_inside(Path::new("npm"), Path::new("lib/npm")));
        assert!(!stays_inside(Path::new("npm"), Path::new("../npm")));
    }

    #[test]
    fn tar_extraction_keeps_modes_and_refuses_escaping_links() {
        let dir = tempfile::tempdir().unwrap();
        let top = LINUX_X64.top_folder("22.23.4");
        let archive = dir.path().join("n.tar.gz");
        tar_gz_with(
            &archive,
            &[
                (&format!("{top}/bin/node"), "ELF", false),
                (
                    &format!("{top}/lib/node_modules/npm/bin/npm-cli.js"),
                    "js",
                    false,
                ),
            ],
        );
        let out = dir.path().join("out");
        extract_tar_gz(&archive, &top, &out).unwrap();
        assert_eq!(fs::read(out.join("bin/node")).unwrap(), b"ELF");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(out.join("bin/node"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }

        let evil = dir.path().join("evil.tar.gz");
        tar_gz_with(
            &evil,
            &[(&format!("{top}/bin/npm"), "../../../../etc/passwd", true)],
        );
        let err = extract_tar_gz(&evil, &top, &dir.path().join("o2")).unwrap_err();
        assert!(err.to_string().contains("links outside"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn tar_extraction_recreates_links_inside() {
        let dir = tempfile::tempdir().unwrap();
        let top = LINUX_X64.top_folder("22.23.4");
        let archive = dir.path().join("n.tar.gz");
        tar_gz_with(
            &archive,
            &[
                (
                    &format!("{top}/lib/node_modules/npm/bin/npm-cli.js"),
                    "js",
                    false,
                ),
                (
                    &format!("{top}/bin/npm"),
                    "../lib/node_modules/npm/bin/npm-cli.js",
                    true,
                ),
            ],
        );
        let out = dir.path().join("out");
        extract_tar_gz(&archive, &top, &out).unwrap();
        assert_eq!(fs::read(out.join("bin/npm")).unwrap(), b"js");
    }

    #[test]
    fn installs_from_the_verified_cache_and_lists_it() {
        let dir = tempfile::tempdir().unwrap();
        let platform = if cfg!(windows) { WIN_X64 } else { LINUX_X64 };
        let version = "22.23.4";
        let top = platform.top_folder(version);
        let cache = dir.path().join("downloads");
        fs::create_dir_all(&cache).unwrap();
        let archive = cache.join(platform.archive(version));
        let node = format!(
            "{top}/{}",
            if cfg!(windows) {
                "node.exe"
            } else {
                "bin/node"
            }
        );
        if cfg!(windows) {
            zip_with(&archive, &[(&node, b"MZ")]);
        } else {
            tar_gz_with(&archive, &[(&node, "ELF", false)]);
        }
        // Skip the signed checksums: put a verified archive in the cache and install from it.
        let root = dir.path().join("node");
        let fetcher = Fetcher::insecure_for_tests(cache);
        let release = release(version);
        let expected = {
            use sha2::Digest;
            let digest = sha2::Sha256::digest(fs::read(&archive).unwrap());
            Expected::sha256(&pgp::hex(&digest).to_ascii_lowercase()).unwrap()
        };
        let cached = fetcher
            .fetch(
                "http://unused.invalid/",
                &platform.archive(version),
                &expected,
                &mut |_, _| {},
            )
            .unwrap();
        assert_eq!(cached, archive);
        let target = root.join(version);
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join(INCOMPLETE_MARKER), b"").unwrap();
        assert!(
            installed(&root).is_empty(),
            "an interrupted install doesn't count"
        );
        fs::remove_dir_all(&target).unwrap();
        fs::create_dir_all(&target).unwrap();
        if cfg!(windows) {
            extract_zip(&archive, &top, &target).unwrap();
        } else {
            extract_tar_gz(&archive, &top, &target).unwrap();
        }
        fs::write(
            target.join(INFO_FILE),
            r#"{"version":"22.23.4","lts":"jod"}"#,
        )
        .unwrap();
        let found = installed(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].lts.as_deref(), Some("jod"));
        assert!(found[0].node_exe().is_file());

        let again = install(
            &fetcher,
            &release,
            platform,
            &root,
            &mut |_, _| {},
            &ok_check,
        )
        .unwrap();
        assert!(again.already, "an install that is there isn't redone");
    }
}
