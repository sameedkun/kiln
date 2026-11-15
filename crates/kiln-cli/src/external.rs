//! Database servers Kiln doesn't run ([ADR 0091]): Herd's MySQL, XAMPP's MariaDB, a Windows
//! service. Attached by port and login, they get Kiln's wiring, dumps and snapshots on
//! request; Kiln never starts, stops, configures or updates them.
//!
//! [ADR 0091]: ../../../docs/adr/0091-database-servers-kiln-doesnt-run.md

use std::collections::BTreeMap;
use std::path::PathBuf;

use kiln_services::{Auth, InstanceDirs};
use kiln_state::{ServiceRecord, StateStore};
use serde_json::{Value, json};

/// The setting holding them, as JSON by name.
pub const KEY: &str = "services.external";
/// The version an attached server's record carries.
pub const VERSION: &str = "external";
const KINDS: &[&str] = &["mysql", "mariadb", "postgres"];

/// One attached server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    pub kind: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// The server's own install folder (with `bin\`), for its tools.
    pub bin: Option<PathBuf>,
}

impl External {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            kind: v["kind"].as_str()?.to_owned(),
            port: u16::try_from(v["port"].as_u64()?).ok()?,
            user: v["user"].as_str()?.to_owned(),
            password: v["password"].as_str().unwrap_or_default().to_owned(),
            bin: v["bin"].as_str().map(PathBuf::from),
        })
    }

    fn to_json(&self) -> Value {
        json!({"kind": self.kind, "port": self.port, "user": self.user, "password": self.password, "bin": self.bin})
    }
}

/// Every attached server, by name.
#[must_use]
pub fn all(store: &StateStore) -> BTreeMap<String, External> {
    store
        .setting(KEY)
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str::<BTreeMap<String, Value>>(&t).ok())
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| External::from_json(&v).map(|e| (k, e)))
                .collect()
        })
        .unwrap_or_default()
}

fn save(store: &mut StateStore, all: &BTreeMap<String, External>) -> Result<(), String> {
    let map: BTreeMap<&String, Value> = all.iter().map(|(k, v)| (k, v.to_json())).collect();
    store
        .set_setting(
            KEY,
            &serde_json::to_string(&map).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
}

/// An attached server as a service record: always "on", its one port under the kind's role.
#[must_use]
pub fn record(name: &str, e: &External) -> ServiceRecord {
    let role = kiln_services::provider(&e.kind)
        .and_then(|p| p.ports().first())
        .map_or("mysql", |r| r.role);
    ServiceRecord {
        name: name.to_owned(),
        kind: e.kind.clone(),
        version: VERSION.to_owned(),
        ports: [(role.to_owned(), e.port)].into(),
        enabled: true,
        project: None,
    }
}

/// The newest of Kiln's own installs of `kind`, for its client and dump tools.
fn kilns_bin(kind: &str) -> Option<PathBuf> {
    let provider = kiln_services::provider(kind)?;
    let root = crate::service::services_dir().ok()?.join("bin").join(kind);
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&root)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| provider.installed(p))
        .collect();
    versions.sort();
    versions.pop()
}

/// The folders and login Kiln's tools use for an attached server.
///
/// # Errors
/// No tools for it: neither `--bin` nor one of Kiln's own installs of that kind.
pub fn dirs(name: &str, e: &External) -> Result<InstanceDirs, String> {
    let services = crate::service::services_dir()?;
    let mut dirs = InstanceDirs::new(&services, &e.kind, VERSION, name);
    dirs.bin = e.bin.clone().or_else(|| kilns_bin(&e.kind)).ok_or_else(|| {
        format!(
            "Kiln has no {} tools for {name}: give its install folder with `kiln service attach … --bin <folder>`, or add one of Kiln's (`kiln service add {}`)",
            e.kind, e.kind
        )
    })?;
    dirs.auth = Some(Auth {
        user: e.user.clone(),
        password: e.password.clone(),
    });
    Ok(dirs)
}

/// The attached server called `name`, if it is one.
#[must_use]
pub fn get(name: &str) -> Option<External> {
    crate::web::open_store()
        .ok()
        .and_then(|s| all(&s).remove(name))
}

/// Refuses what Kiln never does to a server it doesn't run (ADR 0091 §3).
///
/// # Errors
/// `name` is an attached server.
pub fn refuse(name: &str, what: &str) -> Result<(), String> {
    if get(name).is_some() {
        return Err(format!(
            "{name} isn't run by Kiln, so Kiln doesn't {what} it (it uses it only as you ask: wiring, dumps, snapshots)"
        ));
    }
    Ok(())
}

/// `kiln service attach <kind> --port <port> [--name] [--user] [--password] [--bin]`.
pub fn attach(
    kind: &str,
    port: u16,
    name: Option<&str>,
    user: Option<&str>,
    password: Option<&str>,
    bin: Option<PathBuf>,
) -> Result<(), String> {
    if !KINDS.contains(&kind) {
        return Err(format!(
            "Kiln attaches MySQL, MariaDB and PostgreSQL servers, not {kind}"
        ));
    }
    let name = name.map_or_else(|| format!("{kind}-{port}"), str::to_owned);
    let valid = !name.is_empty()
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'));
    if !valid {
        return Err(format!(
            "{name:?} can't be a service name: letters, digits, '.', '-' and '_'"
        ));
    }
    let mut store = crate::web::open_store()?;
    if store.service(&name).map_err(|e| e.to_string())?.is_some() || all(&store).contains_key(&name)
    {
        return Err(format!(
            "a service called {name} already exists; pass --name"
        ));
    }
    if store
        .services()
        .map_err(|e| e.to_string())?
        .iter()
        .any(|r| r.ports.values().any(|p| *p == port))
    {
        return Err(format!("port {port} is one of Kiln's own services"));
    }
    if std::net::TcpStream::connect_timeout(
        &(std::net::Ipv4Addr::LOCALHOST, port).into(),
        std::time::Duration::from_secs(2),
    )
    .is_err()
    {
        return Err(format!(
            "nothing answers on 127.0.0.1:{port}; start that server first"
        ));
    }
    let default_user = if kind == "postgres" {
        "postgres"
    } else {
        "root"
    };
    let external = External {
        kind: kind.to_owned(),
        port,
        user: user.unwrap_or(default_user).to_owned(),
        password: password.unwrap_or_default().to_owned(),
        bin,
    };
    // Kiln's tools reach it with that login before it's kept.
    let provider = crate::service::provider_of(kind)?;
    let record = record(&name, &external);
    let dirs = dirs(&name, &external)?;
    let databases = provider
        .list_databases(&dirs, &record.ports)
        .ok_or("that kind has no databases to list")?
        .map_err(|e| {
            format!(
                "Kiln couldn't log in to 127.0.0.1:{port} as {}: {e}",
                external.user
            )
        })?;
    let mut every = all(&store);
    every.insert(name.clone(), external);
    save(&mut store, &every)?;
    say!(
        "Attached {name}: {} on 127.0.0.1:{port}, not run by Kiln ({} database(s)). `kiln env use {name}` wires a site to it; Kiln never starts, stops or changes it.",
        provider.display_name(),
        databases.len()
    );
    Ok(())
}

/// `kiln service attach` with the password given, or staged by the app (read, then removed).
pub fn attach_with(
    kind: &str,
    port: u16,
    name: Option<&str>,
    user: Option<&str>,
    password: Option<String>,
    staged: Option<&str>,
    bin: Option<PathBuf>,
) -> Result<(), String> {
    let password = match staged {
        Some(token) => {
            let path = crate::config::staged_path(token)?;
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read the staged password: {e}"))?;
            let _ = std::fs::remove_file(&path);
            Some(text)
        }
        None => password,
    };
    attach(kind, port, name, user, password.as_deref(), bin)
}

/// `kiln service detach <name>`: forgotten; nothing on the server changes.
pub fn detach(name: &str) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let mut every = all(&store);
    if every.remove(name).is_none() {
        return Err(format!(
            "{name} isn't an attached server (see `kiln service list`)"
        ));
    }
    save(&mut store, &every)?;
    say!("Detached {name}: Kiln forgot it; the server and its data are as they were.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attached_server_round_trips_and_is_a_record_with_one_port() {
        let e = External {
            kind: "mysql".into(),
            port: 3306,
            user: "root".into(),
            password: "secret".into(),
            bin: Some(PathBuf::from(r"C:\xampp\mysql")),
        };
        assert_eq!(External::from_json(&e.to_json()), Some(e.clone()));
        let r = record("mysql-3306", &e);
        assert_eq!(r.version, VERSION);
        assert_eq!(r.ports.get("mysql"), Some(&3306));
        assert!(r.enabled && r.project.is_none());
        let pg = External {
            kind: "postgres".into(),
            port: 5432,
            ..e
        };
        assert_eq!(record("pg", &pg).ports.get("postgres"), Some(&5432));
    }
}
