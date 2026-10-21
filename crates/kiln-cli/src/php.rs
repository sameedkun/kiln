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
    if crate::pool::running()?.is_none() || !crate::web::is_running()? {
        println!("It takes effect when the web server starts (`kiln web start`).");
        return Ok(());
    }
    let Some(new) = crate::pool::plan(store)? else {
        return Ok(());
    };
    crate::pool::switch(&new, crate::web::reload_with_php)?;
    println!("Switched the running server over without downtime.");
    Ok(())
}
