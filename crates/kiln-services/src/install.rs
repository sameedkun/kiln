//! Installing a service's pinned release ([ADR 0036] §3–6, [ADR 0038] §2): the archive is
//! checked against its pinned SHA-256 again, then either its one executable is installed
//! under the provider's Kiln-specific name, or its top folder is unpacked (the files the
//! provider keeps) and the server gets its Kiln name as a hard link. Archives are read from
//! disk as they are unpacked, never whole into memory: `.zip`, `.tar.gz` and `.tar.xz`.
//!
//! [ADR 0036]: ../../../docs/adr/0036-service-framework.md
//! [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};

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

/// The SHA-256 of the file at `path`, lowercase hex, read in pieces.
fn sha256_of_file(path: &Path) -> io::Result<String> {
    use std::fmt::Write as _;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        }))
}

/// Checks `archive` against `release` and installs it into `bin`. Returns the installed
/// executable (the Kiln-named one).
pub fn install(
    provider: &dyn ServiceProvider,
    release: &Release,
    archive: &Path,
    bin: &Path,
) -> Result<PathBuf, InstallError> {
    let actual =
        sha256_of_file(archive).map_err(io_err(format!("cannot read {}", archive.display())))?;
    if actual != release.sha256 {
        return Err(InstallError::Unverified {
            archive: release.archive.to_owned(),
            actual,
            expected: release.sha256.to_owned(),
        });
    }
    let bad = |detail: String| InstallError::BadArchive {
        archive: release.archive.to_owned(),
        executable: release.executable.to_owned(),
        detail,
    };
    let dest = provider.executable(bin);
    if let Some(top) = release.tree {
        // Straight into a cleared `bin`, the Kiln-named link last: the CLI counts a version as
        // installed once that link exists, so an unpack that stopped half-way is redone. A
        // failed one is removed.
        let _ = fs::remove_dir_all(bin);
        let unpacked = unpack_tree(archive, release.archive, top, bin, &|path| {
            provider.keep(path)
        })
        .and_then(|()| {
            let original = bin.join(release.executable);
            if !original.is_file() {
                return Err("not in the archive".into());
            }
            fs::hard_link(&original, &dest)
                .map_err(|e| format!("cannot link {}: {e}", dest.display()))
        });
        if let Err(detail) = unpacked {
            let _ = fs::remove_dir_all(bin);
            return Err(bad(detail));
        }
        return Ok(dest);
    }

    fs::create_dir_all(bin).map_err(io_err(format!("cannot create {}", bin.display())))?;
    let temp = dest.with_extension("partial");
    if release.archive == release.executable {
        // The download is the executable itself (Meilisearch's): copied, not read into memory.
        fs::copy(archive, &temp).map_err(io_err(format!("cannot write {}", temp.display())))?;
    } else {
        let mut out =
            File::create(&temp).map_err(io_err(format!("cannot write {}", temp.display())))?;
        let copied = copy_file(archive, release.archive, release.executable, &mut out);
        drop(out);
        if let Err(detail) = copied {
            let _ = fs::remove_file(&temp);
            return Err(bad(detail));
        }
    }
    make_executable(&temp).map_err(io_err(format!("cannot make {} executable", temp.display())))?;
    kiln_sys::rename_replace(&temp, &dest)
        .map_err(io_err(format!("cannot install {}", dest.display())))?;
    Ok(dest)
}

/// Checks `archive` against `companion` and unpacks it into its folder in `bin`
/// ([`crate::companion_dir`]), replacing what was there ([ADR 0042] §1). Returns its
/// executable.
///
/// [ADR 0042]: ../../../docs/adr/0042-meilisearch-rustfs-mongodb.md
pub fn install_companion(
    companion: &Release,
    archive: &Path,
    bin: &Path,
) -> Result<PathBuf, InstallError> {
    let actual =
        sha256_of_file(archive).map_err(io_err(format!("cannot read {}", archive.display())))?;
    if actual != companion.sha256 {
        return Err(InstallError::Unverified {
            archive: companion.archive.to_owned(),
            actual,
            expected: companion.sha256.to_owned(),
        });
    }
    let dest = crate::companion_dir(bin, companion);
    let _ = fs::remove_dir_all(&dest);
    let top = companion.tree.unwrap_or_default();
    let unpacked = unpack_tree(archive, companion.archive, top, &dest, &|_| true).and_then(|()| {
        if dest.join(companion.executable).is_file() {
            Ok(())
        } else {
            Err("not in the archive".into())
        }
    });
    if let Err(detail) = unpacked {
        let _ = fs::remove_dir_all(&dest);
        return Err(InstallError::BadArchive {
            archive: companion.archive.to_owned(),
            executable: companion.executable.to_owned(),
            detail,
        });
    }
    Ok(dest.join(companion.executable))
}
#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // the Unix version can fail
fn make_executable(_: &Path) -> io::Result<()> {
    Ok(())
}

/// Whether the archive named `name` is of the type ending in `suffix`, in any case.
pub(crate) fn is_type(name: &str, suffix: &str) -> bool {
    name.to_ascii_lowercase().ends_with(suffix)
}

/// A tar archive's decompressed stream, by the archive's file name.
fn tar_stream(archive: &Path, name: &str) -> Result<Box<dyn Read>, String> {
    let file = BufReader::new(File::open(archive).map_err(|e| e.to_string())?);
    if is_type(name, ".tar.gz") {
        Ok(Box::new(flate2::read::GzDecoder::new(file)))
    } else if is_type(name, ".tar.xz") {
        Ok(Box::new(lzma_rust2::XzReader::new(file, true)))
    } else {
        Err(format!("{name}: not a .zip, .tar.gz or .tar.xz archive"))
    }
}

/// Copies the file at `inside` in the archive to `out`, in pieces.
fn copy_file(archive: &Path, name: &str, inside: &str, out: &mut File) -> Result<(), String> {
    if is_type(name, ".zip") {
        let file = File::open(archive).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| e.to_string())?;
        let mut entry = zip.by_name(inside).map_err(|e| e.to_string())?;
        io::copy(&mut entry, out).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut tar = tar::Archive::new(tar_stream(archive, name)?);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let is_it = entry.header().entry_type().is_file()
            && entry
                .path()
                .is_ok_and(|p| p == Path::new(inside) || p == Path::new(".").join(inside));
        if is_it {
            io::copy(&mut entry, out).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err("not in the archive".into())
}

/// `path` inside the top folder `top`, as a safe relative path: `None` for anything outside
/// it, and an error for a path that tries to leave it (`..`, an absolute path).
fn inside_tree(path: &Path, top: &str) -> Result<Option<PathBuf>, String> {
    let mut parts = path.components().filter(|c| *c != Component::CurDir);
    match parts.next() {
        Some(Component::Normal(first)) if first == top => {}
        _ => return Ok(None),
    }
    let mut rel = PathBuf::new();
    for part in parts {
        match part {
            Component::Normal(name) => rel.push(name),
            _ => return Err(format!("unsafe path in the archive: {}", path.display())),
        }
    }
    Ok((!rel.as_os_str().is_empty()).then_some(rel))
}

/// `rel` with `/` separators, for [`ServiceProvider::keep`].
fn slashed(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Unpacks what `keep` accepts under the archive's top folder `top` into `dest`.
fn unpack_tree(
    archive: &Path,
    name: &str,
    top: &str,
    dest: &Path,
    keep: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    if is_type(name, ".zip") {
        let file = File::open(archive).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            // `enclosed_name` is `None` for a path that would leave the archive.
            let name = entry
                .enclosed_name()
                .ok_or_else(|| format!("unsafe path in the archive (entry {i})"))?;
            let Some(rel) = inside_tree(&name, top)? else {
                continue;
            };
            if entry.is_dir() || !keep(&slashed(&rel)) {
                continue;
            }
            let target = dest.join(&rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = File::create(&target)
                .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
            io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let mut tar = tar::Archive::new(tar_stream(archive, name)?);
    tar.set_preserve_permissions(true);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let Some(rel) = inside_tree(&path, top)? else {
            continue;
        };
        let kind = entry.header().entry_type();
        if kind.is_dir() || !keep(&slashed(&rel)) {
            continue;
        }
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if kind.is_hard_link() {
            // Linked to another file of the archive: copy that one, once it is unpacked.
            let linked = entry
                .link_name()
                .map_err(|e| e.to_string())?
                .ok_or("a hard link without a target")?;
            let Some(source) = inside_tree(&linked, top)? else {
                return Err(format!(
                    "a hard link outside the tree: {}",
                    linked.display()
                ));
            };
            fs::copy(dest.join(source), &target).map_err(|e| e.to_string())?;
            continue;
        }
        // Files and symbolic links (Linux libraries name each other through them).
        entry
            .unpack(&target)
            .map_err(|e| format!("cannot unpack {}: {e}", rel.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Write as _;

    use crate::{InstanceDirs, Launch, PortRole};

    struct Fake(&'static str);

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
            &[]
        }
        fn executable_name(&self) -> &'static str {
            "kiln-fake"
        }
        fn init_data(&self, _: &InstanceDirs, _: &BTreeMap<String, u16>) -> Result<(), String> {
            Ok(())
        }
        fn launch(&self, _: &InstanceDirs, _: &BTreeMap<String, u16>) -> Launch {
            Launch::default()
        }
        fn executable(&self, bin: &Path) -> PathBuf {
            bin.join(self.0)
                .join(format!("kiln-fake{}", crate::EXE_SUFFIX))
        }
        fn keep(&self, path: &str) -> bool {
            !is_type(path, ".pdb") && !path.starts_with("tests/")
        }
    }

    fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        for (name, content) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        out.into_inner()
    }

    fn tar_gz_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, name, *content).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn release(archive: &'static str, sha256: &'static str) -> Release {
        Release {
            version: "1.0.0",
            os: "windows",
            arch: "x86_64",
            archive,
            url: "https://example.invalid/fake",
            sha256,
            executable: "fake.exe",
            tree: None,
        }
    }

    fn digest(path: &Path) -> &'static str {
        Box::leak(sha256_of_file(path).unwrap().into())
    }

    #[test]
    fn a_download_that_is_the_executable_is_installed_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("fake.exe");
        fs::write(&archive, b"the program").unwrap();
        let provider = Fake("");
        let bin = dir.path().join("bin");
        let installed = install(
            &provider,
            &release("fake.exe", digest(&archive)),
            &archive,
            &bin,
        )
        .unwrap();
        assert_eq!(fs::read(installed).unwrap(), b"the program");
    }

    #[test]
    fn a_companion_is_unpacked_into_its_own_folder_only_if_it_matches() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("shell.zip");
        fs::write(
            &archive,
            zip_with(&[
                ("shell-1/bin/shell.exe", b"shell"),
                ("shell-1/LICENSE", b"L"),
            ]),
        )
        .unwrap();
        let companion = Release {
            executable: "bin/shell.exe",
            tree: Some("shell-1"),
            ..release("shell.zip", digest(&archive))
        };
        let bin = dir.path().join("bin");
        let installed = install_companion(&companion, &archive, &bin).unwrap();
        assert_eq!(installed, bin.join("shell-1").join("bin").join("shell.exe"));
        assert_eq!(fs::read(&installed).unwrap(), b"shell");
        assert!(bin.join("shell-1").join("LICENSE").is_file());

        let wrong = Release {
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            ..companion
        };
        assert!(matches!(
            install_companion(&wrong, &archive, &bin),
            Err(InstallError::Unverified { .. })
        ));
        let missing = Release {
            executable: "bin/other.exe",
            ..companion
        };
        assert!(install_companion(&missing, &archive, &bin).is_err());
        assert!(!bin.join("shell-1").exists(), "a failed unpack is removed");
    }

    #[test]
    fn installs_the_executable_under_its_kiln_name_only_if_the_archive_matches() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("fake.zip");
        fs::write(&archive, zip_with(&[("fake.exe", b"binary")])).unwrap();
        let good = release("fake.zip", digest(&archive));
        let provider = Fake("");
        let bin = dir.path().join("bin").join("fake").join("1.0.0");

        let installed = install(&provider, &good, &archive, &bin).unwrap();
        assert_eq!(installed, provider.executable(&bin));
        assert!(installed.ends_with(format!("kiln-fake{}", crate::EXE_SUFFIX)));
        assert_eq!(fs::read(&installed).unwrap(), b"binary");

        let other = release(
            "fake.zip",
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
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

    #[test]
    fn a_tree_is_unpacked_without_what_the_provider_skips_and_the_server_gets_its_kiln_name() {
        let files: &[(&str, &[u8])] = &[
            ("db-1.0/bin/fake.exe", b"server"),
            ("db-1.0/bin/fake.pdb", b"symbols"),
            ("db-1.0/share/english/errmsg.sys", b"messages"),
            ("db-1.0/tests/t1.test", b"test"),
            ("README", b"outside the tree"),
        ];
        for (archive_name, bytes) in [
            ("db.zip", zip_with(files)),
            ("db.tar.gz", tar_gz_with(files)),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let archive = dir.path().join(archive_name);
            fs::write(&archive, bytes).unwrap();
            let tree = Release {
                executable: "bin/fake.exe",
                tree: Some("db-1.0"),
                ..release(Box::leak(archive_name.into()), digest(&archive))
            };
            let provider = Fake("bin");
            let bin = dir.path().join("bin").join("db").join("1.0.0");

            let installed = install(&provider, &tree, &archive, &bin).unwrap();
            assert_eq!(installed, provider.executable(&bin), "{archive_name}");
            assert_eq!(fs::read(&installed).unwrap(), b"server");
            assert_eq!(
                fs::read(bin.join("bin").join("fake.exe")).unwrap(),
                b"server"
            );
            assert!(
                bin.join("share")
                    .join("english")
                    .join("errmsg.sys")
                    .is_file()
            );
            assert!(!bin.join("bin").join("fake.pdb").exists(), "{archive_name}");
            assert!(!bin.join("tests").exists(), "{archive_name}");
            assert!(!bin.join("README").exists(), "{archive_name}");
        }
    }

    #[test]
    fn a_path_that_leaves_the_tree_is_refused() {
        assert_eq!(
            inside_tree(Path::new("db-1.0/bin/x"), "db-1.0").unwrap(),
            Some(PathBuf::from("bin").join("x"))
        );
        assert_eq!(inside_tree(Path::new("other/x"), "db-1.0").unwrap(), None);
        assert_eq!(inside_tree(Path::new("db-1.0"), "db-1.0").unwrap(), None);
        assert!(inside_tree(Path::new("db-1.0/../../etc/passwd"), "db-1.0").is_err());
        assert_eq!(
            slashed(&Path::new("lib").join("plugin").join("x.so")),
            "lib/plugin/x.so"
        );
    }
}
