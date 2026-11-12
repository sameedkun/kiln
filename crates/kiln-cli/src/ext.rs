//! `kiln php ext list|on|off` ([ADR 0030]): the extensions a PHP build has, and turning them
//! on and off in its `php.ini` as journaled operations.
//!
//! [ADR 0030]: ../../../docs/adr/0030-php-extension-toggles.md

use std::path::{Path, PathBuf};
use std::process::Command;

use kiln_ops::{Journal, OpState, Operation};
use kiln_platform::php_ext::{PHP_EXT_KIND, PhpExtOp, loads, normalise};
use kiln_state::{PhpRecord, StateStore};

/// How an extension comes to be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A file the build ships, loaded (or not) by Kiln's `php.ini`.
    Toggle,
    /// Compiled into PHP.
    BuiltIn,
    /// A system PHP's `.so`, loaded by the distribution's `conf.d`.
    Distribution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Extension {
    name: String,
    loaded: bool,
    kind: Kind,
}

/// Every extension of a build, by name: `shipped` are its loadable files, `loaded` what
/// `php -m` lists, `ini_loads` what Kiln's `php.ini` loads.
fn inventory(shipped: &[String], loaded: &[String], ini_loads: &[String]) -> Vec<Extension> {
    let mut all: Vec<Extension> = shipped
        .iter()
        .map(|name| {
            let loaded = loaded.contains(name);
            Extension {
                name: name.clone(),
                loaded,
                kind: if loaded && !ini_loads.contains(name) {
                    Kind::Distribution
                } else {
                    Kind::Toggle
                },
            }
        })
        .collect();
    for name in loaded.iter().filter(|n| !shipped.contains(n)) {
        all.push(Extension {
            name: name.clone(),
            loaded: true,
            kind: Kind::BuiltIn,
        });
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all.dedup_by(|a, b| a.name == b.name);
    all
}

/// The module names `php -m` prints, normalised: `Zend OPcache` is `opcache`.
fn parse_modules(stdout: &str) -> Vec<String> {
    let mut names: Vec<String> = stdout
        .lines()
        .skip_while(|l| l.trim() != "[PHP Modules]")
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .map(|l| {
            if l.eq_ignore_ascii_case("Zend OPcache") {
                "opcache".to_owned()
            } else {
                normalise(l)
            }
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Extension files in `dir`: `php_<name>.dll` on Windows, `<name>.so` elsewhere.
fn extension_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| {
            let suffix = if cfg!(windows) { "dll" } else { "so" };
            let prefixed = !cfg!(windows) || f.to_ascii_lowercase().starts_with("php_");
            prefixed
                && Path::new(f)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(suffix))
        })
        .map(|f| normalise(&f))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn is_system(record: &PhpRecord) -> bool {
    record.archive == kiln_php::linux::SYSTEM_ARCHIVE
}

/// Kiln's static Linux builds have every extension compiled in ([ADR 0022]).
///
/// [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
fn is_static(record: &PhpRecord) -> bool {
    !cfg!(windows) && !is_system(record)
}

fn ini_path(record: &PhpRecord) -> PathBuf {
    record.dir.join("php.ini")
}

/// The build's `php` with its own `php.ini`.
fn php(record: &PhpRecord) -> Command {
    let mut command = Command::new(record.dir.join(kiln_php::PHP_EXE));
    command.arg("-c").arg(ini_path(record));
    command
}

/// What `php -m` lists, and PHP's start-up warnings.
fn modules(record: &PhpRecord) -> Result<(Vec<String>, Vec<String>), String> {
    let output = php(record)
        .arg("-m")
        .output()
        .map_err(|e| format!("cannot run PHP {}: {e}", record.version))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let warnings: Vec<String> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|l| l.contains("Startup"))
        .map(|l| l.trim().to_owned())
        .collect();
    if !output.status.success() {
        return Err(format!(
            "PHP {} didn't start with {}: {}",
            record.version,
            ini_path(record).display(),
            stderr
                .lines()
                .chain(stdout.lines())
                .next()
                .unwrap_or("no output")
        ));
    }
    Ok((parse_modules(&stdout), warnings))
}

/// The build's loadable extension files.
fn shipped(record: &PhpRecord) -> Result<Vec<String>, String> {
    if cfg!(windows) {
        return Ok(extension_files(&record.dir.join("ext")));
    }
    if !is_system(record) {
        return Ok(Vec::new());
    }
    let output = php(record)
        .args(["-r", "echo ini_get('extension_dir');"])
        .output()
        .map_err(|e| format!("cannot run PHP {}: {e}", record.version))?;
    let dir = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(extension_files(Path::new(&dir)))
}

fn inspect(record: &PhpRecord) -> Result<Vec<Extension>, String> {
    let ini = ini_path(record);
    let text = std::fs::read(&ini).map_err(|e| format!("cannot read {}: {e}", ini.display()))?;
    let (loaded, _) = modules(record)?;
    Ok(inventory(&shipped(record)?, &loaded, &loads(&text)))
}

fn open_store() -> Result<StateStore, String> {
    crate::web::open_store()
}

/// The default PHP, or the installed build `spec` names.
fn target(store: &StateStore, spec: Option<&str>) -> Result<PhpRecord, String> {
    let installed = kiln_runtime::installed(store).map_err(|e| e.to_string())?;
    match spec {
        Some(spec) => kiln_runtime::pick(&installed, spec)
            .cloned()
            .ok_or_else(|| format!("PHP {spec} isn't installed by Kiln (see `kiln php list`)")),
        None => kiln_runtime::default_php(store, &installed)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "No PHP installed by Kiln yet. See `kiln php available`.".to_owned()),
    }
}

fn minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// `names` on lines of at most `width` characters, indented.
fn wrapped(names: &[&str], width: usize) -> String {
    let mut out = String::new();
    let mut line = String::from(" ");
    for name in names {
        if line.len() + name.len() + 1 > width && line.trim() != "" {
            out.push_str(&line);
            out.push('\n');
            line = String::from(" ");
        }
        line.push(' ');
        line.push_str(name);
    }
    out.push_str(&line);
    out
}

fn static_build(record: &PhpRecord) -> String {
    format!(
        "PHP {} is one of Kiln's static builds: its extensions are compiled in, so none can be turned on or off. For loadable extensions, install your distribution's PHP and use `kiln php install {} --system`.",
        record.version,
        minor(&record.version)
    )
}

/// `kiln php ext list [--php X]`.
pub fn list(spec: Option<&str>) -> Result<(), String> {
    let record = target(&open_store()?, spec)?;
    let all = inspect(&record)?;
    say!("PHP {} ({})", record.version, ini_path(&record).display());
    let names = |kind: Kind| -> Vec<&str> {
        all.iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.name.as_str())
            .collect()
    };
    let toggles: Vec<&Extension> = all.iter().filter(|e| e.kind == Kind::Toggle).collect();
    if is_static(&record) {
        say!("{}", static_build(&record));
    } else if toggles.is_empty() {
        say!("No extension files to turn on or off.");
    } else {
        say!("Can be turned on or off (`kiln php ext on|off <name>`):");
        let width = toggles.iter().map(|e| e.name.len()).max().unwrap_or(0);
        for e in toggles {
            say!(
                "  {:<width$}  {}",
                e.name,
                if e.loaded { "on" } else { "off" }
            );
        }
    }
    let built_in = names(Kind::BuiltIn);
    if !built_in.is_empty() {
        say!("Built in, always on:\n{}", wrapped(&built_in, 90));
    }
    let distribution = names(Kind::Distribution);
    if !distribution.is_empty() {
        say!(
            "Loaded by your distribution's configuration (`sudo phpdismod -v {} <name>` turns one off):\n{}",
            minor(&record.version),
            wrapped(&distribution, 90)
        );
    }
    Ok(())
}

fn word(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// The toggle `name` needs, or `None` when it's already as asked.
fn plan_one(
    record: &PhpRecord,
    all: &[Extension],
    name: &str,
    on: bool,
) -> Result<Option<PhpExtOp>, String> {
    let version = &record.version;
    let Some(ext) = all.iter().find(|e| e.name == name) else {
        return Err(format!(
            "PHP {version} has no extension `{name}` (see `kiln php ext list`)"
        ));
    };
    match ext.kind {
        Kind::BuiltIn if on => {
            say!("{name} is built into PHP {version}: always on.");
            return Ok(None);
        }
        Kind::BuiltIn => {
            return Err(format!(
                "{name} is built into PHP {version} and can't be turned off"
            ));
        }
        Kind::Distribution if !on => {
            return Err(format!(
                "{name} is loaded by your distribution's configuration in /etc, which Kiln doesn't change; turn it off with `sudo phpdismod -v {} {name}`",
                minor(version)
            ));
        }
        Kind::Distribution | Kind::Toggle => {}
    }
    if ext.loaded == on {
        say!("{name} is already {} for PHP {version}.", word(on));
        return Ok(None);
    }
    Ok(Some(PhpExtOp::new(ini_path(record), version, name, on)))
}

/// `kiln php ext on|off <name>… [--php X]` ([ADR 0030] §4–5).
///
/// [ADR 0030]: ../../../docs/adr/0030-php-extension-toggles.md
pub fn toggle(names: &[String], spec: Option<&str>, on: bool) -> Result<(), String> {
    let record = target(&open_store()?, spec)?;
    if is_static(&record) {
        return Err(static_build(&record));
    }
    let all = inspect(&record)?;
    let mut ops: Vec<Box<dyn Operation>> = Vec::new();
    let mut wanted: Vec<String> = Vec::new();
    for name in names.iter().map(|n| normalise(n)) {
        if wanted.contains(&name) {
            continue;
        }
        if let Some(op) = plan_one(&record, &all, &name, on)? {
            ops.push(Box::new(op));
            wanted.push(name);
        }
    }
    if ops.is_empty() {
        return Ok(());
    }
    let ids = crate::apply::apply_quietly(&ops)?;
    let (loaded, warnings) = modules(&record)?;
    let (done, failed): (Vec<_>, Vec<_>) = ids
        .into_iter()
        .zip(wanted)
        .partition(|(_, name)| loaded.contains(name) == on);
    if !failed.is_empty() {
        crate::apply::revert_quietly(&failed.iter().map(|(id, _)| *id).collect::<Vec<_>>())?;
    }
    for (id, name) in &done {
        say!(
            "{name} is now {} for PHP {} (undo with `kiln undo {id}`).",
            word(on),
            record.version
        );
    }
    if !done.is_empty() {
        crate::web::restart_php(&open_store()?, &record.version)?;
    }
    if failed.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = failed.iter().map(|(_, n)| n.as_str()).collect();
    let mut message = format!(
        "PHP {} still has {} {} after the change, so it was undone",
        record.version,
        names.join(", "),
        if on { "not loaded" } else { "loaded" }
    );
    for warning in warnings {
        message.push_str("\n  ");
        message.push_str(&warning);
    }
    Err(message)
}

/// Toggles in effect on the `php.ini` of `record`, oldest first: (journal id, name, on).
fn toggles_on(record: &PhpRecord) -> Result<Vec<(u64, String, bool)>, String> {
    let ini = ini_path(record).to_string_lossy().into_owned();
    let entries = open_store()?.entries().map_err(|e| e.to_string())?;
    Ok(entries
        .into_iter()
        .filter(|e| {
            e.kind == PHP_EXT_KIND
                && e.state == OpState::Applied
                && e.params["ini"].as_str() == Some(ini.as_str())
        })
        .filter_map(|e| {
            let name = e.params["name"].as_str()?.to_owned();
            Some((e.id, name, e.params["on"].as_bool()?))
        })
        .collect())
}

/// The last state each extension was toggled to, in first-toggled order.
fn final_states(toggles: &[(u64, String, bool)]) -> Vec<(String, bool)> {
    let mut states: Vec<(String, bool)> = Vec::new();
    for (_, name, on) in toggles {
        match states.iter_mut().find(|(n, _)| n == name) {
            Some(state) => state.1 = *on,
            None => states.push((name.clone(), *on)),
        }
    }
    states
}

/// `kiln php update` ([ADR 0030] §6): the toggles in effect on `old` again on `new`.
///
/// [ADR 0030]: ../../../docs/adr/0030-php-extension-toggles.md
pub fn carry_over(old: &PhpRecord, new: &PhpRecord) -> Result<(), String> {
    let states = final_states(&toggles_on(old)?);
    if states.is_empty() {
        return Ok(());
    }
    let all = inspect(new)?;
    let mut ops: Vec<Box<dyn Operation>> = Vec::new();
    let mut carried = Vec::new();
    for (name, on) in states {
        let Some(ext) = all.iter().find(|e| e.name == name) else {
            say!(
                "PHP {} has no {name} extension, so it isn't turned {} there.",
                new.version,
                word(on)
            );
            continue;
        };
        if ext.kind != Kind::Toggle || ext.loaded == on {
            continue;
        }
        ops.push(Box::new(PhpExtOp::new(
            ini_path(new),
            &new.version,
            &name,
            on,
        )));
        carried.push(format!("{name} {}", word(on)));
    }
    if ops.is_empty() {
        return Ok(());
    }
    crate::apply::apply_quietly(&ops)?;
    say!(
        "Extensions carried over from PHP {}: {}.",
        old.version,
        carried.join(", ")
    );
    Ok(())
}

/// Undoes the toggles on `record`'s `php.ini` before its folder is deleted ([ADR 0030] §6).
///
/// [ADR 0030]: ../../../docs/adr/0030-php-extension-toggles.md
pub fn forget(record: &PhpRecord) -> Result<(), String> {
    let mut ids: Vec<u64> = toggles_on(record)?.into_iter().map(|(id, ..)| id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    ids.reverse();
    crate::apply::revert_quietly(&ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn php_m_output_is_read_as_extension_names() {
        let stdout = "PHP Warning:  PHP Startup: Unable to load dynamic library 'oci8'\n[PHP Modules]\nCore\ncurl\nPDO\nZend OPcache\n\n[Zend Modules]\nZend OPcache\nXdebug\n";
        assert_eq!(
            parse_modules(stdout),
            ["core", "curl", "opcache", "pdo", "xdebug"]
        );
    }

    #[test]
    fn each_extension_is_toggleable_built_in_or_the_distributions() {
        let all = inventory(
            &names(&["curl", "xsl", "zip"]),
            &names(&["core", "curl", "zip"]),
            &names(&["curl"]),
        );
        let find = |n: &str| all.iter().find(|e| e.name == n).unwrap();
        assert_eq!(find("curl").kind, Kind::Toggle);
        assert!(find("curl").loaded);
        assert_eq!(find("xsl").kind, Kind::Toggle);
        assert!(!find("xsl").loaded);
        assert_eq!(
            find("zip").kind,
            Kind::Distribution,
            "loaded, but not by Kiln"
        );
        assert_eq!(find("core").kind, Kind::BuiltIn);
        assert_eq!(
            all.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["core", "curl", "xsl", "zip"]
        );
    }

    #[test]
    fn the_last_toggle_of_each_extension_is_carried_over() {
        let toggles = vec![
            (3, "xsl".to_owned(), true),
            (4, "zip".to_owned(), false),
            (5, "xsl".to_owned(), false),
        ];
        assert_eq!(
            final_states(&toggles),
            [("xsl".to_owned(), false), ("zip".to_owned(), false)]
        );
    }

    #[test]
    fn long_name_lists_wrap() {
        let text = wrapped(&["aaaa", "bbbb", "cccc"], 12);
        assert_eq!(text, "  aaaa bbbb\n  cccc");
    }

    #[test]
    fn extension_files_are_found_by_platform_naming() {
        let dir = tempfile::tempdir().unwrap();
        for file in [
            "php_xsl.dll",
            "php_ZIP.dll",
            "libpq.dll",
            "gmp.so",
            "readme.txt",
        ] {
            std::fs::write(dir.path().join(file), b"").unwrap();
        }
        let expected: &[&str] = if cfg!(windows) {
            &["xsl", "zip"]
        } else {
            &["gmp"]
        };
        assert_eq!(extension_files(dir.path()), expected);
    }
}
