//! `kiln db dump` and `kiln db restore` ([ADR 0049]): a database out of Kiln as a portable
//! `.sql` file, written by the service's own tool, and back in through its client.
//!
//! [ADR 0049]: ../../../docs/adr/0049-data-safety.md

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A database name as Kiln passes it: not empty, and not something a tool would read as an
/// option.
fn check_name(database: Option<&str>) -> Result<(), String> {
    match database {
        Some(name) if name.is_empty() || name.starts_with('-') => {
            Err(format!("`{name}` isn't a database name"))
        }
        _ => Ok(()),
    }
}

/// A database name Kiln puts into SQL itself (create, drop): letters, digits, `_`, `$`, `-`,
/// not starting with `$` or `-`, at most 64 (ADR 0068 §2). No quote can get in.
fn strict_name(database: &str) -> Result<(), String> {
    let ok = (1..=64).contains(&database.len())
        && !database.starts_with(['$', '-'])
        && database
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '-'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "`{database}` can't be a database name here: use letters, digits, '_', '$' and '-' (at most 64)"
        ))
    }
}

/// `kiln db list <instance>`: its databases, their size, and (with --json) the sites whose
/// `.env` points at each (ADR 0068 §1).
pub fn list(name: &str) -> Result<(), String> {
    let (record, provider, dirs) = running(name)?;
    let databases = provider
        .list_databases(&dirs, &record.ports)
        .ok_or_else(|| format!("{} has no databases to list", provider.display_name()))??;
    let users = crate::service::database_users(&record);
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "databases": databases.iter().map(|(db, bytes)| serde_json::json!({
                "name": db,
                "bytes": bytes,
                "used_by": users.get(db).cloned().unwrap_or_default(),
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if databases.is_empty() {
        say!("{name} has no databases of its own yet.");
    }
    for (db, bytes) in &databases {
        let sites = users.get(db).map(|s| format!("  used by {}", s.join(", ")));
        say!(
            "{db}  {}{}",
            crate::uninstall::mb(*bytes),
            sites.unwrap_or_default()
        );
    }
    Ok(())
}

/// `kiln db create <instance> <database>`.
pub fn create(name: &str, database: &str) -> Result<(), String> {
    strict_name(database)?;
    let (record, provider, dirs) = running(name)?;
    match provider.create_database(&dirs, &record.ports, database) {
        Some(Ok(true)) => {
            say!("Created the database {database} on {name}.");
            Ok(())
        }
        Some(Ok(false)) => Err(format!("{name} already has a database called {database}")),
        Some(Err(e)) => Err(format!("cannot make the database {database}: {e}")),
        None => Err(format!("{} has no databases", provider.display_name())),
    }
}

/// `kiln db drop <instance> <database>`: after asking, as it can't be undone.
pub fn drop(name: &str, database: &str) -> Result<(), String> {
    strict_name(database)?;
    let (record, provider, dirs) = running(name)?;
    let found = provider
        .list_databases(&dirs, &record.ports)
        .ok_or_else(|| format!("{} has no databases", provider.display_name()))??;
    let Some((_, bytes)) = found.iter().find(|(db, _)| db == database) else {
        return Err(format!("{name} has no database called {database}"));
    };
    let users = crate::service::database_users(&record);
    if let Some(sites) = users.get(database) {
        say!("{database} is used by {}.", sites.join(", "));
    }
    say!(
        "This deletes {database} on {name} ({}) and everything in it. It can't be undone; \
         `kiln db dump {name} {database}` keeps a copy first.",
        crate::uninstall::mb(*bytes)
    );
    if !crate::out::confirm(&format!("Delete the database {database}?")) {
        say!("Nothing was changed.");
        return Ok(());
    }
    provider
        .drop_database(&dirs, &record.ports, database)
        .ok_or_else(|| format!("{} has no databases", provider.display_name()))?
        .map_err(|e| format!("cannot delete {database}: {e}"))?;
    say!("Deleted the database {database} on {name}.");
    Ok(())
}

/// `<instance>-<database or all>-<date and time>.sql`, in the current folder.
fn default_output(instance: &str, database: Option<&str>) -> Result<PathBuf, String> {
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!(
        "{}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    Ok(std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join(format!(
            "{instance}-{}-{stamp}.sql",
            database.unwrap_or("all")
        )))
}

/// Runs `launch`'s program with standard input and output given, without a console window;
/// on failure, the error is the last lines it printed.
fn run(launch: &kiln_services::Launch, stdin: Stdio, stdout: Stdio) -> Result<(), String> {
    let mut command = Command::new(&launch.program);
    command
        .args(&launch.args)
        .envs(&launch.env)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot run {}: {e}", launch.program.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let errors = String::from_utf8_lossy(&output.stderr);
    let tail: Vec<&str> = errors.lines().filter(|l| !l.trim().is_empty()).collect();
    Err(format!(
        "{} failed ({}):\n{}",
        launch
            .program
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
        output.status,
        tail[tail.len().saturating_sub(6)..].join("\n")
    ))
}

/// The instance, its provider and folders, for a running instance.
fn running(
    name: &str,
) -> Result<
    (
        kiln_state::ServiceRecord,
        &'static dyn kiln_services::ServiceProvider,
        kiln_services::InstanceDirs,
    ),
    String,
> {
    let record = crate::service::record_of(&crate::web::open_store()?, name)?;
    let provider = crate::service::provider_of(&record.kind)?;
    if !record.enabled {
        return Err(format!(
            "{name} is stopped: `kiln service start {name}` first"
        ));
    }
    let dirs = crate::service::dirs_of(&record)?;
    Ok((record, provider, dirs))
}

/// `kiln db dump <instance> [database] [--output <file>]`.
pub fn dump(name: &str, database: Option<&str>, output: Option<PathBuf>) -> Result<(), String> {
    check_name(database)?;
    let (record, provider, dirs) = running(name)?;
    let launch = provider
        .dump(&dirs, &record.ports, database)
        .ok_or_else(|| {
            format!(
                "{} has no dump tool in Kiln; its data is in {}, and `kiln uninstall --keep-data` keeps it",
                provider.display_name(),
                dirs.data.display()
            )
        })?;
    let output = match output {
        Some(path) => path,
        None => default_output(name, database)?,
    };
    // Never over an existing file: it may be an earlier dump.
    let file =
        File::create_new(&output).map_err(|e| format!("cannot write {}: {e}", output.display()))?;
    if let Err(e) = run(&launch, Stdio::null(), Stdio::from(file)) {
        let _ = std::fs::remove_file(&output);
        return Err(e);
    }
    let size = std::fs::metadata(&output).map_or(0, |m| m.len());
    say!(
        "Wrote {} ({}) with {}'s own dump tool.",
        output.display(),
        crate::uninstall::mb(size),
        provider.display_name()
    );
    say!(
        "`kiln db restore {name} {}{}` loads it back.",
        output.display(),
        database.map(|d| format!(" {d}")).unwrap_or_default()
    );
    Ok(())
}

/// `kiln db restore <instance> <file> [database]`: into `database` (made if missing), or
/// into the server for a dump of every database.
pub fn restore(name: &str, file: &Path, database: Option<&str>) -> Result<(), String> {
    check_name(database)?;
    let (record, provider, dirs) = running(name)?;
    let input = File::open(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let launch = provider
        .restore(&dirs, &record.ports, database)
        .ok_or_else(|| format!("{} has no restore in Kiln", provider.display_name()))?;
    if let Some(db) = database {
        match provider.create_database(&dirs, &record.ports, db) {
            Some(Ok(true)) => say!("Created the database {db} on {name}."),
            Some(Ok(false)) | None => {}
            Some(Err(e)) => return Err(format!("cannot make the database {db}: {e}")),
        }
    }
    run(&launch, Stdio::from(input), Stdio::null())?;
    say!(
        "Loaded {} into {name}{}.",
        file.display(),
        database.map(|d| format!(" ({d})")).unwrap_or_default()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_put_into_sql_are_strict() {
        assert!(strict_name("shop").is_ok());
        assert!(strict_name("shop_test-2").is_ok());
        assert!(strict_name("").is_err());
        assert!(strict_name("-x").is_err());
        assert!(strict_name("a`; DROP").is_err());
        assert!(strict_name("a\"b").is_err());
        assert!(strict_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn names_that_look_like_options_are_refused() {
        assert!(check_name(None).is_ok());
        assert!(check_name(Some("shop")).is_ok());
        assert!(check_name(Some("--all-databases")).is_err());
        assert!(check_name(Some("")).is_err());
    }

    #[test]
    fn a_dump_is_named_after_the_instance_and_database() {
        let path = default_output("mysql-8.4", Some("shop")).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("mysql-8.4-shop-2"), "{name}");
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("sql"));
        let all = default_output("postgres-18", None).unwrap();
        assert!(all.to_string_lossy().contains("postgres-18-all-"));
    }
}
