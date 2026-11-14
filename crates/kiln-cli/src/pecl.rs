//! `kiln php ext add <name>` ([ADR 0067]): a PECL extension Kiln pins, downloaded and checked,
//! placed in a PHP build's `ext` folder, then turned on like a bundled one (undoable).
//!
//! [ADR 0067]: ../../../docs/adr/0067-pecl-extensions.md

use std::path::{Path, PathBuf};

use kiln_php::pecl::{self, Package};
use kiln_state::PhpRecord;

fn local_dir() -> Result<PathBuf, String> {
    kiln_state::default_local_dir()
        .ok_or_else(|| "cannot determine Kiln's local data directory".to_owned())
}

/// The first file called `name` (any case) under `dir`.
fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find(&path, name) {
                return Some(found);
            }
        } else if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Some(path);
        }
    }
    None
}

/// Puts `package`'s DLL in `record`'s `ext` folder, downloading and checking its pinned
/// archive first. Returns whether it copied anything (false: it was there already).
pub fn place(package: &Package, record: &PhpRecord) -> Result<bool, String> {
    let target = record.dir.join("ext").join(package.dll());
    if target.is_file() {
        return Ok(false);
    }
    let build = package.build_for(&record.version, &record.archive).ok_or_else(|| {
        format!(
            "Kiln has no {} {} build pinned for PHP {} ({}): it has them for PHP 8.2 to 8.5, non-thread-safe x64",
            package.name, package.version, record.version, record.archive
        )
    })?;
    let expected = kiln_fetch::Expected::sha256(build.sha256).map_err(|e| e.to_string())?;
    let label = format!("Downloading {}", build.archive);
    let archive = crate::php::fetcher()?
        .fetch(
            &package.url(build),
            build.archive,
            &expected,
            &mut crate::php::progress_printer(&label),
        )
        .map_err(|e| e.to_string())?;
    let unpacked = local_dir()?.join("pecl").join(format!(
        "{}-{}-{}",
        package.name,
        package.version,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&unpacked);
    let copied = kiln_php::install::extract(&archive, &unpacked)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            let dll = find(&unpacked, &package.dll())
                .ok_or_else(|| format!("{} has no {}", build.archive, package.dll()))?;
            std::fs::copy(&dll, &target)
                .map(|_| ())
                .map_err(|e| format!("cannot copy {} to {}: {e}", dll.display(), target.display()))
        });
    let _ = std::fs::remove_dir_all(&unpacked);
    copied?;
    say!(
        "Verified {} (SHA-256 matches the pinned {} {}) and put {} in PHP {}'s ext folder.",
        build.archive,
        package.name,
        package.version,
        package.dll(),
        record.version
    );
    Ok(true)
}

/// `kiln php ext add <name> [--php X.Y]`: places the extension, then turns it on.
pub fn add(name: &str, spec: Option<&str>) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(format!(
            "PECL extensions are added to Kiln's PHP on Windows (ADR 0067); on Linux, install your distribution's php-{name}"
        ));
    }
    let package = pecl::package(name).ok_or_else(|| {
        let names: Vec<_> = pecl::PACKAGES.iter().map(|p| p.name).collect();
        format!("Kiln can add {} for now, not {name}", names.join(", "))
    })?;
    let store = crate::web::open_store()?;
    let record = crate::ext::target(&store, spec)?;
    place(package, &record)?;
    crate::ext::toggle(&[package.name.to_owned()], Some(&record.version), true)
}

/// For a new PHP (ADR 0072): adds `name` if Kiln has a build pinned for that PHP, saying so;
/// a PHP without one, or a failed download, is a note, never an error.
pub fn add_by_default(name: &str, version: &str) {
    let Some(package) = pecl::package(name) else {
        return;
    };
    let Ok(store) = crate::web::open_store() else {
        return;
    };
    let Ok(record) = crate::ext::target(&store, Some(version)) else {
        return;
    };
    drop(store);
    if package
        .build_for(&record.version, &record.archive)
        .is_none()
    {
        return;
    }
    crate::out::step(&format!("Adding php{name}"));
    match add(name, Some(version)) {
        Ok(()) => say!(
            "Added php{name} {} (`php ext off {name}` turns it off).",
            package.version
        ),
        Err(e) => say!("Note: php{name} wasn't added: {e}. `kiln php ext add {name}` tries again."),
    }
}

/// For `php update`: the PECL extensions `old` has that `new` lacks are placed in `new` too,
/// so their toggles carry over.
pub fn carry_files(old: &PhpRecord, new: &PhpRecord) {
    for package in pecl::PACKAGES {
        if old.dir.join("ext").join(package.dll()).is_file()
            && let Err(e) = place(package, new)
        {
            say!(
                "Note: {} isn't in PHP {} yet: {e}",
                package.name,
                new.version
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dll_is_found_in_the_archive_whatever_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/b/PHP_REDIS.DLL"), "x").unwrap();
        std::fs::write(dir.path().join("a/php_redis.pdb"), "x").unwrap();
        assert_eq!(
            find(dir.path(), "php_redis.dll"),
            Some(dir.path().join("a/b/PHP_REDIS.DLL"))
        );
        assert_eq!(find(dir.path(), "php_igbinary.dll"), None);
    }
}
