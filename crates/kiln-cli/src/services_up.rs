//! `kiln services up` in a project ([ADR 0046]): the services its `kiln.toml` lists, found
//! among Kiln's instances or added, started, and wired into its `.env`.
//!
//! [ADR 0046]: ../../../docs/adr/0046-project-services.md

use std::path::{Path, PathBuf};

use kiln_services::{Release, ServiceProvider};
use kiln_sites::manifest::{self, ServiceWant};

/// A kind as `kiln.toml` may write it, as Kiln names it.
fn kind_of(name: &str) -> &str {
    match name {
        "pgsql" | "postgresql" => "postgres",
        "minio" => "rustfs",
        other => other,
    }
}

/// The pinned release `want` asks for: its version (or the newest), else the newest of the
/// same major line, else Kiln's newest, with a note saying so ([ADR 0046] §2). A project
/// made elsewhere may name a line Kiln doesn't pin, such as Redis 7.
///
/// [ADR 0046]: ../../../docs/adr/0046-project-services.md
fn release_for(
    provider: &dyn ServiceProvider,
    want: &ServiceWant,
) -> Result<&'static Release, String> {
    let version = want.version.as_deref();
    if let Some(release) = kiln_services::release_for_this_platform(provider, version) {
        return Ok(release);
    }
    let major = version.and_then(|v| v.split('.').next());
    let (release, same_line) =
        match major.and_then(|m| kiln_services::release_for_this_platform(provider, Some(m))) {
            Some(release) => (release, true),
            None => (
                kiln_services::release_for_this_platform(provider, None).ok_or_else(|| {
                    format!("Kiln has no {} for this computer", provider.display_name())
                })?,
                false,
            ),
        };
    println!(
        "Kiln has no {} {}; using {}{}.",
        provider.display_name(),
        version.unwrap_or_default(),
        release.version,
        if same_line {
            ""
        } else {
            ", its newest: check it suits the project"
        }
    );
    Ok(release)
}

/// The instance for `want`: one of `release`'s series that exists, else a new one.
fn instance_for(
    provider: &dyn ServiceProvider,
    want: &ServiceWant,
    release: &Release,
) -> Result<String, String> {
    let series = provider.series(release.version);
    let existing = crate::web::open_store()?
        .services()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|s| s.kind == provider.kind() && provider.series(&s.version) == series)
        .max_by_key(|s| s.enabled);
    if let Some(record) = existing {
        println!("{}: using {}.", provider.kind(), record.name);
        if let Some(port) = want.port
            && let Some(main) = provider.ports().first()
            && record.ports.get(main.role) != Some(&port)
        {
            println!(
                "  Note: kiln.toml asks for port {port}; {} is on {}. `kiln service port {} {}={port}` moves it.",
                record.name,
                record.ports.get(main.role).copied().unwrap_or_default(),
                record.name,
                main.role
            );
        }
        return Ok(record.name);
    }
    let ports: Vec<String> = match (want.port, provider.ports().first()) {
        (Some(port), Some(main)) => vec![format!("{}={port}", main.role)],
        _ => Vec::new(),
    };
    crate::service::add(provider.kind(), Some(release.version), None, &ports, None)
}

/// The instance of a service that runs in a project (Reverb): this project's, else a new one.
fn project_instance(
    provider: &dyn ServiceProvider,
    want: &ServiceWant,
    dir: &Path,
) -> Result<String, String> {
    let dir = std::fs::canonicalize(dir)
        .map(crate::web::strip_verbatim)
        .map_err(|e| format!("cannot use {}: {e}", dir.display()))?;
    let existing = crate::web::open_store()?
        .services()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|s| s.kind == provider.kind() && s.project.as_deref() == Some(dir.as_path()));
    if let Some(record) = existing {
        println!("{}: using {}.", provider.kind(), record.name);
        return Ok(record.name);
    }
    let ports: Vec<String> = match (want.port, provider.ports().first()) {
        (Some(port), Some(main)) => vec![format!("{}={port}", main.role)],
        _ => Vec::new(),
    };
    crate::service::add(provider.kind(), None, None, &ports, Some(dir))
}

/// `kiln services up [--project <folder>] [--yes]`. Without a project that lists services,
/// every instance starts, as before.
pub fn up(project: Option<PathBuf>, yes: bool) -> Result<(), String> {
    let explicit = project.is_some();
    let dir = match project {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let wants = manifest::read(&dir)?
        .map(|m| m.services)
        .unwrap_or_default();
    if wants.is_empty() {
        if explicit {
            return Err(format!(
                "{} lists no services: add a [services] table to its {}",
                dir.display(),
                manifest::FILE
            ));
        }
        return crate::service::start(&[]);
    }
    let mut chosen: Vec<(String, &'static dyn ServiceProvider)> = Vec::new();
    for want in &wants {
        let provider = crate::service::provider_of(kind_of(&want.kind))?;
        let name = if provider.runs_in_project() {
            project_instance(provider, want, &dir)?
        } else {
            instance_for(provider, want, release_for(provider, want)?)?
        };
        chosen.push((name, provider));
    }
    let names: Vec<String> = chosen.iter().map(|(name, _)| name.clone()).collect();
    crate::service::start(&names)?;
    println!("Running: {}.", names.join(", "));
    wire(&dir, &chosen, yes)
}

/// Offers `kiln env use` for each instance with `.env` keys. Only the first database is
/// wired: a second would set the same `DB_*` keys.
fn wire(
    dir: &Path,
    chosen: &[(String, &'static dyn ServiceProvider)],
    yes: bool,
) -> Result<(), String> {
    if !dir.join(".env").is_file() {
        println!(
            "{} has no .env, so nothing was wired; `kiln env use <instance>` does it later.",
            dir.display()
        );
        return Ok(());
    }
    let mut database: Option<&str> = None;
    for (name, provider) in chosen {
        if provider.has_databases() {
            if let Some(first) = database {
                println!(
                    "{name} is a second database; .env stays on {first} (`kiln env use {name}` switches it)."
                );
                continue;
            }
            database = Some(name);
        }
        let keys = crate::service::record_of(&crate::web::open_store()?, name)
            .map(|record| provider.env_keys(&record.ports))?;
        if keys.is_empty() {
            continue;
        }
        crate::env::use_instance(
            name,
            Some(dir.to_path_buf()),
            None,
            false,
            false,
            false,
            yes,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_read_as_kiln_names_them() {
        assert_eq!(kind_of("pgsql"), "postgres");
        assert_eq!(kind_of("postgresql"), "postgres");
        assert_eq!(kind_of("minio"), "rustfs");
        assert_eq!(kind_of("mysql"), "mysql");
    }

    #[test]
    fn a_missing_line_falls_back_to_its_major() {
        let mysql = kiln_services::provider("mysql").unwrap();
        let want = |v: &str| ServiceWant {
            kind: "mysql".into(),
            version: Some(v.into()),
            port: None,
        };
        if cfg!(windows) {
            assert_eq!(release_for(mysql, &want("8.4")).unwrap().version, "8.4.13");
            assert_eq!(release_for(mysql, &want("8.0")).unwrap().version, "8.4.13");
            assert_eq!(release_for(mysql, &want("5.7")).unwrap().version, "9.7.4");
        }
    }
}
