//! `kiln env use <instance>` ([ADR 0045]): points a project's `.env` at a Kiln service, as a
//! journaled operation that `kiln undo` reverses byte for byte.
//!
//! [ADR 0045]: ../../../docs/adr/0045-env-wiring.md

use std::path::{Path, PathBuf};

use kiln_ops::Operation;
use kiln_platform::env_file::{EnvKeysOp, value_of};

/// The database name a project gets by default: its folder's name, as a database name can
/// be written without quoting (`my-shop` becomes `my_shop`).
fn default_database(project: &Path) -> String {
    let name: String = project
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let name = name.trim_matches('_');
    if name.is_empty() {
        "laravel".to_owned()
    } else {
        name.chars().take(63).collect()
    }
}

/// A database name Kiln can put in SQL without quoting troubles: lowercase letters, digits
/// and `_`, starting with a letter or `_`.
fn check_database(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(format!(
            "`{name}` isn't a database name Kiln makes: use lowercase letters, digits and _"
        ))
    }
}

/// Whether the project's `config/database.php` has a `mariadb` connection (Laravel 11+).
fn has_mariadb_connection(project: &Path) -> bool {
    std::fs::read_to_string(project.join("config").join("database.php"))
        .is_ok_and(|text| text.contains("'mariadb'"))
}

/// What `kiln env use` sets: the instance's keys, the database's name after `DB_PORT`, and
/// Laravel's drivers when asked.
fn keys_for(
    provider: &dyn kiln_services::ServiceProvider,
    record: &kiln_state::ServiceRecord,
    project: &Path,
    database: Option<&str>,
    drivers: &[&str],
) -> Vec<(String, String)> {
    let mut keys: Vec<(String, String)> = provider
        .env_keys(&record.ports)
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    for (key, value) in &mut keys {
        if key == "DB_CONNECTION" && value == "mariadb" && !has_mariadb_connection(project) {
            "mysql".clone_into(value); // Laravel 10 and earlier: MariaDB through mysql
        }
    }
    if let Some(name) = database {
        let at = keys
            .iter()
            .position(|(k, _)| k == "DB_PORT")
            .map_or(keys.len(), |i| i + 1);
        keys.insert(at, ("DB_DATABASE".to_owned(), name.to_owned()));
    }
    for driver in drivers {
        keys.push(((*driver).to_owned(), "redis".to_owned()));
    }
    keys
}

/// The project folder (`project`, or the current one) and its `.env`, which must exist.
/// A key whose value is a secret: shown only when asked (ADR 0056 §2).
#[must_use]
pub fn is_secret(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    ["KEY", "SECRET", "PASSWORD", "TOKEN"]
        .iter()
        .any(|word| key.contains(word))
}

/// Every `.env` key and value Kiln would write for its instances, to tell its own apart.
fn kiln_values() -> Vec<(&'static str, String)> {
    let Ok(store) = crate::web::open_store() else {
        return Vec::new();
    };
    store
        .services()
        .unwrap_or_default()
        .iter()
        .filter_map(|r| {
            crate::service::provider_of(&r.kind)
                .ok()
                .map(|p| p.env_keys(&r.ports))
        })
        .flatten()
        .collect()
}

/// `kiln env show` ([ADR 0056] §2): a project's `.env`, in order.
///
/// [ADR 0056]: ../../../docs/adr/0056-services-page.md
pub fn show(project: Option<PathBuf>, reveal: bool) -> Result<(), String> {
    let (_, env) = project_env(project)?;
    let text =
        std::fs::read_to_string(&env).map_err(|e| format!("cannot read {}: {e}", env.display()))?;
    let settings = kiln_platform::env_file::settings(&text);
    if crate::out::json() {
        let ours = kiln_values();
        crate::out::data(serde_json::json!({
            "path": env,
            "keys": settings.iter().map(|(key, value)| serde_json::json!({
                "key": key,
                "value": value,
                "secret": is_secret(key),
                "kiln": ours.iter().any(|(k, v)| k == key && v == value),
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    say!("{}", env.display());
    for (key, value) in &settings {
        if is_secret(key) && !reveal && !value.is_empty() {
            say!("  {key}=•••••• (--reveal shows it)");
        } else {
            say!("  {key}={value}");
        }
    }
    Ok(())
}

fn project_env(project: Option<PathBuf>) -> Result<(PathBuf, PathBuf), String> {
    let project = match project {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let project = std::fs::canonicalize(&project)
        .map(crate::web::strip_verbatim)
        .map_err(|e| format!("cannot use {}: {e}", project.display()))?;
    let env = project.join(".env");
    if env.is_file() {
        return Ok((project, env));
    }
    Err(if project.join(".env.example").is_file() {
        format!(
            "{} has no .env yet: copy .env.example to .env (then `php artisan key:generate`) first",
            project.display()
        )
    } else {
        format!("{} has no .env file", project.display())
    })
}

/// What the project may still need after the change.
fn notes(kind: &str, project: &Path, drivers: bool) {
    if kind == "redis"
        && !project
            .join("vendor")
            .join("predis")
            .join("predis")
            .is_dir()
    {
        say!(
            "Laravel talks to Redis through the predis package here: run `composer require predis/predis` in the project."
        );
    }
    if drivers {
        say!("Cached config? Run `php artisan config:clear` so Laravel reads the new .env.");
    }
}

/// `kiln env use <instance>`. Returns the journal number of the change, if one was made.
#[allow(clippy::fn_params_excessive_bools)] // one per command-line switch
pub fn use_instance(
    name: &str,
    project: Option<PathBuf>,
    database: Option<String>,
    cache: bool,
    session: bool,
    queue: bool,
    yes: bool,
) -> Result<Option<u64>, String> {
    let store = crate::web::open_store()?;
    let record = crate::service::record_of(&store, name)?;
    let provider = crate::service::provider_of(&record.kind)?;
    drop(store);
    // A service that runs in a project wires that project by default (ADR 0047).
    let (project, env) = project_env(project.or_else(|| record.project.clone()))?;
    if provider.env_keys(&record.ports).is_empty() {
        return Err(format!(
            "{} has no .env keys for Kiln to set",
            provider.display_name()
        ));
    }
    let drivers: Vec<&str> = [
        (cache, "CACHE_STORE"),
        (session, "SESSION_DRIVER"),
        (queue, "QUEUE_CONNECTION"),
    ]
    .into_iter()
    .filter_map(|(on, key)| on.then_some(key))
    .collect();
    if !drivers.is_empty() && record.kind != "redis" {
        return Err("--cache, --session and --queue choose Redis; give a redis instance".into());
    }
    let dirs = crate::service::dirs_of(&record)?;
    let database = if provider.has_databases() {
        let name = database.unwrap_or_else(|| default_database(&project));
        check_database(&name)?;
        Some(name)
    } else {
        None
    };
    let keys = keys_for(provider, &record, &project, database.as_deref(), &drivers);
    let text =
        std::fs::read_to_string(&env).map_err(|e| format!("cannot read {}: {e}", env.display()))?;
    let unchanged = keys
        .iter()
        .all(|(k, v)| value_of(&text, k).as_deref() == Some(v.as_str()));

    let op: Box<dyn Operation> = Box::new(EnvKeysOp::new(env.clone(), keys));
    if unchanged {
        say!("{} already points at {name}.", env.display());
    } else {
        crate::apply::print_preview(&op.preview().map_err(|e| e.to_string())?, "(not set)");
        if !yes && !crate::apply::confirm(&format!("Point this project at {name}?")) {
            say!("Nothing was changed.");
            return Ok(None);
        }
    }
    if let Some(db) = &database {
        if !record.enabled {
            return Err(format!(
                "{name} is stopped, so its database can't be made: `kiln service start {name}` first"
            ));
        }
        match provider.create_database(&dirs, &record.ports, db) {
            Some(Ok(true)) => say!("Created the database {db} on {name}."),
            Some(Ok(false)) => say!("The database {db} is already on {name}."),
            Some(Err(e)) => return Err(format!("cannot make the database {db}: {e}")),
            None => {}
        }
    }
    if unchanged {
        return Ok(None);
    }
    let id = crate::apply::apply_quietly(&[op])?.first().copied();
    say!(
        "{} now points at {name}{}.",
        env.display(),
        id.map(|id| format!(" (undo with `kiln undo {id}`)"))
            .unwrap_or_default()
    );
    notes(&record.kind, &project, !drivers.is_empty());
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_named_for_what_they_hold() {
        for secret in [
            "APP_KEY",
            "DB_PASSWORD",
            "STRIPE_SECRET",
            "GITHUB_TOKEN",
            "aws_secret_access_key",
        ] {
            assert!(is_secret(secret), "{secret}");
        }
        for plain in ["APP_NAME", "DB_PORT", "MAIL_HOST", "APP_URL"] {
            assert!(!is_secret(plain), "{plain}");
        }
    }

    #[test]
    fn a_projects_database_is_named_after_its_folder() {
        assert_eq!(
            default_database(Path::new("/sites/My-Shop.v2")),
            "my_shop_v2"
        );
        assert_eq!(default_database(Path::new("/sites/blog")), "blog");
        assert_eq!(default_database(Path::new("/")), "laravel");
        assert!(check_database("my_shop").is_ok());
        for bad in ["", "2shop", "my-shop", "x;drop", "Shop"] {
            assert!(check_database(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_database_goes_after_the_port_and_drivers_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let mysql = kiln_services::provider("mariadb").unwrap();
        let record = kiln_state::ServiceRecord {
            name: "mariadb-11.8".into(),
            kind: "mariadb".into(),
            version: "11.8.9".into(),
            ports: [("mysql".to_owned(), 3307)].into(),
            enabled: true,
            project: None,
        };
        let keys = keys_for(mysql, &record, dir.path(), Some("shop"), &[]);
        let names: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "DB_CONNECTION",
                "DB_HOST",
                "DB_PORT",
                "DB_DATABASE",
                "DB_USERNAME",
                "DB_PASSWORD"
            ]
        );
        // No `mariadb` connection in this project (no config at all): mysql.
        assert_eq!(keys[0].1, "mysql");
        std::fs::create_dir(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config").join("database.php"),
            "'connections' => ['mariadb' => []]",
        )
        .unwrap();
        assert_eq!(
            keys_for(mysql, &record, dir.path(), None, &[])[0].1,
            "mariadb"
        );

        let redis = kiln_services::provider("redis").unwrap();
        let record = kiln_state::ServiceRecord {
            kind: "redis".into(),
            ports: [("redis".to_owned(), 6380)].into(),
            ..record
        };
        let keys = keys_for(redis, &record, dir.path(), None, &["CACHE_STORE"]);
        assert_eq!(
            keys.last().unwrap(),
            &("CACHE_STORE".to_owned(), "redis".to_owned())
        );
        assert!(keys.contains(&("REDIS_PORT".to_owned(), "6380".to_owned())));
    }
}
