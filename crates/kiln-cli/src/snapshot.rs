//! Database snapshots ([ADR 0079], [ADR 0086]): a database's state kept by Kiln, to roll back
//! to after a migration or a test run that went wrong. A snapshot is a dump by the server's own
//! tool, in the instance's own `snapshots` folder.
//!
//! Kiln also takes them itself: before a branch switch, migrations, a drop and a rollback, and
//! daily if asked. Only those automatic ones are pruned, to the newest `snapshots.keep`;
//! the user's own and kept ones stay.
//!
//! [ADR 0079]: ../../../docs/adr/0079-snapshots-and-tinker.md
//! [ADR 0086]: ../../../docs/adr/0086-automatic-snapshots.md

use std::path::{Path, PathBuf};

use serde_json::json;

/// Whether Kiln snapshots before risky steps (default on).
pub const AUTO_KEY: &str = "snapshots.auto";
/// Whether kilnd takes a daily snapshot of each database a site uses (default off).
pub const DAILY_KEY: &str = "snapshots.daily";
/// How many automatic snapshots of a database are kept (default 10).
pub const KEEP_KEY: &str = "snapshots.keep";
const KEEP_DEFAULT: usize = 10;

/// Why Kiln took a snapshot itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    BeforeSwitch,
    BeforeMigrate,
    BeforeDrop,
    BeforeRollback,
    Daily,
}

impl Reason {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::BeforeSwitch => "before-switch",
            Self::BeforeMigrate => "before-migrate",
            Self::BeforeDrop => "before-drop",
            Self::BeforeRollback => "before-rollback",
            Self::Daily => "daily",
        }
    }

    fn of(word: &str) -> Option<Self> {
        [
            Self::BeforeSwitch,
            Self::BeforeMigrate,
            Self::BeforeDrop,
            Self::BeforeRollback,
            Self::Daily,
        ]
        .into_iter()
        .find(|r| r.word() == word)
    }
}

/// Whose a snapshot is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The user's own (`kiln db snapshot`).
    Taken,
    Auto(Reason),
    Kept,
}

/// A snapshot's file name, read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Name<'a> {
    pub database: &'a str,
    /// `YYYYMMDD-HHMMSS`, UTC: sorts by time.
    pub stamp: &'a str,
    pub kind: Kind,
}

fn is_stamp(s: &str) -> bool {
    s.len() == 15 && s.as_bytes()[8] == b'-' && s.chars().filter(char::is_ascii_digit).count() == 14
}

/// `<database>--<stamp>[--<reason>|--kept].sql`, if it's one of Kiln's names.
#[must_use]
pub fn parse(file: &str) -> Option<Name<'_>> {
    let stem = file.strip_suffix(".sql")?;
    let (rest, last) = stem.rsplit_once("--")?;
    let (database, stamp, kind) = if is_stamp(last) {
        (rest, last, Kind::Taken)
    } else {
        let (database, stamp) = rest.rsplit_once("--")?;
        let kind = if last == "kept" {
            Kind::Kept
        } else {
            Kind::Auto(Reason::of(last)?)
        };
        (database, stamp, kind)
    };
    (is_stamp(stamp) && !database.is_empty()).then_some(Name {
        database,
        stamp,
        kind,
    })
}

fn stamp_of(at: time::OffsetDateTime) -> String {
    format!(
        "{}{:02}{:02}-{:02}{:02}{:02}",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    )
}

fn file_name(database: &str, kind: Kind) -> String {
    let stamp = stamp_of(time::OffsetDateTime::now_utc());
    match kind {
        Kind::Taken => format!("{database}--{stamp}.sql"),
        Kind::Auto(reason) => format!("{database}--{stamp}--{}.sql", reason.word()),
        Kind::Kept => format!("{database}--{stamp}--kept.sql"),
    }
}

/// The instance's snapshots folder.
fn folder(name: &str) -> Result<PathBuf, String> {
    let record = crate::service::record_of(&crate::web::open_store()?, name)?;
    Ok(crate::service::dirs_of(&record)?
        .instance()
        .join("snapshots"))
}

/// The snapshot files in `dir`.
fn files(dir: &Path) -> Vec<(String, u64)> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let file = e.file_name().to_string_lossy().into_owned();
                    parse(&file)?;
                    Some((file, e.metadata().ok()?.len()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The automatic snapshots of `database` beyond the newest `keep`: the ones to delete.
fn beyond(files: &[String], database: &str, keep: usize) -> Vec<String> {
    let mut auto: Vec<&String> = files
        .iter()
        .filter(|f| {
            parse(f).is_some_and(|n| n.database == database && matches!(n.kind, Kind::Auto(_)))
        })
        .collect();
    auto.sort_by_key(|f| std::cmp::Reverse(parse(f).map(|n| n.stamp.to_owned())));
    auto.into_iter().skip(keep).cloned().collect()
}

fn setting(key: &str) -> Option<String> {
    crate::web::open_store()
        .ok()
        .and_then(|s| s.setting(key).ok().flatten())
}

fn keep_count() -> usize {
    setting(KEEP_KEY)
        .and_then(|v| v.parse().ok())
        .filter(|n| (1..=100).contains(n))
        .unwrap_or(KEEP_DEFAULT)
}

/// Dumps `database` as a snapshot of `kind`; after an automatic one, prunes older automatic
/// ones of that database.
fn take(name: &str, database: &str, kind: Kind) -> Result<PathBuf, String> {
    let dir = folder(name)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let file = dir.join(file_name(database, kind));
    crate::dump::dump(name, Some(database), Some(file.clone()))?;
    if matches!(kind, Kind::Auto(_)) {
        let all: Vec<String> = files(&dir).into_iter().map(|(f, _)| f).collect();
        for old in beyond(&all, database, keep_count()) {
            let _ = std::fs::remove_file(dir.join(&old));
        }
    }
    Ok(file)
}

/// `kiln db snapshot <instance> <database>`.
pub fn snapshot(name: &str, database: &str) -> Result<(), String> {
    let file = take(name, database, Kind::Taken)?;
    say!("Snapshot of {database} kept as {}.", file.display());
    Ok(())
}

/// Whether Kiln snapshots before risky steps.
#[must_use]
pub fn auto_on() -> bool {
    setting(AUTO_KEY).is_none_or(|v| v != "off")
}

/// An automatic snapshot of `database` on `name`, said in a line; `None` when automatic
/// snapshots are off.
pub(crate) fn auto(name: &str, database: &str, reason: Reason) -> Option<Result<PathBuf, String>> {
    if !auto_on() {
        return None;
    }
    let taken = take(name, database, Kind::Auto(reason));
    if let Ok(file) = &taken {
        say!(
            "Kept a snapshot of {database} first ({}): `kiln db rollback {name} {}` goes back to it.",
            reason.word(),
            file.file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
    }
    Some(taken)
}

/// The instance (running, Kiln's) and database a project's `.env` uses, if any.
pub(crate) fn site_database(project: &Path) -> Option<(String, String)> {
    let env = std::fs::read_to_string(project.join(".env")).ok()?;
    let value = |k: &str| kiln_doctor::site::value(&env, k).filter(|v| !v.is_empty());
    let connection = value("DB_CONNECTION")?;
    let (kinds, default): (&[&str], u16) = match connection.as_str() {
        "mysql" | "mariadb" => (&["mysql", "mariadb"], 3306),
        "pgsql" => (&["postgres"], 5432),
        _ => return None,
    };
    let host = value("DB_HOST").unwrap_or_else(|| "127.0.0.1".into());
    if !matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return None;
    }
    let port: u16 = value("DB_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(default);
    let database = value("DB_DATABASE")?;
    let record = crate::web::open_store()
        .ok()?
        .services()
        .ok()?
        .into_iter()
        .find(|r| {
            r.enabled && kinds.contains(&r.kind.as_str()) && r.ports.values().any(|p| *p == port)
        })?;
    Some((record.name, database))
}

/// Before a step on a site's project: a snapshot of its database if it's on Kiln's, a warning
/// if that fails (the step goes on).
pub(crate) fn before(project: &Path, reason: Reason) {
    let Some((name, database)) = site_database(project) else {
        return;
    };
    if let Some(Err(e)) = auto(&name, &database, reason) {
        crate::out::warning(&format!(
            "couldn't keep a snapshot of {database} first ({e}); going on"
        ));
    }
}

/// `kiln db snapshots <instance>`: each snapshot, newest first.
pub fn list(name: &str) -> Result<(), String> {
    let dir = folder(name)?;
    let mut all = files(&dir);
    all.sort_by_key(|(f, _)| std::cmp::Reverse(parse(f).map(|n| n.stamp.to_owned())));
    if crate::out::json() {
        crate::out::data(json!({
            "folder": dir,
            "snapshots": all.iter().filter_map(|(file, bytes)| {
                let n = parse(file)?;
                let (kind, reason) = match n.kind {
                    Kind::Taken => ("taken", None),
                    Kind::Auto(r) => ("auto", Some(r.word())),
                    Kind::Kept => ("kept", None),
                };
                Some(json!({"file": file, "database": n.database, "bytes": bytes, "kind": kind, "reason": reason}))
            }).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if all.is_empty() {
        say!("{name} has no snapshots yet: `kiln db snapshot {name} <database>` takes one.");
    }
    for (file, bytes) in &all {
        say!("{file}  {}", crate::uninstall::mb(*bytes));
    }
    Ok(())
}

/// A snapshot file name given by a user: one of Kiln's, no folder in it.
fn checked<'a>(name: &str, file: &'a str) -> Result<Name<'a>, String> {
    parse(file)
        .filter(|_| !file.contains(['/', '\\']))
        .ok_or_else(|| {
            format!("{file:?} isn't one of Kiln's snapshots (see `kiln db snapshots {name}`)")
        })
}

/// `kiln db rollback <instance> <snapshot>`: the database back as the snapshot has it, after
/// asking (what it holds now is replaced; a snapshot of it is taken first).
pub fn rollback(name: &str, file: &str) -> Result<(), String> {
    let database = checked(name, file)?.database.to_owned();
    let path = folder(name)?.join(file);
    if !path.is_file() {
        return Err(format!("{name} has no snapshot {file}"));
    }
    say!(
        "This replaces everything in {database} on {name} with the snapshot {file}. Kiln keeps a snapshot of it as it is now first."
    );
    if !crate::out::confirm(&format!("Roll {database} back to {file}?")) {
        say!("Nothing was changed.");
        return Ok(());
    }
    // Always, whatever `snapshots.auto` says: the user was told it would be kept.
    take(name, &database, Kind::Auto(Reason::BeforeRollback))
        .map_err(|e| format!("couldn't keep a snapshot first, so nothing was rolled back: {e}"))?;
    crate::dump::replace(name, &database, &path)?;
    say!("{database} on {name} is back as of {file}.");
    Ok(())
}

/// `kiln db snapshot keep <instance> <snapshot>`: an automatic snapshot kept past the limit.
pub fn keep(name: &str, file: &str) -> Result<(), String> {
    let n = checked(name, file)?;
    let dir = folder(name)?;
    if !dir.join(file).is_file() {
        return Err(format!("{name} has no snapshot {file}"));
    }
    match n.kind {
        Kind::Auto(_) => {
            let kept = format!("{}--{}--kept.sql", n.database, n.stamp);
            kiln_sys::rename_replace(&dir.join(file), &dir.join(&kept))
                .map_err(|e| format!("cannot keep {file}: {e}"))?;
            say!("Kept {kept}: Kiln won't delete it.");
        }
        Kind::Taken | Kind::Kept => {
            say!("{file} is already kept: Kiln only deletes its own automatic snapshots.");
        }
    }
    Ok(())
}

/// `kiln db snapshot-due` (kilnd, hourly, while `snapshots.daily` is on): a daily snapshot
/// of each database a site uses on a running instance, when its newest is a day old.
pub fn due() -> Result<(), String> {
    let store = crate::web::open_store()?;
    let records = store.services().map_err(|e| e.to_string())?;
    drop(store);
    let day_ago = stamp_of(time::OffsetDateTime::now_utc() - time::Duration::days(1));
    let mut taken = 0;
    for record in records.iter().filter(|r| r.enabled) {
        let Ok(provider) = crate::service::provider_of(&record.kind) else {
            continue;
        };
        if !provider.has_databases() {
            continue;
        }
        let dir = folder(&record.name)?;
        let all = files(&dir);
        for database in crate::service::database_users(record).into_keys() {
            let newest = all
                .iter()
                .filter_map(|(f, _)| {
                    parse(f)
                        .filter(|n| n.database == database)
                        .map(|n| n.stamp.to_owned())
                })
                .max();
            if newest.is_some_and(|s| s > day_ago) {
                continue;
            }
            match take(&record.name, &database, Kind::Auto(Reason::Daily)) {
                Ok(file) => {
                    taken += 1;
                    say!(
                        "Daily snapshot of {database} on {}: {}",
                        record.name,
                        file.display()
                    );
                }
                Err(e) => crate::out::warning(&format!(
                    "daily snapshot of {database} on {}: {e}",
                    record.name
                )),
            }
        }
    }
    if taken == 0 {
        say!("No daily snapshot was due.");
    }
    Ok(())
}

/// `kiln db snapshot-settings [--auto on|off] [--daily on|off] [--keep <n>]`.
pub fn settings(
    auto: Option<&str>,
    daily: Option<&str>,
    keep: Option<usize>,
) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let on_off = |v: &str| match v {
        "on" | "off" => Ok(v.to_owned()),
        other => Err(format!("{other:?}: say on or off")),
    };
    if let Some(v) = auto {
        store
            .set_setting(AUTO_KEY, &on_off(v)?)
            .map_err(|e| e.to_string())?;
    }
    if let Some(v) = daily {
        store
            .set_setting(DAILY_KEY, &on_off(v)?)
            .map_err(|e| e.to_string())?;
    }
    if let Some(n) = keep {
        if !(1..=100).contains(&n) {
            return Err("--keep takes 1 to 100".into());
        }
        store
            .set_setting(KEEP_KEY, &n.to_string())
            .map_err(|e| e.to_string())?;
    }
    drop(store);
    let (auto, daily, keep) = (
        auto_on(),
        setting(DAILY_KEY).as_deref() == Some("on"),
        keep_count(),
    );
    if crate::out::json() {
        crate::out::data(json!({ "auto": auto, "daily": daily, "keep": keep }));
        return Ok(());
    }
    say!(
        "Before a branch switch, migrations, a drop or a rollback: {}.",
        if auto {
            "a snapshot first"
        } else {
            "no snapshot (off)"
        }
    );
    say!(
        "Daily snapshots of each database a site uses: {}.",
        if daily { "on" } else { "off" }
    );
    say!("Automatic snapshots kept per database: {keep} (yours and kept ones are never deleted).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_names_say_their_database_time_and_kind() {
        let name = file_name("shop_test", Kind::Taken);
        assert!(name.starts_with("shop_test--2"), "{name}");
        assert_eq!(parse(&name).map(|n| n.database), Some("shop_test"));
        assert_eq!(
            parse("shop--20261114-101500.sql").map(|n| n.database),
            Some("shop")
        );
        assert_eq!(
            parse("a--b--20261114-101500.sql").map(|n| n.database),
            Some("a--b")
        );
        assert_eq!(parse("shop.sql").map(|n| n.database), None);
        assert_eq!(parse("shop--2026.sql").map(|n| n.database), None);
        assert_eq!(parse("--20261114-101500.sql").map(|n| n.database), None);
        let auto = parse("a--b--20261114-101500--before-migrate.sql").unwrap();
        assert_eq!(
            (auto.database, auto.stamp, auto.kind),
            ("a--b", "20261114-101500", Kind::Auto(Reason::BeforeMigrate))
        );
        assert_eq!(
            parse("shop--20261114-101500--kept.sql").unwrap().kind,
            Kind::Kept
        );
        assert_eq!(parse("shop--20261114-101500--whatever.sql"), None);
        let made = file_name("shop", Kind::Auto(Reason::Daily));
        assert_eq!(parse(&made).unwrap().kind, Kind::Auto(Reason::Daily));
    }

    #[test]
    fn only_the_oldest_automatic_snapshots_beyond_the_limit_go() {
        let files: Vec<String> = [
            "shop--20261101-000000--daily.sql",
            "shop--20261102-000000--before-migrate.sql",
            "shop--20261103-000000--daily.sql",
            "shop--20261104-000000--before-switch.sql",
            "shop--20261001-000000.sql",
            "shop--20261002-000000--kept.sql",
            "blog--20261101-000000--daily.sql",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            beyond(&files, "shop", 2),
            [
                "shop--20261102-000000--before-migrate.sql",
                "shop--20261101-000000--daily.sql"
            ]
        );
        assert_eq!(beyond(&files, "shop", 10), Vec::<String>::new());
        assert_eq!(beyond(&files, "blog", 1), Vec::<String>::new());
    }
}
