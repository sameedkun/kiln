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
    let modes = modes(&crate::web::open_store()?);
    let modes: Vec<&str> = modes.iter().map(String::as_str).collect();
    let output = output_dir()?;
    std::fs::create_dir_all(&output)
        .map_err(|e| format!("cannot create {}: {e}", output.display()))?;
    std::fs::write(&path, xdebug::debug_ini(&base, dll, &modes, &output))
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

/// The setting holding the debug worker's modes (ADR 0088 §1): `debug,profile`.
pub const MODES_KEY: &str = "xdebug.modes";

/// The debug worker's modes, `debug` alone unless chosen.
#[must_use]
pub fn modes(store: &StateStore) -> Vec<String> {
    let chosen: Vec<String> = store
        .setting(MODES_KEY)
        .ok()
        .flatten()
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|m| xdebug::MODES.contains(m))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if chosen.is_empty() {
        vec!["debug".to_owned()]
    } else {
        chosen
    }
}

/// Where profiles and traces go (ADR 0088 §2).
fn output_dir() -> Result<PathBuf, String> {
    Ok(local_dir()?.join("output"))
}

/// `modes` as the debug worker takes them: known ones, each once, in Kiln's order.
fn parse_modes(text: &str) -> Result<Vec<&'static str>, String> {
    let asked: Vec<&str> = text
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .collect();
    if let Some(bad) = asked.iter().find(|m| !xdebug::MODES.contains(m)) {
        return Err(format!(
            "Xdebug's debug worker runs {}; not {bad:?} (coverage is for test runs: `kiln run <site> pest --coverage`)",
            xdebug::MODES.join(", ")
        ));
    }
    let modes: Vec<&'static str> = xdebug::MODES
        .iter()
        .copied()
        .filter(|m| asked.contains(m))
        .collect();
    if modes.is_empty() {
        return Err("give at least one mode: debug, profile or trace".into());
    }
    Ok(modes)
}

/// `kiln php xdebug mode <modes>` (ADR 0088 §1).
pub fn set_modes(text: &str) -> Result<(), String> {
    let modes = parse_modes(text)?;
    let mut store = crate::web::open_store()?;
    store
        .set_setting(MODES_KEY, &modes.join(","))
        .map_err(|e| e.to_string())?;
    say!(
        "Xdebug's debug worker runs {} now, on a trigger.",
        modes.join(", ")
    );
    if modes.iter().any(|m| *m != "debug") {
        say!("Profiles and traces go to {}.", output_dir()?.display());
    }
    let on = minors_on(&store)?;
    if on.is_empty() {
        say!("Xdebug is off for every PHP: `kiln php xdebug on` turns it on.");
        return Ok(());
    }
    // The debug worker's php.ini keeps its path, so the pool looks unchanged: its workers are
    // restarted to read the new modes.
    for minor in &on {
        if let Ok(record) = crate::ext::target(&store, Some(minor)) {
            crate::web::restart_php(&store, &record.version)?;
        }
    }
    Ok(())
}

/// Profile and trace files, newest first: (name, bytes, modified as Unix seconds).
fn output_files(dir: &std::path::Path) -> Vec<(String, u64, u64)> {
    let mut files: Vec<(String, u64, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !(name.starts_with("cachegrind.out.") || name.starts_with("trace.")) {
                return None;
            }
            let meta = e.metadata().ok()?;
            let at = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some((name, meta.len(), at))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.2));
    files
}

/// `kiln php xdebug status` (ADR 0088 §2): where it's on, its modes, its files.
pub fn status() -> Result<(), String> {
    let store = crate::web::open_store()?;
    let on = minors_on(&store)?;
    let modes = modes(&store);
    drop(store);
    let dir = output_dir()?;
    let files = output_files(&dir);
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "on": on,
            "modes": modes,
            "output_dir": dir,
            "files": files.iter().map(|(name, bytes, at)| serde_json::json!({"name": name, "bytes": bytes, "at": at})).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if on.is_empty() {
        say!("Xdebug is off (`kiln php xdebug on` turns it on).");
    } else {
        say!(
            "Xdebug is on for PHP {}, running {} on a trigger.",
            on.join(", "),
            modes.join(", ")
        );
    }
    say!(
        "Profiles and traces: {} ({} file(s))",
        dir.display(),
        files.len()
    );
    for (name, bytes, _) in files.iter().take(20) {
        say!("  {name}  {}", crate::uninstall::mb(*bytes));
    }
    Ok(())
}

/// `kiln php xdebug clear`: deletes the profile and trace files, nothing else.
pub fn clear() -> Result<(), String> {
    let dir = output_dir()?;
    let files = output_files(&dir);
    for (name, _, _) in &files {
        let _ = std::fs::remove_file(dir.join(name));
    }
    say!(
        "Deleted {} profile and trace file(s) from {}.",
        files.len(),
        dir.display()
    );
    Ok(())
}

/// Xdebug's file for `record`, fetched and checked first if it isn't there.
pub(crate) fn ensure_dll(record: &PhpRecord) -> Result<PathBuf, String> {
    let build = pinned(record)?;
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
    Ok(dll)
}

/// `kiln php xdebug on [--php X.Y]`: fetches the pinned build (checked), proves PHP loads it,
/// then runs a debug worker beside the PHP's others.
pub fn on(spec: Option<&str>) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let record = target(&store, spec)?;
    let dll = ensure_dll(&record)?;
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
    fn modes_are_known_ones_in_kilns_order() {
        assert_eq!(parse_modes("trace, debug"), Ok(vec!["debug", "trace"]));
        assert_eq!(parse_modes("profile,profile"), Ok(vec!["profile"]));
        assert!(
            parse_modes("coverage")
                .unwrap_err()
                .contains("coverage is for test runs")
        );
        assert!(parse_modes(" , ").unwrap_err().contains("at least one"));
    }

    #[test]
    fn only_profiles_and_traces_are_listed_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["cachegrind.out.1.shop", "trace.2.shop.xt", "notes.txt"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let names: Vec<String> = output_files(dir.path()).into_iter().map(|f| f.0).collect();
        assert_eq!(names.len(), 2);
        assert!(!names.contains(&"notes.txt".to_owned()));
    }

    #[test]
    fn xdebug_is_kept_by_minor() {
        assert_eq!(minor("8.4.27"), "8.4");
        assert_eq!(minor("8.5"), "8.5");
    }
}
