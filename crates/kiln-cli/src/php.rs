//! PHP runtimes ([ADR 0013]): `kiln php available|install|list|remove`.
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

use std::io::Write;
use std::path::PathBuf;

use kiln_fetch::Fetcher;
use kiln_php::{INDEX_LIMIT, INDEX_URL, PhpRelease};
use kiln_state::{PhpRecord, StateStore};

/// Kiln's verified download cache.
pub fn fetcher() -> Result<Fetcher, String> {
    Ok(Fetcher::new(local_dir()?.join("downloads")))
}

fn local_dir() -> Result<PathBuf, String> {
    kiln_state::default_local_dir()
        .ok_or_else(|| "cannot determine Kiln's local data directory".to_owned())
}

fn open_store() -> Result<StateStore, String> {
    let dir = kiln_state::default_state_dir().ok_or("cannot determine the state directory")?;
    StateStore::open_dir(&dir)
        .map(|(store, _)| store)
        .map_err(|e| e.to_string())
}

/// Prints download progress on one line, about every 5%.
pub fn progress_printer(label: &str) -> impl FnMut(u64, Option<u64>) + '_ {
    let mut last_step = None;
    move |done, total| {
        let mb = |n: u64| {
            #[allow(clippy::cast_precision_loss)] // display only
            let n = n as f64 / 1_048_576.0;
            n
        };
        let step = total.map(|t| done * 20 / t.max(1));
        if step == last_step && done != total.unwrap_or(u64::MAX) {
            return;
        }
        last_step = step;
        match total {
            Some(total) => print!("\r{label}: {:.1} / {:.1} MB", mb(done), mb(total)),
            None => print!("\r{label}: {:.1} MB", mb(done)),
        }
        let _ = std::io::stdout().flush();
        if total == Some(done) {
            println!();
        }
    }
}

fn releases(fetcher: &Fetcher) -> Result<Vec<PhpRelease>, String> {
    let json = fetcher
        .get_text(INDEX_URL, INDEX_LIMIT)
        .map_err(|e| e.to_string())?;
    kiln_php::index::parse(&json).map_err(|e| e.to_string())
}

pub fn available() -> Result<(), String> {
    let fetcher = fetcher()?;
    let releases = releases(&fetcher)?;
    let installed: Vec<String> = open_store()?
        .php_versions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| r.version)
        .collect();
    println!("Installable PHP builds (x64, non-thread-safe) from windows.php.net:");
    for release in releases.iter().rev() {
        let mark = if installed.contains(&release.version) {
            "  (installed)"
        } else {
            ""
        };
        println!(
            "  {:<6} {:<8} {}{mark}",
            release.minor, release.version, release.archive
        );
    }
    println!("Install one with `kiln php install <version>`, e.g. `kiln php install 8.4`.");
    Ok(())
}

pub fn install(spec: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("PHP runtimes are only available on Windows so far (Linux: P1-04)".into());
    }
    let fetcher = fetcher()?;
    let releases = releases(&fetcher)?;
    let release = kiln_php::index::resolve(&releases, spec).map_err(|e| e.to_string())?;
    let mut store = open_store()?;
    let php_root = local_dir()?.join("php");
    println!(
        "PHP {} ({}), {} {}",
        release.version,
        release.build,
        release.sha256.algorithm(),
        release.sha256.hex()
    );
    let label = format!("Downloading {}", release.archive);
    let installed =
        kiln_php::install::install(&fetcher, release, &php_root, &mut progress_printer(&label))
            .map_err(|e| e.to_string())?;
    store
        .put_php(&PhpRecord {
            version: installed.version.clone(),
            dir: installed.dir.clone(),
            archive: release.archive.clone(),
            sha256: release.sha256.hex().to_owned(),
        })
        .map_err(|e| e.to_string())?;
    if installed.already {
        println!(
            "PHP {} is already installed in {}.",
            installed.version,
            installed.dir.display()
        );
    } else {
        println!(
            "Verified and installed PHP {} in {}.",
            installed.version,
            installed.dir.display()
        );
        println!("php.ini enables: {}", installed.extensions.join(", "));
        println!("It checked out: `php -v` reports {}.", installed.version);
    }
    Ok(())
}

pub fn list() -> Result<(), String> {
    let records = open_store()?.php_versions().map_err(|e| e.to_string())?;
    if records.is_empty() {
        println!("No PHP installed by Kiln yet. See `kiln php available`.");
        return Ok(());
    }
    for record in records {
        let state = if kiln_php::is_installed(&record.dir) {
            ""
        } else {
            "  (missing on disk; reinstall)"
        };
        println!("{:<8} {}{state}", record.version, record.dir.display());
    }
    Ok(())
}

pub fn remove(version: &str) -> Result<(), String> {
    let mut store = open_store()?;
    if crate::pool::running()?.is_some_and(|p| p.pools.iter().any(|pool| pool.id == version)) {
        return Err(format!(
            "PHP {version} is serving sites right now; switch with `kiln php default <other>` or stop the server first"
        ));
    }
    let Some(record) = store
        .php_versions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|r| r.version == version)
    else {
        return Err(format!(
            "PHP {version} isn't installed by Kiln (see `kiln php list`)"
        ));
    };
    // Only ever delete a folder inside Kiln's own PHP directory.
    let php_root = local_dir()?.join("php");
    if record.dir.parent() != Some(php_root.as_path()) {
        return Err(format!(
            "refusing to delete {}: it isn't inside {}",
            record.dir.display(),
            php_root.display()
        ));
    }
    match std::fs::remove_dir_all(&record.dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!(
                "cannot remove {} (is a PHP process still running?): {e}",
                record.dir.display()
            ));
        }
        _ => {}
    }
    store.remove_php(version).map_err(|e| e.to_string())?;
    println!("Removed PHP {version} ({}).", record.dir.display());
    Ok(())
}

/// `kiln php default <version>`: the PHP that serves sites.
pub fn set_default(version: &str) -> Result<(), String> {
    let mut store = open_store()?;
    let installed = store.php_versions().map_err(|e| e.to_string())?;
    if !installed
        .iter()
        .any(|r| r.version == version && kiln_php::is_installed(&r.dir))
    {
        return Err(format!(
            "PHP {version} isn't installed; see `kiln php list`, or `kiln php install {version}`"
        ));
    }
    store
        .set_setting(crate::pool::DEFAULT_KEY, version)
        .map_err(|e| e.to_string())?;
    println!("PHP {version} now serves sites.");
    apply_pool_change(&store)
}

/// `kiln php workers <n>`: php-cgi workers per pool.
pub fn set_workers(count: usize) -> Result<(), String> {
    let max = kiln_supervisor::pools::MAX_WORKERS;
    if !(1..=max).contains(&count) {
        return Err(format!("choose between 1 and {max} workers"));
    }
    let mut store = open_store()?;
    store
        .set_setting(crate::pool::WORKERS_KEY, &count.to_string())
        .map_err(|e| e.to_string())?;
    println!("{count} php-cgi worker(s) per pool.");
    apply_pool_change(&store)
}

/// Applies a changed pool to a running server without downtime, or leaves it for the next
/// start.
fn apply_pool_change(store: &StateStore) -> Result<(), String> {
    crate::web::apply_site_change(store)
}

/// The site a command means: `name` if given, else the site whose project folder contains the
/// current folder.
fn site_host(store: &StateStore, name: Option<String>) -> Result<String, String> {
    if let Some(name) = name {
        return crate::web::host_name(&name).map(|h| h.to_string());
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let sites = kiln_runtime::sites(store).map_err(|e| e.to_string())?;
    kiln_runtime::site_for_dir(&sites.sites, &cwd)
        .map(|s| s.host.to_string())
        .ok_or_else(|| {
            format!(
                "{} isn't inside a linked or parked site; name the site, e.g. `kiln isolate 8.3 blog`",
                cwd.display()
            )
        })
}

/// `kiln isolate <version> [site]` ([ADR 0016] §1).
///
/// [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md
pub fn isolate(version: &str, site: Option<String>) -> Result<(), String> {
    if !kiln_runtime::is_version_spec(version) {
        return Err(format!(
            "{version:?} isn't a PHP version; use e.g. 8.3 or 8.3.36"
        ));
    }
    let mut store = open_store()?;
    let host = site_host(&store, site)?;
    let sites = kiln_runtime::sites(&store).map_err(|e| e.to_string())?;
    if !sites.sites.iter().any(|s| s.host.as_str() == host) {
        return Err(format!(
            "{host} isn't a linked or parked site (see `kiln sites`)"
        ));
    }
    let installed = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
    let Some(php) = kiln_runtime::pick(&installed, version) else {
        return Err(format!(
            "PHP {version} isn't installed; run `kiln php install {version}` first"
        ));
    };
    store
        .isolate(&host, version.trim())
        .map_err(|e| e.to_string())?;
    println!(
        "{host} now uses PHP {} (pinned to {}).",
        php.version,
        version.trim()
    );
    if version.trim().split('.').count() == 2 {
        println!("It follows the newest installed {}.x.", version.trim());
    }
    crate::web::apply_site_change(&store)
}

/// `kiln unisolate [site]`.
pub fn unisolate(site: Option<String>) -> Result<(), String> {
    let mut store = open_store()?;
    let host = site_host(&store, site)?;
    if !store.unisolate(&host).map_err(|e| e.to_string())? {
        return Err(format!("{host} isn't pinned to a PHP version"));
    }
    let installed = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
    let default = kiln_runtime::default_php(&store, &installed).map_err(|e| e.to_string())?;
    println!(
        "{host} uses the default PHP again ({}).",
        default.map_or_else(|| "none installed".to_owned(), |p| p.version)
    );
    crate::web::apply_site_change(&store)
}

/// `kiln which [folder]`: what `php` and `composer` run there, and why.
pub fn which(dir: Option<PathBuf>) -> Result<(), String> {
    let dir = match dir {
        Some(dir) => dir,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let store = open_store()?;
    let here = kiln_runtime::php_for_dir(&store, &dir).map_err(|e| e.to_string())?;
    let why = match (&here.site, &here.php.pinned, here.php.fell_back) {
        (Some(site), Some(pin), false) => format!("{site} is pinned to {pin}"),
        (Some(site), Some(pin), true) => {
            format!("{site} is pinned to {pin}, which isn't installed, so the default")
        }
        (Some(site), None, _) => format!("{site} uses the default"),
        (None, _, _) => "not inside a site, so the default".to_owned(),
    };
    match &here.php.php {
        Some(php) => {
            println!("php      → PHP {} ({why})", php.version);
            println!("           {}", php.dir.join(kiln_php::PHP_EXE).display());
        }
        None => println!("php      → none: no PHP installed by Kiln (`kiln php install 8.4`)"),
    }
    let phar = local_dir()?.join(kiln_runtime::shim::COMPOSER_PHAR);
    if phar.is_file() {
        println!("composer → {} with that PHP", phar.display());
    } else {
        println!("composer → not installed (`kiln composer install`)");
    }
    let shims = local_dir()?.join(kiln_runtime::shim::SHIMS_DIR);
    if !shims
        .join(kiln_runtime::shim::Tool::Php.file_name())
        .is_file()
    {
        println!("The `php` / `composer` commands use this after `kiln shims install`.");
    }
    Ok(())
}

const COMPOSER_VERSIONS_URL: &str = "https://getcomposer.org/versions";

/// `kiln composer install`: the current stable Composer, SHA-256 verified ([ADR 0016] §10).
///
/// [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md
pub fn composer_install() -> Result<(), String> {
    let fetcher = fetcher()?;
    let index = fetcher
        .get_text(COMPOSER_VERSIONS_URL, 64 * 1024)
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value =
        serde_json::from_str(&index).map_err(|e| format!("unexpected Composer index: {e}"))?;
    let version = json["stable"][0]["version"]
        .as_str()
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
        .ok_or("unexpected Composer index: no stable version")?;
    let base = format!("https://getcomposer.org/download/{version}/composer.phar");
    let sum = fetcher
        .get_text(&format!("{base}.sha256sum"), 1024)
        .map_err(|e| e.to_string())?;
    let expected = kiln_fetch::Expected::sha256(sum.split_whitespace().next().unwrap_or(""))
        .map_err(|e| format!("unexpected Composer checksum file: {e}"))?;
    println!("Composer {version}, SHA-256 {}", expected.hex());
    let label = format!("Downloading composer {version}");
    let cached = fetcher
        .fetch(
            &base,
            &format!("composer-{version}.phar"),
            &expected,
            &mut progress_printer(&label),
        )
        .map_err(|e| e.to_string())?;
    let dest = local_dir()?.join(kiln_runtime::shim::COMPOSER_PHAR);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&cached, &dest).map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
    open_store()?
        .set_setting("composer.version", version)
        .map_err(|e| e.to_string())?;
    println!(
        "Verified and installed Composer {version} as {}.",
        dest.display()
    );
    Ok(())
}
