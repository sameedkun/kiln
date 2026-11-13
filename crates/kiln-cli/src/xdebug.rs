//! `kiln php xdebug on|off` ([ADR 0061]): Xdebug on demand, in a debug worker of its own per
//! PHP version, so requests that don't ask for it stay fast.
//!
//! [ADR 0061]: ../../../docs/adr/0061-xdebug.md

use std::path::PathBuf;

use kiln_php::xdebug::{self, Build};
use kiln_state::{PhpRecord, StateStore};
use kiln_supervisor::PoolSpec;

/// The PHP minors (`8.4`) whose debug worker runs, comma-separated. By minor, so a
/// `kiln php update` keeps it.
pub const XDEBUG_KEY: &str = "php.xdebug";

fn minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// The minors with Xdebug on.
pub fn minors_on(store: &StateStore) -> Result<Vec<String>, String> {
    Ok(store
        .setting(XDEBUG_KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
        .collect())
}

fn save(store: &mut StateStore, minors: &[String]) -> Result<(), String> {
    let mut minors = minors.to_vec();
    minors.sort();
    minors.dedup();
    store
        .set_setting(XDEBUG_KEY, &minors.join(","))
        .map_err(|e| e.to_string())
}

/// Whether Xdebug is on for `version`.
pub fn is_on(store: &StateStore, version: &str) -> bool {
    minors_on(store).is_ok_and(|on| on.contains(&minor(version)))
}

fn local_dir() -> Result<PathBuf, String> {
    kiln_state::default_local_dir()
        .ok_or_else(|| "cannot determine Kiln's local data directory".to_owned())
}

/// `<local>/xdebug/<release>/<file>`: Kiln's own copy, outside PHP's `ext` folder, so
/// `kiln php ext on xdebug` can't load it into every worker and `kiln php update` keeps it.
fn dll_path(build: &Build) -> Result<PathBuf, String> {
    Ok(local_dir()?
        .join("xdebug")
        .join(xdebug::VERSION)
        .join(build.file))
}

/// `<local>/xdebug/<version>.ini`: the debug worker's `php.ini`, written again on each plan
/// from the PHP's own, so `kiln php ext on|off` reaches it too.
fn ini_path(version: &str) -> Result<PathBuf, String> {
    Ok(local_dir()?.join("xdebug").join(format!("{version}.ini")))
}

fn pinned(record: &PhpRecord) -> Result<&'static Build, String> {
    xdebug::build_for(&record.version, &record.archive).ok_or_else(|| {
        format!(
            "Kiln has no Xdebug {} build pinned for PHP {} ({}): it has them for PHP 8.2 to 8.5, non-thread-safe x64",
            xdebug::VERSION,
            record.version,
            record.archive
        )
    })
}

/// Writes the debug worker's `php.ini` for `record` and returns its path.
fn write_ini(record: &PhpRecord, dll: &std::path::Path) -> Result<PathBuf, String> {
    let base_path = record.dir.join("php.ini");
    let base = std::fs::read_to_string(&base_path)
        .map_err(|e| format!("cannot read {}: {e}", base_path.display()))?;
    let path = ini_path(&record.version)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, xdebug::debug_ini(&base, dll))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// The debug worker of `record` ([ADR 0061] §2): one php-cgi with Xdebug loaded, as the pool
/// `<version>+xdebug`. `None` when Xdebug is off for it, or its file is missing (said once).
///
/// [ADR 0061]: ../../../docs/adr/0061-xdebug.md
pub fn debug_pool(
    store: &StateStore,
    record: &PhpRecord,
    port: u16,
    max_requests: u32,
) -> Result<Option<PoolSpec>, String> {
    if !cfg!(windows) || !is_on(store, &record.version) {
        return Ok(None);
    }
    let Ok(build) = pinned(record) else {
        return Ok(None);
    };
    let dll = dll_path(build)?;
    if !dll.is_file() {
        say!(
            "Note: Xdebug's file for PHP {} is missing ({}); `kiln php xdebug on --php {}` fetches it again.",
            record.version,
            dll.display(),
            minor(&record.version)
        );
        return Ok(None);
    }
    let ini = write_ini(record, &dll)?;
    Ok(Some(PoolSpec::new(
        format!("{}{}", record.version, kiln_web::DEBUG_POOL_SUFFIX),
        record.dir.join(kiln_php::PHP_CGI_EXE),
        ini,
        &[port],
        max_requests,
    )))
}

/// Checks PHP starts with the debug `php.ini` and lists Xdebug, with no start-up warning.
fn check_loads(record: &PhpRecord, ini: &std::path::Path) -> Result<(), String> {
    let out = std::process::Command::new(record.dir.join(kiln_php::PHP_EXE))
        .arg("-c")
        .arg(ini)
        .arg("-m")
        .current_dir(&record.dir)
        .output()
        .map_err(|e| format!("cannot run PHP {}: {e}", record.version))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let zend = text
        .lines()
        .skip_while(|l| l.trim() != "[Zend Modules]")
        .any(|l| l.trim() == "Xdebug");
    // A warning about Xdebug itself; one from elsewhere in php.ini isn't this change's.
    let warned = text.lines().any(|l| {
        (l.contains("Warning") || l.contains("Failed loading"))
            && l.to_ascii_lowercase().contains("xdebug")
    });
    if out.status.success() && zend && !warned {
        Ok(())
    } else {
        Err(format!(
            "PHP {} didn't load Xdebug cleanly, so it stays off:\n{}",
            record.version,
            text.trim()
        ))
    }
}

fn target(store: &StateStore, spec: Option<&str>) -> Result<PhpRecord, String> {
    if !cfg!(windows) {
        return Err(
            "Xdebug on demand is for Kiln's PHP on Windows for now (ADR 0061); on Linux, install your distribution's php-xdebug".to_owned(),
        );
    }
    crate::ext::target(store, spec)
}

/// `kiln php xdebug on [--php X.Y]`: fetches the pinned build (checked), proves PHP loads it,
/// then runs a debug worker beside the PHP's others.
pub fn on(spec: Option<&str>) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let record = target(&store, spec)?;
    let build = pinned(&record)?;
    let dll = dll_path(build)?;
    if !dll.is_file() {
        let expected = kiln_fetch::Expected::sha256(build.sha256).map_err(|e| e.to_string())?;
        let label = format!("Downloading {}", build.file);
        let cached = crate::php::fetcher()?
            .fetch(
                &build.url(),
                build.file,
                &expected,
                &mut crate::php::progress_printer(&label),
            )
            .map_err(|e| e.to_string())?;
        if let Some(dir) = dll.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        std::fs::copy(&cached, &dll)
            .map_err(|e| format!("cannot copy {} to {}: {e}", cached.display(), dll.display()))?;
        say!(
            "Verified {} (SHA-256 matches the pinned Xdebug {}).",
            build.file,
            xdebug::VERSION
        );
    }
    check_loads(&record, &write_ini(&record, &dll)?)?;
    let minor = minor(&record.version);
    let mut minors = minors_on(&store)?;
    if minors.contains(&minor) {
        say!("Xdebug is already on for PHP {minor}.");
        return Ok(());
    }
    minors.push(minor.clone());
    save(&mut store, &minors)?;
    say!(
        "Xdebug {} is on for PHP {minor}: a request with the XDEBUG_SESSION cookie (your browser's Xdebug helper) or ?XDEBUG_SESSION_START=1 goes to its debug worker, which connects to your editor on 127.0.0.1:{}. Other requests stay as fast as before.",
        xdebug::VERSION,
        xdebug::CLIENT_PORT
    );
    crate::web::apply_site_change(&store)
}

/// `kiln php xdebug off [--php X.Y]`: the debug worker stops; Xdebug's file stays for next time.
pub fn off(spec: Option<&str>) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let record = target(&store, spec)?;
    let minor = minor(&record.version);
    let mut minors = minors_on(&store)?;
    if !minors.contains(&minor) {
        say!("Xdebug is already off for PHP {minor}.");
        return Ok(());
    }
    minors.retain(|m| *m != minor);
    save(&mut store, &minors)?;
    say!("Xdebug is off for PHP {minor}.");
    crate::web::apply_site_change(&store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdebug_is_kept_by_minor() {
        assert_eq!(minor("8.4.27"), "8.4");
        assert_eq!(minor("8.5"), "8.5");
    }
}
