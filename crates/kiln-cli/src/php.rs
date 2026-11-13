//! PHP runtimes ([ADR 0013]): `kiln php available|install|list|remove`.
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

use std::io::Write;
use std::path::PathBuf;

use kiln_fetch::Fetcher;
use kiln_php::{INDEX_LIMIT, INDEX_URL, PhpRelease};
use kiln_runtime::DEFAULT_KEY;
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

/// Prints download progress on one line, about every 5%; with --json, a `progress` event
/// about every 2%.
pub fn progress_printer(label: &str) -> impl FnMut(u64, Option<u64>) + '_ {
    let mut last_step = None;
    move |done, total| {
        if crate::out::json() {
            // Without a total, every 4 MB.
            let step = total.map_or(done / 4_194_304, |t| done * 50 / t.max(1));
            if Some(step) != last_step || total == Some(done) {
                last_step = Some(step);
                crate::out::progress(label, done, total);
            }
            return;
        }
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
            say!();
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
    if !cfg!(windows) {
        return available_linux();
    }
    let fetcher = fetcher()?;
    let releases = releases(&fetcher)?;
    let installed: Vec<String> = open_store()?
        .php_versions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| r.version)
        .collect();
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "releases": releases.iter().rev().map(|r| serde_json::json!({
                "line": r.minor,
                "version": r.version,
                "archive": r.archive,
                "installed": installed.contains(&r.version),
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    say!("Installable PHP builds (x64, non-thread-safe) from windows.php.net:");
    for release in releases.iter().rev() {
        let mark = if installed.contains(&release.version) {
            "  (installed)"
        } else {
            ""
        };
        say!(
            "  {:<6} {:<8} {}{mark}",
            release.minor,
            release.version,
            release.archive
        );
    }
    say!("Install one with `kiln php install <version>`, e.g. `kiln php install 8.4`.");
    Ok(())
}

pub fn install(spec: &str, system: bool) -> Result<(), String> {
    if !cfg!(windows) {
        return if system {
            install_system(spec)
        } else {
            install_static(spec)
        };
    }
    if system {
        return Err(
            "--system is for Linux; on Windows Kiln installs PHP from windows.php.net".into(),
        );
    }
    let fetcher = fetcher()?;
    let releases = releases(&fetcher)?;
    let release = kiln_php::index::resolve(&releases, spec).map_err(|e| e.to_string())?;
    let mut store = open_store()?;
    let php_root = local_dir()?.join("php");
    say!(
        "PHP {} ({}), {} {}",
        release.version,
        release.build,
        release.sha256.algorithm(),
        release.sha256.hex()
    );
    let label = format!("Downloading {}", release.archive);
    let installed =
        kiln_php::install::install(&fetcher, &release, &php_root, &mut progress_printer(&label))
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
        say!(
            "PHP {} is already installed in {}.",
            installed.version,
            installed.dir.display()
        );
    } else {
        say!(
            "Verified and installed PHP {} in {}.",
            installed.version,
            installed.dir.display()
        );
        say!("php.ini enables: {}", installed.extensions.join(", "));
        say!("It checked out: `php -v` reports {}.", installed.version);
    }
    Ok(())
}

/// Linux: the pinned static builds for this CPU, and the distribution's PHP found here.
fn available_linux() -> Result<(), String> {
    use kiln_php::linux;
    let arch = std::env::consts::ARCH;
    let installed: Vec<String> = open_store()?
        .php_versions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| r.version)
        .collect();
    let mark = |version: &str| {
        if installed.iter().any(|v| v == version) {
            "  (installed)"
        } else {
            ""
        }
    };
    say!("Downloadable (static-php-cli, {arch}, SHA-256 pinned in Kiln):");
    let builds: Vec<_> = linux::STATIC_BUILDS
        .iter()
        .filter(|b| b.arch == arch)
        .collect();
    if builds.is_empty() {
        say!("  (none for {arch} yet)");
    }
    for build in builds.iter().rev() {
        say!(
            "  {:<6} {:<8}{}",
            build.minor(),
            build.version,
            mark(build.version)
        );
    }
    say!("From your distribution (`kiln php install <version> --system`):");
    let system = linux::find_system(std::path::Path::new("/"), None);
    if system.is_empty() {
        say!("  (none with PHP-FPM; e.g. `sudo apt install php8.3-fpm php8.3-cli`)");
    }
    for php in system {
        let version = linux::version_of(&php.php).unwrap_or_else(|_| "?".into());
        say!(
            "  {:<8} {} + {}{}",
            version,
            php.php.display(),
            php.fpm.display(),
            mark(&version)
        );
    }
    Ok(())
}

/// The first two parts of `8.3` or `8.3.6`.
fn minor_of(spec: &str) -> Result<String, String> {
    let parts: Vec<&str> = spec.trim().split('.').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.parse::<u32>().is_err()) {
        return Err(format!("{spec:?} isn't a PHP version (expected e.g. 8.3)"));
    }
    Ok(format!("{}.{}", parts[0], parts[1]))
}

/// `kiln php install 8.4` on Linux ([ADR 0022] §1): a static-php-cli build whose SHA-256 is
/// pinned in Kiln.
///
/// [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
fn install_static(spec: &str) -> Result<(), String> {
    use kiln_php::linux;
    let build = linux::resolve_static(spec, std::env::consts::ARCH).map_err(|e| e.to_string())?;
    let fetcher = fetcher()?;
    let mut store = open_store()?;
    let php_root = local_dir()?.join("php");
    say!(
        "PHP {} (static-php-cli, {}), SHA-256 pinned in Kiln",
        build.version,
        std::env::consts::ARCH
    );
    let installed = linux::install_static(
        &fetcher,
        build,
        &php_root,
        &mut progress_printer("Downloading"),
        &linux::check_runs,
    )
    .map_err(|e| e.to_string())?;
    store
        .put_php(&PhpRecord {
            version: installed.version.clone(),
            dir: installed.dir.clone(),
            archive: build.cli.archive.to_owned(),
            sha256: build.cli.sha256.to_owned(),
        })
        .map_err(|e| e.to_string())?;
    if installed.already {
        say!(
            "PHP {} is already installed in {}.",
            installed.version,
            installed.dir.display()
        );
    } else {
        say!(
            "Verified and installed PHP {} (php and php-fpm) in {}.",
            installed.version,
            installed.dir.display()
        );
        say!(
            "Extensions are built in (`{} -m` lists them). intl, opcache and sodium aren't; for those use your distribution's PHP: `kiln php install {} --system`.",
            installed.dir.join(linux::PHP_BIN).display(),
            build.minor()
        );
    }
    Ok(())
}

/// `kiln php install 8.3 --system` ([ADR 0022] §2): links the distribution's PHP into Kiln's
/// folder. Nothing is downloaded and nothing outside Kiln's folder changes.
///
/// [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
#[cfg(unix)]
fn install_system(spec: &str) -> Result<(), String> {
    use kiln_php::linux;
    let minor = minor_of(spec)?;
    let candidates = linux::find_system(std::path::Path::new("/"), None);
    let found = candidates.iter().find_map(|system| {
        let version = linux::version_of(&system.php).ok()?;
        version
            .starts_with(&format!("{minor}."))
            .then(|| (system.clone(), version))
    });
    let Some((system, version)) = found else {
        let have: Vec<String> = candidates
            .iter()
            .filter_map(|s| linux::version_of(&s.php).ok())
            .collect();
        return Err(format!(
            "no PHP {minor} with PHP-FPM from your distribution{}. Install it first, e.g. `sudo apt install php{minor}-fpm php{minor}-cli` (Debian/Ubuntu) or `sudo dnf install php-fpm php-cli` (Fedora), or let Kiln download one: `kiln php install {minor}`",
            if have.is_empty() {
                String::new()
            } else {
                format!(" (found: {})", have.join(", "))
            }
        ));
    };
    let mut store = open_store()?;
    let php_root = local_dir()?.join("php");
    let installed =
        linux::install_system(&system, &version, &php_root).map_err(|e| e.to_string())?;
    store
        .put_php(&PhpRecord {
            version: installed.version.clone(),
            dir: installed.dir.clone(),
            archive: linux::SYSTEM_ARCHIVE.to_owned(),
            sha256: String::new(),
        })
        .map_err(|e| e.to_string())?;
    say!(
        "Using your distribution's PHP {version}: {} and {}, linked into {}.",
        system.php.display(),
        system.fpm.display(),
        installed.dir.display()
    );
    say!(
        "Kiln's php.ini there sets development defaults; extensions come from your distribution's packages."
    );
    Ok(())
}

#[cfg(not(unix))]
fn install_system(_spec: &str) -> Result<(), String> {
    Err("--system is for Linux".into())
}

pub fn list() -> Result<(), String> {
    let store = open_store()?;
    let records = store.php_versions().map_err(|e| e.to_string())?;
    if crate::out::json() {
        let default = store
            .setting(crate::pool::DEFAULT_KEY)
            .map_err(|e| e.to_string())?;
        crate::out::data(serde_json::json!({
            "default": default,
            "versions": records.iter().map(|r| serde_json::json!({
                "version": r.version,
                "dir": r.dir,
                "installed": kiln_php::is_installed(&r.dir),
                "system": r.archive == kiln_php::linux::SYSTEM_ARCHIVE,
                "xdebug": crate::xdebug::is_on(&store, &r.version),
                "xdebug_available": cfg!(windows) && kiln_php::xdebug::build_for(&r.version, &r.archive).is_some(),
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if records.is_empty() {
        say!("No PHP installed by Kiln yet. See `kiln php available`.");
        return Ok(());
    }
    for record in records {
        let state = if kiln_php::is_installed(&record.dir) {
            ""
        } else {
            "  (missing on disk; reinstall)"
        };
        let origin = if record.archive == kiln_php::linux::SYSTEM_ARCHIVE {
            "  (your distribution's)"
        } else {
            ""
        };
        say!(
            "{:<8} {}{origin}{state}",
            record.version,
            record.dir.display()
        );
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
    crate::ext::forget(&record)?;
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
    say!("Removed PHP {version} ({}).", record.dir.display());
    Ok(())
}

/// The setting recording the patch `kiln php update` moved `minor` away from ([ADR 0029] §2).
///
/// [ADR 0029]: ../../../docs/adr/0029-php-patch-updates-and-rollback.md
fn rollback_key(minor: &str) -> String {
    format!("php.rollback.{minor}")
}

fn version_numbers(version: &str) -> Vec<u32> {
    version.split('.').filter_map(|p| p.parse().ok()).collect()
}

/// (minor, newest patch offered) pairs.
type Offered = Vec<(String, String)>;

/// The newest patch offered for each of `minors`, and where that comes from: the release
/// index on Windows, the builds pinned in this Kiln on Linux.
fn newest_offered(minors: &[String]) -> Result<(Offered, &'static str), String> {
    if cfg!(windows) {
        let releases = releases(&fetcher()?)?;
        let newest = minors
            .iter()
            .filter_map(|minor| {
                kiln_php::index::resolve(&releases, minor)
                    .ok()
                    .map(|r| (minor.clone(), r.version))
            })
            .collect();
        Ok((newest, "windows.php.net"))
    } else {
        let newest = minors
            .iter()
            .filter_map(|minor| {
                kiln_php::linux::resolve_static(minor, std::env::consts::ARCH)
                    .ok()
                    .map(|b| (minor.clone(), b.version.to_owned()))
            })
            .collect();
        Ok((newest, "the builds pinned in this Kiln"))
    }
}

/// `kiln php update [minor]` ([ADR 0029] §2): the newest patch of each installed line (or the
/// one named), beside the current one, switched to live.
///
/// [ADR 0029]: ../../../docs/adr/0029-php-patch-updates-and-rollback.md
pub fn update(minor: Option<&str>) -> Result<(), String> {
    let store = open_store()?;
    let installed = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
    let is_system = |r: &PhpRecord| r.archive == kiln_php::linux::SYSTEM_ARCHIVE;
    let mut minors: Vec<String> = Vec::new();
    for record in installed.iter().filter(|r| !is_system(r)) {
        let minor = minor_of(&record.version)?;
        if !minors.contains(&minor) {
            minors.push(minor);
        }
    }
    for record in installed.iter().filter(|r| is_system(r)) {
        say!(
            "PHP {} is your distribution's: your package manager updates it.",
            record.version
        );
    }
    if let Some(wanted) = minor {
        let wanted = minor_of(wanted)?;
        if !minors.contains(&wanted) {
            return Err(format!(
                "no PHP {wanted} installed by Kiln to update (`kiln php install {wanted}`)"
            ));
        }
        minors = vec![wanted];
    }
    if minors.is_empty() {
        say!("No PHP installed by Kiln to update. See `kiln php available`.");
        return Ok(());
    }
    let (offered, source) = newest_offered(&minors)?;
    drop(store);

    let mut updated = false;
    for minor in &minors {
        let Some(current) = kiln_runtime::pick(&installed, minor).map(|r| r.version.clone()) else {
            continue;
        };
        let Some((_, newest)) = offered.iter().find(|(m, _)| m == minor) else {
            say!("PHP {minor}: {source} no longer offers it; staying on {current}.");
            continue;
        };
        if version_numbers(newest) <= version_numbers(&current) {
            say!("PHP {minor}: {current} is the newest in {source}.");
            continue;
        }
        say!("PHP {minor}: {current} → {newest}");
        install(newest, false)?;
        let mut store = open_store()?;
        let now = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
        if let (Some(old), Some(new)) = (
            installed.iter().find(|r| r.version == current),
            now.iter().find(|r| r.version == *newest),
        ) {
            crate::ext::carry_over(old, new)?;
        }
        store
            .set_setting(&rollback_key(minor), &current)
            .map_err(|e| e.to_string())?;
        if store
            .setting(DEFAULT_KEY)
            .map_err(|e| e.to_string())?
            .as_deref()
            == Some(&current)
        {
            store
                .set_setting(DEFAULT_KEY, newest)
                .map_err(|e| e.to_string())?;
            say!("The default PHP moves to {newest}.");
        }
        for (host, pin) in store.isolations().map_err(|e| e.to_string())? {
            if pin == current {
                say!(
                    "{host} is pinned to {current} exactly, and stays on it (`kiln isolate {minor} {host}` follows updates)."
                );
            }
        }
        say!("PHP {current} is kept for `kiln php rollback {minor}`.");
        for older in installed.iter().filter(|r| {
            r.version != current
                && minor_of(&r.version).as_deref() == Ok(minor.as_str())
                && version_numbers(&r.version) < version_numbers(&current)
        }) {
            say!(
                "PHP {} is no longer used by default; `kiln php remove {}` frees its space.",
                older.version,
                older.version
            );
        }
        updated = true;
    }
    if updated {
        crate::web::apply_site_change(&open_store()?)?;
    }
    Ok(())
}

/// The line to roll back and the patch to go back to: `minor`'s recorded update, or the only
/// one recorded.
fn recorded_update(
    store: &StateStore,
    installed: &[PhpRecord],
    minor: Option<&str>,
) -> Result<(String, String), String> {
    let mut recorded: Vec<(String, String)> = Vec::new();
    for record in installed {
        let minor = minor_of(&record.version)?;
        if recorded.iter().any(|(m, _)| *m == minor) {
            continue;
        }
        if let Some(previous) = store
            .setting(&rollback_key(&minor))
            .map_err(|e| e.to_string())?
        {
            recorded.push((minor, previous));
        }
    }
    if let Some(wanted) = minor {
        let wanted = minor_of(wanted)?;
        return recorded
            .into_iter()
            .find(|(m, _)| *m == wanted)
            .ok_or_else(|| {
                format!("nothing to roll back for PHP {wanted}: no `kiln php update` is recorded")
            });
    }
    match recorded.len() {
        0 => Err("nothing to roll back: no `kiln php update` is recorded".into()),
        1 => Ok(recorded.remove(0)),
        _ => {
            let names: Vec<&str> = recorded.iter().map(|(m, _)| m.as_str()).collect();
            Err(format!(
                "updates of several versions are recorded ({}); name one, e.g. `kiln php rollback {}`",
                names.join(", "),
                names[0]
            ))
        }
    }
}

/// `kiln php rollback [minor]` ([ADR 0029] §3): back to the patch the last update replaced.
///
/// [ADR 0029]: ../../../docs/adr/0029-php-patch-updates-and-rollback.md
pub fn rollback(minor: Option<&str>) -> Result<(), String> {
    let mut store = open_store()?;
    let installed = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
    let (minor, previous) = recorded_update(&store, &installed, minor)?;
    if !installed.iter().any(|r| r.version == previous) {
        return Err(format!(
            "PHP {previous} was removed since the update, so there's nothing to go back to"
        ));
    }
    let Some(current) = kiln_runtime::pick(&installed, &minor).cloned() else {
        return Err(format!("no PHP {minor} installed"));
    };
    if version_numbers(&current.version) <= version_numbers(&previous) {
        store
            .remove_setting(&rollback_key(&minor))
            .map_err(|e| e.to_string())?;
        return Err(format!("PHP {minor} is already on {previous}"));
    }
    let pinned: Vec<String> = store
        .isolations()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|(_, pin)| *pin == current.version)
        .map(|(host, _)| host)
        .collect();
    if !pinned.is_empty() {
        return Err(format!(
            "{} pinned to {} exactly; pin another version or `kiln unisolate` first",
            pinned.join(", "),
            current.version
        ));
    }
    // Only ever delete a folder inside Kiln's own PHP directory.
    let php_root = local_dir()?.join("php");
    if current.dir.parent() != Some(php_root.as_path()) {
        return Err(format!(
            "refusing to delete {}: it isn't inside {}",
            current.dir.display(),
            php_root.display()
        ));
    }

    say!("PHP {minor}: {} → {previous}", current.version);
    if store
        .setting(DEFAULT_KEY)
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some(&current.version)
    {
        store
            .set_setting(DEFAULT_KEY, &previous)
            .map_err(|e| e.to_string())?;
        say!("The default PHP moves back to {previous}.");
    }
    // Out of use first, then the pools switch back, then the folder goes.
    store
        .remove_php(&current.version)
        .map_err(|e| e.to_string())?;
    store
        .remove_setting(&rollback_key(&minor))
        .map_err(|e| e.to_string())?;
    crate::web::apply_site_change(&store)?;
    crate::ext::forget(&current)?;
    remove_build_dir(&current.dir);
    say!(
        "Rolled back to PHP {previous}. The {} download stays cached: `kiln php update {minor}` installs it again.",
        current.version
    );
    Ok(())
}

/// Deletes a PHP folder that is out of use. A worker that is still exiting can hold its files
/// for a moment on Windows: retried for a few seconds, else marked incomplete so nothing uses
/// it, and replaced by the next install of that version.
fn remove_build_dir(dir: &std::path::Path) {
    for attempt in 0..10 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) if attempt < 9 => std::thread::sleep(std::time::Duration::from_secs(1)),
            Err(e) => {
                let _ = std::fs::write(
                    dir.join(kiln_php::install::INCOMPLETE_MARKER),
                    b"kiln: rolled back; remove this folder\n",
                );
                say!(
                    "Note: {} couldn't be deleted yet ({e}); it is out of use, and `kiln uninstall` or the next install of that version removes it.",
                    dir.display()
                );
            }
        }
    }
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
    say!("PHP {version} now serves sites.");
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
    say!("{count} php-cgi worker(s) per pool.");
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
    let Some(site) = sites.sites.iter().find(|s| s.host.as_str() == host) else {
        return Err(format!(
            "{host} isn't a linked or parked site (see `kiln sites`)"
        ));
    };
    if let Some(alias_of) = &site.alias_of {
        return Err(format!(
            "{host} is an alias of {alias_of} (from its kiln.toml); pin {alias_of} instead"
        ));
    }
    if let Some(wanted) = site.manifest.as_ref().and_then(|m| m.php.as_deref()) {
        say!(
            "{host}'s kiln.toml asks for PHP {wanted}; this pin overrides it on this machine (`kiln unisolate` goes back to it)."
        );
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
    say!(
        "{host} now uses PHP {} (pinned to {}).",
        php.version,
        version.trim()
    );
    if version.trim().split('.').count() == 2 {
        say!("It follows the newest installed {}.x.", version.trim());
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
    say!(
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
    let asks = if here.php.from_project {
        "asks in its kiln.toml for"
    } else {
        "is pinned to"
    };
    let why = match (&here.site, &here.php.pinned, here.php.fell_back) {
        (Some(site), Some(pin), false) => format!("{site} {asks} {pin}"),
        (Some(site), Some(pin), true) => {
            format!("{site} {asks} {pin}, which isn't installed, so the default")
        }
        (Some(site), None, _) => format!("{site} uses the default"),
        (None, _, _) => "not inside a site, so the default".to_owned(),
    };
    match &here.php.php {
        Some(php) => {
            say!("php      → PHP {} ({why})", php.version);
            say!("           {}", php.dir.join(kiln_php::PHP_EXE).display());
        }
        None => say!("php      → none: no PHP installed by Kiln (`kiln php install 8.4`)"),
    }
    let phar = local_dir()?.join(kiln_runtime::shim::COMPOSER_PHAR);
    if phar.is_file() {
        say!("composer → {} with that PHP", phar.display());
    } else {
        say!("composer → not installed (`kiln composer install`)");
    }
    which_node(&store, &dir)?;
    let shims = local_dir()?.join(kiln_runtime::shim::SHIMS_DIR);
    let has = |tool: kiln_runtime::shim::Tool| shims.join(tool.file_name()).is_file();
    if !has(kiln_runtime::shim::Tool::Php) {
        say!("The `php`, `composer` and `node` commands use this after `kiln shims install`.");
    } else if !has(kiln_runtime::shim::Tool::Node) {
        say!(
            "Run `kiln shims install` again to add the `node`, `npm`, `npx` and `corepack` shims."
        );
    }
    Ok(())
}

/// The `node` line of `kiln which` ([ADR 0028] §5).
///
/// [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md
fn which_node(store: &StateStore, dir: &std::path::Path) -> Result<(), String> {
    use kiln_runtime::node::{DEFAULT_KEY, NODE_DIR, node_for_dir};
    let installed = kiln_node::installed(&local_dir()?.join(NODE_DIR));
    let default = store.setting(DEFAULT_KEY).map_err(|e| e.to_string())?;
    let here = node_for_dir(&installed, default.as_deref(), dir);
    let asks = here
        .wanted
        .as_ref()
        .map(|w| format!("{} asks for {}", w.file.display(), w.text));
    let Some(node) = &here.node else {
        say!(
            "node     → none from Kiln, so the shims run the next `node` on PATH (`kiln node install lts`){}",
            asks.map(|a| format!("; {a}")).unwrap_or_default()
        );
        return Ok(());
    };
    let why = match (asks, here.fell_back) {
        (Some(asks), false) => asks,
        (Some(asks), true) => format!("{asks}, which isn't installed, so the default"),
        (None, _) => "no kiln.toml, .nvmrc or .node-version asks, so the default".to_owned(),
    };
    say!("node     → Node {} ({why})", node.version);
    say!("           {}", node.node_exe().display());
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
    say!("Composer {version}, SHA-256 {}", expected.hex());
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
    say!(
        "Verified and installed Composer {version} as {}.",
        dest.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(version: &str) -> PhpRecord {
        PhpRecord {
            version: version.into(),
            dir: PathBuf::from(format!("/php/{version}")),
            archive: String::new(),
            sha256: String::new(),
        }
    }

    #[test]
    fn rollback_picks_the_recorded_update() {
        let mut store = StateStore::open_in_memory().unwrap();
        let installed = [
            record("8.3.35"),
            record("8.3.36"),
            record("8.4.26"),
            record("8.4.27"),
        ];
        let err = recorded_update(&store, &installed, None).unwrap_err();
        assert!(err.contains("nothing to roll back"), "{err}");

        store.set_setting(&rollback_key("8.4"), "8.4.26").unwrap();
        let only = recorded_update(&store, &installed, None).unwrap();
        assert_eq!(only, ("8.4".to_owned(), "8.4.26".to_owned()));
        assert_eq!(
            recorded_update(&store, &installed, Some("8.4.27")).unwrap(),
            only,
            "an exact version names its line"
        );
        assert!(recorded_update(&store, &installed, Some("8.3")).is_err());

        store.set_setting(&rollback_key("8.3"), "8.3.35").unwrap();
        let err = recorded_update(&store, &installed, None).unwrap_err();
        assert!(err.contains("name one"), "{err}");
        assert_eq!(
            recorded_update(&store, &installed, Some("8.3")).unwrap().1,
            "8.3.35"
        );
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(version_numbers("8.4.10") > version_numbers("8.4.9"));
        assert_eq!(minor_of("8.4.27").unwrap(), "8.4");
        assert!(minor_of("8").is_err());
    }
}
