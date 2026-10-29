//! Installing a PHP build into Kiln's folder ([ADR 0013] §10–12): verified download, extraction
//! that can't escape its folder, `php.ini`, and a check that PHP actually starts.
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use kiln_fetch::{FetchError, Fetcher};

use crate::index::PhpRelease;
use crate::ini;

/// The PHP CLI in an install: `php.exe` on Windows, `php` on Linux ([`crate::linux::PHP_BIN`]).
pub const PHP_EXE: &str = if cfg!(windows) { "php.exe" } else { "php" };
pub const PHP_CGI_EXE: &str = "php-cgi.exe";
/// The CLI in a windows.php.net build, whatever platform this code runs on (its tests run on
/// Linux too).
const WINDOWS_PHP_EXE: &str = "php.exe";
/// Upper bound for one extracted build (a real one is about 120 MB).
pub const MAX_EXTRACTED: u64 = 1024 * 1024 * 1024;
/// Microsoft's permanent link to the Visual C++ 2015–2022 x64 runtime.
pub const VC_REDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";
/// `STATUS_DLL_NOT_FOUND`, as `ExitStatus::code` reports it on Windows.
const STATUS_DLL_NOT_FOUND: i32 = 0xC000_0135_u32.cast_signed();

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error("{archive}: {detail}")]
    Archive { archive: PathBuf, detail: String },
    #[error(
        "PHP {version} can't start: Windows is missing a DLL it needs, usually the Visual C++ \
         2015–2022 runtime. Install it from Microsoft ({VC_REDIST_URL}), then run the install \
         again. Kiln doesn't install system components itself."
    )]
    VcRuntimeMissing { version: String },
    #[error("PHP {version} failed its start-up check: {detail}")]
    Check { version: String, detail: String },
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> InstallError {
    let context = context.into();
    move |source| InstallError::Io { context, source }
}

/// An installed build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub dir: PathBuf,
    /// Extensions Kiln's `php.ini` section enabled (empty if it was already installed).
    pub extensions: Vec<&'static str>,
    /// `true` if this version was already there and nothing was changed.
    pub already: bool,
}

/// Downloads (or reuses the verified cache), extracts and checks `release` into
/// `<php_root>/<version>`.
pub fn install(
    fetcher: &Fetcher,
    release: &PhpRelease,
    php_root: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Installed, InstallError> {
    install_with(fetcher, release, php_root, progress, &check_runs)
}

type Check = dyn Fn(&Path, &str) -> Result<(), InstallError>;

pub(crate) fn install_with(
    fetcher: &Fetcher,
    release: &PhpRelease,
    php_root: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    check: &Check,
) -> Result<Installed, InstallError> {
    let dir = php_root.join(&release.version);
    let marker = dir.join(INCOMPLETE_MARKER);
    if windows_build_installed(&dir) {
        return Ok(Installed {
            version: release.version.clone(),
            dir,
            extensions: Vec::new(),
            already: true,
        });
    }
    let archive = fetcher.fetch(&release.url(), &release.archive, &release.sha256, progress)?;

    // Extract straight into the final folder, marked incomplete until the check passes. A
    // folder rename at the end would be atomic, but fails when antivirus is still scanning
    // the new executables (and inside packaged-app containers, I-44).
    remove_dir_if_present(&dir)?;
    fs::create_dir_all(&dir).map_err(io_err(format!("cannot create {}", dir.display())))?;
    fs::write(&marker, b"kiln: install in progress\n")
        .map_err(io_err(format!("cannot write {}", marker.display())))?;
    let result = (|| {
        extract(&archive, &dir)?;
        for required in [WINDOWS_PHP_EXE, PHP_CGI_EXE, "php.ini-development"] {
            if !dir.join(required).is_file() {
                return Err(InstallError::Archive {
                    archive: archive.clone(),
                    detail: format!("no {required} at the top level"),
                });
            }
        }
        let development = fs::read_to_string(dir.join("php.ini-development"))
            .map_err(io_err("cannot read php.ini-development"))?;
        let ext_dir = dir.join("ext");
        let (text, extensions) = ini::render(&development, |name| {
            ext_dir.join(format!("php_{name}.dll")).is_file()
        });
        fs::write(dir.join("php.ini"), text).map_err(io_err("cannot write php.ini"))?;
        check(&dir.join(WINDOWS_PHP_EXE), &release.version)?;
        fs::remove_file(&marker).map_err(io_err(format!("cannot remove {}", marker.display())))?;
        Ok(extensions)
    })();
    match result {
        Ok(extensions) => Ok(Installed {
            version: release.version.clone(),
            dir,
            extensions,
            already: false,
        }),
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// Present only while an install is in progress; a folder holding it isn't a usable PHP.
pub const INCOMPLETE_MARKER: &str = ".kiln-install-incomplete";

/// Whether `dir` holds a completed install of this platform (on Linux, see
/// [`crate::linux::is_installed`]).
#[must_use]
pub fn is_installed(dir: &Path) -> bool {
    if cfg!(windows) {
        windows_build_installed(dir)
    } else {
        crate::linux::is_installed(dir)
    }
}

/// Whether `dir` holds a completed windows.php.net build (this module's installs, on any
/// platform its tests run on).
fn windows_build_installed(dir: &Path) -> bool {
    dir.join(WINDOWS_PHP_EXE).is_file() && !dir.join(INCOMPLETE_MARKER).exists()
}

fn remove_dir_if_present(dir: &Path) -> Result<(), InstallError> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(io_err(format!("cannot remove {}", dir.display()))(e))
        }
        _ => Ok(()),
    }
}

/// Extracts every entry of `archive` under `dest`. Entries that would land outside it (absolute
/// paths, `..`) or are symbolic links are refused, and the total size is capped.
pub fn extract(archive: &Path, dest: &Path) -> Result<(), InstallError> {
    let bad = |detail: String| InstallError::Archive {
        archive: archive.to_owned(),
        detail,
    };
    let file = File::open(archive).map_err(io_err(format!("cannot open {}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(e.to_string()))?;
    fs::create_dir_all(dest).map_err(io_err(format!("cannot create {}", dest.display())))?;
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| bad(e.to_string()))?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(bad(format!("unsafe path {:?}", entry.name())));
        };
        if entry.is_symlink() {
            return Err(bad(format!("symbolic link {:?}", entry.name())));
        }
        let target = dest.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(io_err(format!("cannot create {}", target.display())))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(io_err(format!("cannot create {}", parent.display())))?;
        }
        let mut out =
            File::create(&target).map_err(io_err(format!("cannot create {}", target.display())))?;
        let budget = MAX_EXTRACTED - total;
        let written = io::copy(&mut (&mut entry).take(budget + 1), &mut out)
            .map_err(|e| bad(format!("{}: {e}", relative.display())))?;
        if written > budget {
            return Err(bad(format!("expands to more than {MAX_EXTRACTED} bytes")));
        }
        total += written;
    }
    Ok(())
}

/// Runs `php -v` and requires it to report `version` without extension load warnings.
fn check_runs(php: &Path, version: &str) -> Result<(), InstallError> {
    let output = Command::new(php)
        .arg("-v")
        .current_dir(php.parent().unwrap_or_else(|| Path::new(".")))
        .output()
        .map_err(io_err(format!("cannot run {}", php.display())))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    check_output(version, output.status.code(), &text)
}

fn check_output(version: &str, code: Option<i32>, text: &str) -> Result<(), InstallError> {
    if code == Some(STATUS_DLL_NOT_FOUND) {
        return Err(InstallError::VcRuntimeMissing {
            version: version.to_owned(),
        });
    }
    let failed = |detail: String| InstallError::Check {
        version: version.to_owned(),
        detail,
    };
    if code != Some(0) {
        return Err(failed(format!("exit code {code:?}: {}", text.trim())));
    }
    if !text.starts_with(&format!("PHP {version} ")) {
        return Err(failed(format!("unexpected output: {}", text.trim())));
    }
    if let Some(line) = text
        .lines()
        .find(|l| l.contains("Unable to load dynamic library") || l.contains("Warning"))
    {
        return Err(failed(line.trim().to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::PhpRelease;
    use kiln_fetch::Expected;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
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

    fn sha256_of(path: &Path) -> Expected {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(fs::read(path).unwrap());
        let hex = digest.iter().fold(String::new(), |mut hex, b| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{b:02x}");
            hex
        });
        Expected::sha256(&hex).unwrap()
    }

    fn release(archive: &str, sha256: Expected) -> PhpRelease {
        PhpRelease {
            minor: "8.4".into(),
            version: "8.4.27".into(),
            build: "nts-vs17-x64".into(),
            archive: archive.into(),
            sha256,
            archived: false,
        }
    }

    const GOOD: &[(&str, &[u8])] = &[
        ("php.exe", b"MZ php"),
        ("php-cgi.exe", b"MZ cgi"),
        ("php.ini-development", b"[PHP]\n;extension=curl\n"),
        ("ext/", b""),
        ("ext/php_curl.dll", b"MZ"),
        ("ext/php_zip.dll", b"MZ"),
    ];

    #[allow(clippy::unnecessary_wraps)] // must match the check signature
    fn ok_check(_: &Path, _: &str) -> Result<(), InstallError> {
        Ok(())
    }

    #[test]
    fn installs_from_the_verified_cache_with_php_ini() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("downloads");
        fs::create_dir_all(&cache).unwrap();
        let archive = cache.join("php-8.4.27-nts-Win32-vs17-x64.zip");
        zip_with(&archive, GOOD);
        let fetcher = Fetcher::insecure_for_tests(cache);
        let release = release("php-8.4.27-nts-Win32-vs17-x64.zip", sha256_of(&archive));
        let root = dir.path().join("php");
        let installed = install_with(&fetcher, &release, &root, &mut |_, _| {}, &ok_check).unwrap();
        assert!(!installed.already);
        assert_eq!(installed.dir, root.join("8.4.27"));
        assert!(installed.dir.join(PHP_CGI_EXE).is_file());
        assert_eq!(installed.extensions, ["curl", "zip"]);
        let ini = fs::read_to_string(installed.dir.join("php.ini")).unwrap();
        assert!(ini.contains("extension_dir = \"ext\"") && ini.contains("extension=zip"));
        assert!(
            !installed.dir.join(INCOMPLETE_MARKER).exists(),
            "marker removed"
        );
        assert!(windows_build_installed(&installed.dir));

        let again = install_with(&fetcher, &release, &root, &mut |_, _| {}, &ok_check).unwrap();
        assert!(again.already);
    }

    #[test]
    fn a_failed_check_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("downloads");
        fs::create_dir_all(&cache).unwrap();
        let archive = cache.join("p.zip");
        zip_with(&archive, GOOD);
        let fetcher = Fetcher::insecure_for_tests(cache);
        let release = release("p.zip", sha256_of(&archive));
        let root = dir.path().join("php");
        let missing_vc = |_: &Path, v: &str| {
            Err(InstallError::VcRuntimeMissing {
                version: v.to_owned(),
            })
        };
        let err = install_with(&fetcher, &release, &root, &mut |_, _| {}, &missing_vc).unwrap_err();
        assert!(
            err.to_string()
                .contains("aka.ms/vs/17/release/vc_redist.x64.exe"),
            "{err}"
        );
        assert!(!root.join("8.4.27").exists());
    }

    #[test]
    fn an_interrupted_install_is_not_counted_and_is_redone() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("downloads");
        fs::create_dir_all(&cache).unwrap();
        let archive = cache.join("p.zip");
        zip_with(&archive, GOOD);
        let fetcher = Fetcher::insecure_for_tests(cache);
        let release = release("p.zip", sha256_of(&archive));
        let root = dir.path().join("php");
        // What a crash mid-install leaves: files plus the marker.
        let left = root.join("8.4.27");
        fs::create_dir_all(&left).unwrap();
        fs::write(left.join(WINDOWS_PHP_EXE), b"half").unwrap();
        fs::write(left.join(INCOMPLETE_MARKER), b"").unwrap();
        assert!(!windows_build_installed(&left));
        let installed = install_with(&fetcher, &release, &root, &mut |_, _| {}, &ok_check).unwrap();
        assert!(!installed.already, "redone, not trusted");
        assert_eq!(fs::read(left.join(WINDOWS_PHP_EXE)).unwrap(), b"MZ php");
        assert!(windows_build_installed(&left));
    }

    #[test]
    fn an_archive_without_php_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("downloads");
        fs::create_dir_all(&cache).unwrap();
        let archive = cache.join("p.zip");
        zip_with(&archive, &[("README.txt", b"hi")]);
        let fetcher = Fetcher::insecure_for_tests(cache);
        let release = release("p.zip", sha256_of(&archive));
        let err = install_with(
            &fetcher,
            &release,
            &dir.path().join("php"),
            &mut |_, _| {},
            &ok_check,
        )
        .unwrap_err();
        assert!(matches!(err, InstallError::Archive { .. }), "{err}");
    }

    #[test]
    fn extraction_refuses_paths_that_escape() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("evil.zip");
        zip_with(&archive, &[("../escaped.txt", b"x")]);
        let err = extract(&archive, &dir.path().join("out")).unwrap_err();
        assert!(matches!(err, InstallError::Archive { .. }), "{err}");
        assert!(!dir.path().join("escaped.txt").exists());
    }

    #[test]
    fn start_up_check_reads_php_output() {
        let ok = "PHP 8.4.27 (cli) (built: Sep 30 2026) (NTS Visual C++ 2022 x64)\nCopyright (c) The PHP Group\n";
        assert!(check_output("8.4.27", Some(0), ok).is_ok());
        assert!(matches!(
            check_output("8.4.27", Some(STATUS_DLL_NOT_FOUND), ""),
            Err(InstallError::VcRuntimeMissing { .. })
        ));
        assert!(matches!(
            check_output("8.4.27", Some(0), "PHP 8.3.1 (cli)"),
            Err(InstallError::Check { .. })
        ));
        let warning =
            format!("{ok}PHP Warning:  PHP Startup: Unable to load dynamic library 'intl'");
        assert!(matches!(
            check_output("8.4.27", Some(0), &warning),
            Err(InstallError::Check { .. })
        ));
        assert_eq!(STATUS_DLL_NOT_FOUND, -1_073_741_515);
    }
}
