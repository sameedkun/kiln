//! PHP on Linux ([ADR 0022] §1–3): static-php-cli builds whose SHA-256 is pinned here, or the
//! distribution's own PHP, linked into Kiln's folder.
//!
//! Either way an install is a folder `<php root>/<name>/` holding `php`, `php-fpm` and
//! `php.ini`, so per-site versions, pools and shims don't care where PHP came from.
//!
//! [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use kiln_fetch::{Expected, Fetcher};

use crate::index::IndexError;
use crate::install::{INCOMPLETE_MARKER, InstallError, Installed, MAX_EXTRACTED};

/// The CLI binary in a Linux install.
pub const PHP_BIN: &str = "php";
/// The PHP-FPM binary in a Linux install.
pub const PHP_FPM_BIN: &str = "php-fpm";
/// Where the pinned builds live: Kiln's own static-php-cli builds, released from the public
/// `sameedkun/kiln-php` repository ([ADR 0025]). They have static-php-cli's `common` set
/// (`pdo_sqlite` for a new Laravel app) plus `mysqli` (which `WordPress` requires), `intl`,
/// `sodium` and `opcache`, as on Windows. No published set has both `mysqli` and `pdo_sqlite`.
///
/// [ADR 0025]: ../../../docs/adr/0025-own-static-linux-php-builds.md
pub const STATIC_BASE: &str = "https://github.com/sameedkun/kiln-php/releases/download/2026-10-27/";
/// What `php_versions.archive` records for a system PHP.
pub const SYSTEM_ARCHIVE: &str = "system";

/// One file of a pinned static build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    pub archive: &'static str,
    pub sha256: &'static str,
}

/// A pinned static-php-cli build: its CLI and FPM archives for one CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticBuild {
    pub version: &'static str,
    /// `std::env::consts::ARCH`.
    pub arch: &'static str,
    pub cli: Pin,
    pub fpm: Pin,
}

impl StaticBuild {
    #[must_use]
    pub fn minor(&self) -> &'static str {
        self.version
            .rsplit_once('.')
            .map_or(self.version, |(minor, _)| minor)
    }
}

/// Pinned builds ([ADR 0022] §1, [ADR 0025] §3). Each SHA-256 matches the release's own
/// `.sha256` file and two independent HTTPS downloads, and the build was run and checked
/// (`php -v`, `php -m`, `php-fpm -t`) before it was pinned. Add a pin only the same way, and
/// note it in docs/progress.md.
///
/// [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
/// [ADR 0025]: ../../../docs/adr/0025-own-static-linux-php-builds.md
pub const STATIC_BUILDS: &[StaticBuild] = &[
    // Release 2026-10-27 of sameedkun/kiln-php (static-php-cli 7bc4586). Checked 2026-10-27:
    // the release's `.sha256` files, a download on Windows and one in the Ubuntu 24.04 test VM
    // agree; each build ran there (`php -v`, `php -m` with mysqli, pdo_sqlite, intl, sodium
    // and OPcache, `php-fpm -t`).
    StaticBuild {
        version: "8.2.35",
        arch: "x86_64",
        cli: Pin {
            archive: "php-8.2.35-cli-linux-x86_64.tar.gz",
            sha256: "1bbe01a6af1fab907fe96e0b281b0847358f06cd985d30cd0b0b705c394522e9",
        },
        fpm: Pin {
            archive: "php-8.2.35-fpm-linux-x86_64.tar.gz",
            sha256: "f98bd4fc2d28db4515df2026d09dc15f81e398e530671e20da51fe459f618317",
        },
    },
    StaticBuild {
        version: "8.3.36",
        arch: "x86_64",
        cli: Pin {
            archive: "php-8.3.36-cli-linux-x86_64.tar.gz",
            sha256: "440e205b25090a2667fe697f26f630790b72e7568b5832e53b9a1fd436e8bad1",
        },
        fpm: Pin {
            archive: "php-8.3.36-fpm-linux-x86_64.tar.gz",
            sha256: "322bd1c16c73efee61b77277558d0e3b666d30533b527b735f9b5cfc570d0944",
        },
    },
    StaticBuild {
        version: "8.4.27",
        arch: "x86_64",
        cli: Pin {
            archive: "php-8.4.27-cli-linux-x86_64.tar.gz",
            sha256: "9bcff154f36ec6d961c2c33109484cc5380ba0fa31de14c34ee65d1bf072c446",
        },
        fpm: Pin {
            archive: "php-8.4.27-fpm-linux-x86_64.tar.gz",
            sha256: "ee8f78a44487ce33ac71a8042bf1217a09952c79b9d106bce7105baf9452324d",
        },
    },
    StaticBuild {
        version: "8.5.12",
        arch: "x86_64",
        cli: Pin {
            archive: "php-8.5.12-cli-linux-x86_64.tar.gz",
            sha256: "8b07a7dd2213165628cc29b229aa439cd897222b5a0bb5f2af1c9325188ee28b",
        },
        fpm: Pin {
            archive: "php-8.5.12-fpm-linux-x86_64.tar.gz",
            sha256: "43dc0dbd0d2e08b128ca74aa29ff4685bbaf2bd1c2d31055a930d2a17675b7d7",
        },
    },
];

/// The pinned build for `spec` (`8.4` or `8.4.27`) on `arch`.
pub fn resolve_static(spec: &str, arch: &str) -> Result<&'static StaticBuild, IndexError> {
    let spec = spec.trim();
    let parts: Vec<&str> = spec.split('.').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.parse::<u32>().is_err()) {
        return Err(IndexError::BadSpec(spec.to_owned()));
    }
    let here: Vec<&StaticBuild> = STATIC_BUILDS.iter().filter(|b| b.arch == arch).collect();
    let found = if parts.len() == 2 {
        here.iter().find(|b| b.minor() == spec)
    } else {
        here.iter().find(|b| b.version == spec)
    };
    found.copied().ok_or_else(|| IndexError::Unknown {
        requested: spec.to_owned(),
        available: if here.is_empty() {
            format!("none for {arch} yet")
        } else {
            here.iter()
                .map(|b| b.version)
                .collect::<Vec<_>>()
                .join(", ")
        },
    })
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> InstallError {
    let context = context.into();
    move |source| InstallError::Io { context, source }
}

/// Writes the one regular file called `name` from the `.tar.gz` `archive` to `dest`, mode
/// 0755. Links, other names and anything over [`MAX_EXTRACTED`] are refused; nothing else in
/// the archive is written.
pub fn extract_binary(archive: &Path, name: &str, dest: &Path) -> Result<(), InstallError> {
    let bad = |detail: String| InstallError::Archive {
        archive: archive.to_owned(),
        detail,
    };
    let file =
        fs::File::open(archive).map_err(io_err(format!("cannot open {}", archive.display())))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(|e| bad(e.to_string()))? {
        let entry = entry.map_err(|e| bad(e.to_string()))?;
        let path = entry.path().map_err(|e| bad(e.to_string()))?.into_owned();
        // At the top level only: `php` or `./php`, never `lib/php`.
        let mut parts = path
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir));
        let is_it = parts.next().is_some_and(|c| c.as_os_str() == name) && parts.next().is_none();
        if !is_it {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(bad(format!("{name} isn't a regular file")));
        }
        let mut bytes = Vec::new();
        entry
            .take(MAX_EXTRACTED + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| bad(e.to_string()))?;
        if bytes.len() as u64 > MAX_EXTRACTED {
            return Err(bad(format!("{name} is larger than {MAX_EXTRACTED} bytes")));
        }
        fs::write(dest, &bytes).map_err(io_err(format!("cannot write {}", dest.display())))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dest, fs::Permissions::from_mode(0o755))
                .map_err(io_err(format!("cannot make {} executable", dest.display())))?;
        }
        return Ok(());
    }
    Err(bad(format!("no {name} in the archive")))
}

/// Kiln's `php.ini` on Linux: development defaults only. Static builds have their
/// extensions compiled in; a system PHP still reads its distribution's extension files.
#[must_use]
pub fn default_ini() -> String {
    format!(
        "{}\n\
         display_errors = On\n\
         display_startup_errors = On\n\
         error_reporting = E_ALL\n\
         log_errors = On\n\
         {}\
         opcache.enable = 1\n\
         opcache.validate_timestamps = 1\n\
         opcache.revalidate_freq = 0\n",
        crate::ini::KILN_MARKER,
        crate::ini::LIMITS
    )
}

/// Checks a just-installed PHP: `php -v` names `version`. Linux only (it runs the binary).
pub type Check = dyn Fn(&Path, &str) -> Result<(), InstallError>;

/// The real [`Check`]: runs `php -v` and requires `PHP <version>` in its output.
pub fn check_runs(php: &Path, version: &str) -> Result<(), InstallError> {
    let failed = |detail: String| InstallError::Check {
        version: version.to_owned(),
        detail,
    };
    let output = std::process::Command::new(php)
        .arg("-n")
        .arg("-v")
        .output()
        .map_err(|e| failed(format!("cannot run {}: {e}", php.display())))?;
    let text = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && text.starts_with(&format!("PHP {version} ")) {
        Ok(())
    } else {
        Err(failed(format!(
            "`php -v` printed {:?}",
            text.lines().next().unwrap_or("")
        )))
    }
}

/// The version a PHP binary reports (`PHP_VERSION`, e.g. `8.3.6`).
pub fn version_of(php: &Path) -> io::Result<String> {
    let output = std::process::Command::new(php)
        .args(["-n", "-r", "echo PHP_VERSION;"])
        .output()?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let valid = output.status.success()
        && version.split('.').count() == 3
        && version.split('.').all(|p| p.parse::<u32>().is_ok());
    if valid {
        Ok(version)
    } else {
        Err(io::Error::other(format!(
            "{} didn't report a PHP version",
            php.display()
        )))
    }
}

/// Installs a pinned static build into `<php_root>/<version>`: verified downloads of the CLI
/// and FPM archives, their binaries, `php.ini`, and `check`.
pub fn install_static(
    fetcher: &Fetcher,
    build: &StaticBuild,
    php_root: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    check: &Check,
) -> Result<Installed, InstallError> {
    let dir = php_root.join(build.version);
    if is_installed(&dir) {
        return Ok(Installed {
            version: build.version.to_owned(),
            dir,
            extensions: Vec::new(),
            already: true,
        });
    }
    let mut archives = Vec::new();
    for pin in [build.cli, build.fpm] {
        let expected = Expected::sha256(pin.sha256)?;
        let url = format!("{STATIC_BASE}{}", pin.archive);
        archives.push(fetcher.fetch(&url, pin.archive, &expected, progress)?);
    }
    with_incomplete_marker(&dir, |dir| {
        extract_binary(&archives[0], PHP_BIN, &dir.join(PHP_BIN))?;
        extract_binary(&archives[1], PHP_FPM_BIN, &dir.join(PHP_FPM_BIN))?;
        fs::write(dir.join("php.ini"), default_ini()).map_err(io_err("cannot write php.ini"))?;
        check(&dir.join(PHP_BIN), build.version)
    })?;
    Ok(Installed {
        version: build.version.to_owned(),
        dir,
        extensions: Vec::new(),
        already: false,
    })
}

/// Fills `dir` from scratch with `fill`, marked incomplete until it succeeds; removed again
/// if it fails.
fn with_incomplete_marker(
    dir: &Path,
    fill: impl FnOnce(&Path) -> Result<(), InstallError>,
) -> Result<(), InstallError> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            return Err(io_err(format!("cannot remove {}", dir.display()))(e));
        }
        _ => {}
    }
    fs::create_dir_all(dir).map_err(io_err(format!("cannot create {}", dir.display())))?;
    let marker = dir.join(INCOMPLETE_MARKER);
    fs::write(&marker, b"kiln: install in progress\n")
        .map_err(io_err(format!("cannot write {}", marker.display())))?;
    match fill(dir) {
        Ok(()) => {
            fs::remove_file(&marker).map_err(io_err(format!("cannot remove {}", marker.display())))
        }
        Err(e) => {
            let _ = fs::remove_dir_all(dir);
            Err(e)
        }
    }
}

/// Whether `dir` holds a completed Linux install (its own binaries or links to the system's).
#[must_use]
pub fn is_installed(dir: &Path) -> bool {
    dir.join(PHP_BIN).is_file()
        && dir.join(PHP_FPM_BIN).is_file()
        && !dir.join(INCOMPLETE_MARKER).exists()
}

/// A PHP the distribution installed: the CLI and FPM binaries of one minor version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPhp {
    /// `8.3`
    pub minor: String,
    pub php: PathBuf,
    pub fpm: PathBuf,
}

/// The distribution PHPs under `root` (`/` in production) that have both a CLI and FPM:
/// Debian/Ubuntu's `php8.3` + `php-fpm8.3` (several versions side by side), then the single
/// `php` + `php-fpm` of Fedora and Arch. Pass `minor` to look for one version only.
#[must_use]
pub fn find_system(root: &Path, minor: Option<&str>) -> Vec<SystemPhp> {
    let mut found = Vec::new();
    let bins = root.join("usr/bin");
    if let Ok(entries) = fs::read_dir(&bins) {
        let mut minors: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let version = name.strip_prefix("php")?.to_owned();
                let (major, rest) = version.split_once('.')?;
                (major.parse::<u32>().is_ok() && rest.parse::<u32>().is_ok()).then_some(version)
            })
            .collect();
        minors.sort();
        for m in minors {
            let fpm = root.join(format!("usr/sbin/php-fpm{m}"));
            if fpm.is_file() && minor.is_none_or(|want| want == m) {
                found.push(SystemPhp {
                    php: bins.join(format!("php{m}")),
                    fpm,
                    minor: m,
                });
            }
        }
    }
    let (php, fpm) = (bins.join("php"), root.join("usr/sbin/php-fpm"));
    if found.is_empty() && php.is_file() && fpm.is_file() {
        // One unversioned PHP: its version comes from running it (`minor` unknown here).
        found.push(SystemPhp {
            minor: String::new(),
            php,
            fpm,
        });
    }
    found
}

/// Links a system PHP into `<php_root>/<version>-system` with Kiln's `php.ini`. `version`
/// is what the system PHP reports (e.g. `8.3.6`); nothing outside Kiln's folder changes.
#[cfg(unix)]
pub fn install_system(
    system: &SystemPhp,
    version: &str,
    php_root: &Path,
) -> Result<Installed, InstallError> {
    let dir = php_root.join(format!("{version}-system"));
    with_incomplete_marker(&dir, |dir| {
        for (link, target) in [(PHP_BIN, &system.php), (PHP_FPM_BIN, &system.fpm)] {
            std::os::unix::fs::symlink(target, dir.join(link))
                .map_err(io_err(format!("cannot link {}", target.display())))?;
        }
        fs::write(dir.join("php.ini"), default_ini()).map_err(io_err("cannot write php.ini"))
    })?;
    Ok(Installed {
        version: version.to_owned(),
        dir,
        extensions: Vec::new(),
        already: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_gz(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join("build.tar.gz");
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, name, *bytes).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        path
    }

    #[test]
    fn only_the_named_binary_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let archive = tar_gz(
            dir.path(),
            &[("README", b"hi"), ("./php-fpm", b"\x7fELF fpm")],
        );
        let dest = dir.path().join("out-php-fpm");
        extract_binary(&archive, PHP_FPM_BIN, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"\x7fELF fpm");
        assert!(!dir.path().join("README").exists());

        let err = extract_binary(&archive, PHP_BIN, &dir.path().join("x")).unwrap_err();
        assert!(err.to_string().contains("no php in the archive"), "{err}");
        // A `php` deeper in the tree isn't the binary.
        let nested = tar_gz(dir.path(), &[("lib/php", b"x")]);
        assert!(extract_binary(&nested, PHP_BIN, &dir.path().join("y")).is_err());
    }

    #[test]
    fn system_php_is_found_where_distributions_install_it() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        fs::create_dir_all(r.join("usr/bin")).unwrap();
        fs::create_dir_all(r.join("usr/sbin")).unwrap();
        assert_eq!(find_system(r, None), vec![]);
        for file in [
            "usr/bin/php8.3",
            "usr/sbin/php-fpm8.3",
            "usr/bin/php8.1",
            "usr/bin/phpize8.3",
        ] {
            fs::write(r.join(file), b"").unwrap();
        }
        let found = find_system(r, None);
        assert_eq!(found.len(), 1, "8.1 has no FPM, phpize isn't PHP");
        assert_eq!(found[0].minor, "8.3");
        assert!(found[0].fpm.ends_with("usr/sbin/php-fpm8.3"));
        assert_eq!(find_system(r, Some("8.1")), vec![]);

        let fedora = tempfile::tempdir().unwrap();
        let f = fedora.path();
        fs::create_dir_all(f.join("usr/bin")).unwrap();
        fs::create_dir_all(f.join("usr/sbin")).unwrap();
        fs::write(f.join("usr/bin/php"), b"").unwrap();
        fs::write(f.join("usr/sbin/php-fpm"), b"").unwrap();
        let found = find_system(f, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].minor, "", "learned by running it");
    }

    #[test]
    fn specs_resolve_against_the_pins_for_this_cpu() {
        assert!(matches!(
            resolve_static("eight", "x86_64"),
            Err(IndexError::BadSpec(_))
        ));
        let err = resolve_static("7.4", "riscv64").unwrap_err().to_string();
        assert!(err.contains("riscv64"), "{err}");
        for build in STATIC_BUILDS {
            assert_eq!(resolve_static(build.minor(), build.arch).unwrap(), build);
            assert_eq!(resolve_static(build.version, build.arch).unwrap(), build);
            for pin in [build.cli, build.fpm] {
                assert!(Expected::sha256(pin.sha256).is_ok(), "{}", pin.archive);
                assert!(pin.archive.contains(build.version) && pin.archive.contains(build.arch));
            }
        }
        assert!(default_ini().starts_with(crate::ini::KILN_MARKER));
    }
}
