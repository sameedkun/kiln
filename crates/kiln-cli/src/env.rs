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

/// The project folder (`project`, or the current one), as Kiln spells it.
fn project_dir(project: Option<PathBuf>) -> Result<PathBuf, String> {
    let project = match project {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    std::fs::canonicalize(&project)
        .map(crate::web::strip_verbatim)
        .map_err(|e| format!("cannot use {}: {e}", project.display()))
}

fn project_env(project: Option<PathBuf>) -> Result<(PathBuf, PathBuf), String> {
    let project = project_dir(project)?;
    let env = project.join(".env");
    if env.is_file() {
        return Ok((project, env));
    }
    Err(no_env(&project))
}

/// What `kiln env use` edits in a project ([ADR 0082] §1).
///
/// [ADR 0082]: ../../../docs/adr/0082-wordpress-config.md
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// A `.env` with Laravel's names.
    Laravel(PathBuf),
    /// Bedrock's `.env`, with `WordPress`'s names.
    Bedrock(PathBuf),
    /// `wp-config.php`'s constants.
    WpConfig(PathBuf),
}

impl Target {
    fn file(&self) -> &Path {
        match self {
            Self::Laravel(p) | Self::Bedrock(p) | Self::WpConfig(p) => p,
        }
    }
}

/// Roots' Bedrock (`web/wp-config.php`, settings in `.env`), as the site list's driver says,
/// so the app and the CLI agree.
fn is_bedrock(project: &Path) -> bool {
    kiln_sites::detect(project) == kiln_sites::Driver::WordPressBedrock
}

fn target(project: &Path) -> Result<Target, String> {
    let env = project.join(".env");
    if env.is_file() {
        return Ok(if is_bedrock(project) {
            Target::Bedrock(env)
        } else {
            Target::Laravel(env)
        });
    }
    let config = project.join("wp-config.php");
    if config.is_file() {
        return Ok(Target::WpConfig(config));
    }
    Err(no_env(project))
}

/// Why a project has nothing for Kiln to edit, and what to do.
fn no_env(project: &Path) -> String {
    // What to do depends on what the project is (P4D-07): Laravel's advice only for Laravel.
    let wordpress = project.join("wp-config.php").is_file() || project.join("wp-includes").is_dir();
    if wordpress && project.join("wp-config.php").is_file() {
        format!(
            "{} is a WordPress site: its settings are in wp-config.php, not a .env (`kiln env use` writes them there)",
            project.display()
        )
    } else if wordpress {
        format!(
            "{} is a WordPress site without wp-config.php yet: open it to run WordPress's installer, or copy wp-config-sample.php to wp-config.php",
            project.display()
        )
    } else if project.join(".env.example").is_file() && project.join("artisan").is_file() {
        format!(
            "{} has no .env yet: copy .env.example to .env (then `php artisan key:generate`) first",
            project.display()
        )
    } else if project.join(".env.example").is_file() {
        format!(
            "{} has no .env yet: copy .env.example to .env first",
            project.display()
        )
    } else {
        format!("{} has no .env file", project.display())
    }
}

/// `WordPress`'s names for an instance ([ADR 0082] §2): the database's four constants (the
/// port in `DB_HOST`), or the Redis Object Cache plugin's two. Bedrock gets no Redis.
///
/// [ADR 0082]: ../../../docs/adr/0082-wordpress-config.md
fn wordpress_keys(
    provider: &dyn kiln_services::ServiceProvider,
    record: &kiln_state::ServiceRecord,
    database: Option<&str>,
    bedrock: bool,
) -> Result<Vec<(String, String)>, String> {
    let laravel = provider.env_keys(&record.ports);
    let get = |key: &str| {
        laravel
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let pairs = |list: Vec<(&str, String)>| {
        list.into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<Vec<_>>()
    };
    match record.kind.as_str() {
        "mysql" | "mariadb" => Ok(pairs(vec![
            ("DB_NAME", database.unwrap_or_default().to_owned()),
            ("DB_USER", get("DB_USERNAME")),
            ("DB_PASSWORD", get("DB_PASSWORD")),
            ("DB_HOST", format!("{}:{}", get("DB_HOST"), get("DB_PORT"))),
        ])),
        "redis" if !bedrock => Ok(pairs(vec![
            ("WP_REDIS_HOST", get("REDIS_HOST")),
            ("WP_REDIS_PORT", get("REDIS_PORT")),
        ])),
        "mailpit" => Err(format!(
            "WordPress sends mail with PHP's mail(); an SMTP plugin (such as WP Mail SMTP) set to {} sends it to Mailpit",
            get("MAIL_HOST") + ":" + &get("MAIL_PORT")
        )),
        _ => Err(format!(
            "WordPress has no setting Kiln knows for {}; it uses MySQL or MariaDB{}",
            provider.display_name(),
            if bedrock {
                ""
            } else {
                ", and Redis through the Redis Object Cache plugin"
            }
        )),
    }
}

/// A server Kiln doesn't run logs in its own way (ADR 0091 §2): its user and password in
/// place of Kiln's defaults.
fn with_external_login(name: &str, mut keys: Vec<(String, String)>) -> Vec<(String, String)> {
    if let Some(e) = crate::external::get(name) {
        for (key, value) in &mut keys {
            match key.as_str() {
                "DB_USERNAME" | "DB_USER" => value.clone_from(&e.user),
                "DB_PASSWORD" => value.clone_from(&e.password),
                _ => {}
            }
        }
    }
    keys
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
    let project = project_dir(project.or_else(|| record.project.clone()))?;
    let target = target(&project)?;
    let env = target.file().to_path_buf();
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
    if !drivers.is_empty() && !matches!(target, Target::Laravel(_)) {
        return Err("--cache, --session and --queue are Laravel's settings".into());
    }
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
    let keys = match &target {
        Target::Laravel(_) => keys_for(provider, &record, &project, database.as_deref(), &drivers),
        Target::Bedrock(_) => wordpress_keys(provider, &record, database.as_deref(), true)?,
        Target::WpConfig(_) => wordpress_keys(provider, &record, database.as_deref(), false)?,
    };
    let keys = with_external_login(name, keys);
    let text =
        std::fs::read_to_string(&env).map_err(|e| format!("cannot read {}: {e}", env.display()))?;
    let current = |k: &str| match &target {
        Target::WpConfig(_) => kiln_platform::php_defines::value_of(&text, k),
        _ => value_of(&text, k),
    };
    let unchanged = keys
        .iter()
        .all(|(k, v)| current(k).as_deref() == Some(v.as_str()));

    let op: Box<dyn Operation> = match &target {
        Target::WpConfig(_) => Box::new(kiln_platform::php_defines::PhpDefinesOp::new(
            env.clone(),
            keys,
        )),
        _ => Box::new(EnvKeysOp::new(env.clone(), keys)),
    };
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
    match &target {
        Target::Laravel(_) => notes(&record.kind, &project, !drivers.is_empty()),
        Target::WpConfig(_) if record.kind == "redis" => say!(
            "WordPress uses these with the Redis Object Cache plugin: install and enable it in WordPress (Kiln doesn't)."
        ),
        _ => {}
    }
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
    fn env_use_edits_what_the_project_has() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        assert!(target(p).unwrap_err().contains("has no .env file"));
        std::fs::create_dir(p.join("wp-includes")).unwrap();
        assert!(target(p).unwrap_err().contains("without wp-config.php yet"));
        std::fs::write(p.join("wp-config.php"), "<?php").unwrap();
        assert_eq!(target(p), Ok(Target::WpConfig(p.join("wp-config.php"))));
        std::fs::write(p.join(".env"), "").unwrap();
        assert_eq!(target(p), Ok(Target::Laravel(p.join(".env"))));
        // Bedrock as a fresh clone has it (no web/wp until Composer runs), and as the site
        // list's driver sees it.
        std::fs::create_dir(p.join("web")).unwrap();
        std::fs::write(p.join("web").join("wp-config.php"), "<?php").unwrap();
        std::fs::write(p.join("web").join("index.php"), "<?php").unwrap();
        assert_eq!(target(p), Ok(Target::Bedrock(p.join(".env"))));
        assert_eq!(kiln_sites::detect(p), kiln_sites::Driver::WordPressBedrock);
    }

    #[test]
    fn wordpress_gets_its_own_names() {
        let record = |kind: &str, role: &str, port: u16| kiln_state::ServiceRecord {
            name: format!("{kind}-x"),
            kind: kind.into(),
            version: "1".into(),
            ports: [(role.to_owned(), port)].into(),
            enabled: true,
            project: None,
        };
        let mysql = record("mysql", "mysql", 3307);
        let provider = kiln_services::provider("mysql").unwrap();
        let keys = wordpress_keys(provider, &mysql, Some("blog"), false).unwrap();
        let names: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["DB_NAME", "DB_USER", "DB_PASSWORD", "DB_HOST"]);
        assert_eq!(keys[0].1, "blog");
        assert_eq!(keys[3].1, "127.0.0.1:3307");

        let redis = record("redis", "redis", 6380);
        let provider = kiln_services::provider("redis").unwrap();
        let keys = wordpress_keys(provider, &redis, None, false).unwrap();
        assert_eq!(
            keys,
            [
                ("WP_REDIS_HOST".to_owned(), "127.0.0.1".to_owned()),
                ("WP_REDIS_PORT".to_owned(), "6380".to_owned())
            ]
        );
        assert!(wordpress_keys(provider, &redis, None, true).is_err());
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
