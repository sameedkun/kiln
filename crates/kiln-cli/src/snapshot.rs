//! Database snapshots ([ADR 0079]): a database's state kept by Kiln, to roll back to after a
//! migration or a test run that went wrong. A snapshot is a dump by the server's own tool, in
//! the instance's own `snapshots` folder.
//!
//! [ADR 0079]: ../../../docs/adr/0079-snapshots-and-tinker.md

use std::path::PathBuf;

use serde_json::json;

/// The instance's snapshots folder.
fn folder(name: &str) -> Result<PathBuf, String> {
    let record = crate::service::record_of(&crate::web::open_store()?, name)?;
    Ok(crate::service::dirs_of(&record)?
        .instance()
        .join("snapshots"))
}

/// `<database>--<YYYYMMDD-HHMMSS>.sql`, so a listing sorts by database, then time.
fn file_name(database: &str) -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{database}--{}{:02}{:02}-{:02}{:02}{:02}.sql",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

/// The database a snapshot file is of, if it's one of Kiln's names.
#[must_use]
pub fn database_of(file: &str) -> Option<&str> {
    let stem = file.strip_suffix(".sql")?;
    let (database, stamp) = stem.rsplit_once("--")?;
    (stamp.len() == 15
        && stamp.as_bytes()[8] == b'-'
        && stamp.chars().filter(char::is_ascii_digit).count() == 14
        && !database.is_empty())
    .then_some(database)
}

/// `kiln db snapshot <instance> <database>`.
pub fn snapshot(name: &str, database: &str) -> Result<(), String> {
    let dir = folder(name)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let file = dir.join(file_name(database));
    crate::dump::dump(name, Some(database), Some(file.clone()))?;
    say!("Snapshot of {database} kept as {}.", file.display());
    Ok(())
}

/// `kiln db snapshots <instance>`: each snapshot, newest first.
pub fn list(name: &str) -> Result<(), String> {
    let dir = folder(name)?;
    let mut all: Vec<(String, String, u64)> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let file = e.file_name().to_string_lossy().into_owned();
                    let database = database_of(&file)?.to_owned();
                    let bytes = e.metadata().ok()?.len();
                    Some((file, database, bytes))
                })
                .collect()
        })
        .unwrap_or_default();
    all.sort_by(|a, b| {
        b.0.rsplit_once("--")
            .map(|x| x.1)
            .cmp(&a.0.rsplit_once("--").map(|x| x.1))
    });
    if crate::out::json() {
        crate::out::data(json!({
            "folder": dir,
            "snapshots": all.iter().map(|(file, database, bytes)| json!({
                "file": file, "database": database, "bytes": bytes,
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if all.is_empty() {
        say!("{name} has no snapshots yet: `kiln db snapshot {name} <database>` takes one.");
    }
    for (file, _, bytes) in &all {
        say!("{file}  {}", crate::uninstall::mb(*bytes));
    }
    Ok(())
}

/// `kiln db rollback <instance> <snapshot>`: the database back as the snapshot has it, after
/// asking (what it holds now is replaced; a snapshot of it is taken first).
pub fn rollback(name: &str, file: &str) -> Result<(), String> {
    let database = database_of(file)
        .filter(|_| !file.contains(['/', '\\']))
        .ok_or_else(|| {
            format!("{file:?} isn't one of Kiln's snapshots (see `kiln db snapshots {name}`)")
        })?
        .to_owned();
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
    snapshot(name, &database)?;
    crate::dump::replace(name, &database, &path)?;
    say!("{database} on {name} is back as of {file}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_names_say_their_database_and_time() {
        let name = file_name("shop_test");
        assert!(name.starts_with("shop_test--2"), "{name}");
        assert_eq!(database_of(&name), Some("shop_test"));
        assert_eq!(database_of("shop--20261114-101500.sql"), Some("shop"));
        assert_eq!(database_of("a--b--20261114-101500.sql"), Some("a--b"));
        assert_eq!(database_of("shop.sql"), None);
        assert_eq!(database_of("shop--2026.sql"), None);
        assert_eq!(database_of("--20261114-101500.sql"), None);
    }
}
