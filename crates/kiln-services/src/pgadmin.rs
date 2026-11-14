//! pgAdmin 4 (PostgreSQL License), on request only ([ADR 0071]): taken from the PostgreSQL zip
//! Kiln already pins, which EDB ships it in, and run as a web app on loopback with the Python
//! it brings. Its desktop shell (`runtime/`, 384 MB) and help (`docs/`) stay in the archive.
//!
//! [ADR 0071]: ../../../docs/adr/0071-pgadmin-on-request.md

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{EXE_SUFFIX, InstanceDirs, Launch, PortRole, Release, ServiceProvider};

#[derive(Debug)]
pub struct Pgadmin;

const HTTP: PortRole = PortRole {
    role: "http",
    usual: 5050,
    what: "pgAdmin's web page",
};

/// pgAdmin 9.18, inside EDB's PostgreSQL 18.6 zip: the same file and pin as PostgreSQL 18.6's
/// (a test keeps them equal), so a PC with that PostgreSQL has it already downloaded.
const RELEASES: &[Release] = &[Release {
    version: "9.18",
    os: "windows",
    arch: "x86_64",
    archive: "postgresql-18.6-5-windows-x64-binaries.zip",
    url: "https://get.enterprisedb.com/postgresql/postgresql-18.6-5-windows-x64-binaries.zip",
    sha256: "e2246ba91d22345bc3d017586c09ede52d9df180b1eeb480f050445f1cad84e2",
    executable: "pgAdmin 4/python/python.exe",
    tree: Some("pgsql"),
}];

/// The PostgreSQL whose tools (`pg_dump`, `psql`) pgAdmin's backup and restore use.
const POSTGRES_VERSION: &str = "18.6";

/// The folder of pgAdmin's web app in `bin`.
fn web(bin: &Path) -> PathBuf {
    bin.join("pgAdmin 4").join("web")
}

/// A path as a Python string literal: JSON's is one, so a quote or backslash in a folder name
/// can't break the launcher.
fn py(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The file listing Kiln's PostgreSQL instances for pgAdmin, which Kiln writes before each
/// start (P4C-07).
#[must_use]
pub fn servers_file(dirs: &InstanceDirs) -> PathBuf {
    dirs.data.join("servers.json")
}

/// pgAdmin's `servers.json` for Kiln's PostgreSQL instances, by name and port: connected as
/// `postgres`, which Kiln's PostgreSQL trusts on loopback (ADR 0040).
#[must_use]
pub fn servers_json(instances: &[(String, u16)]) -> String {
    let servers: serde_json::Map<String, serde_json::Value> = instances
        .iter()
        .enumerate()
        .map(|(i, (name, port))| {
            (
                (i + 1).to_string(),
                serde_json::json!({
                    "Name": name,
                    "Group": "Kiln",
                    "Host": "127.0.0.1",
                    "Port": port,
                    "MaintenanceDB": "postgres",
                    "Username": "postgres",
                    "SSLMode": "disable",
                }),
            )
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({ "Servers": servers })).unwrap_or_default()
}

impl ServiceProvider for Pgadmin {
    fn kind(&self) -> &'static str {
        "pgadmin"
    }

    fn display_name(&self) -> &'static str {
        "pgAdmin"
    }

    fn license(&self) -> &'static str {
        "PostgreSQL License (pgAdmin 4 and the Python it ships), from EDB's PostgreSQL zip"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[HTTP]
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-pgadmin"
    }

    /// Beside `python.exe`: Python finds its library (`python313.zip`, `python313._pth`)
    /// next to the program it runs as.
    fn executable(&self, bin: &Path) -> PathBuf {
        bin.join("pgAdmin 4")
            .join("python")
            .join(format!("{}{EXE_SUFFIX}", self.executable_name()))
    }

    /// pgAdmin's Python and web app only.
    fn keep(&self, path: &str) -> bool {
        path.starts_with("pgAdmin 4/")
            && !path.starts_with("pgAdmin 4/runtime/")
            && !path.starts_with("pgAdmin 4/docs/")
    }

    /// Its page, as `pgadmin.test` (ADR 0044).
    fn web_ui(&self) -> Option<(&'static str, &'static str)> {
        Some(("pgadmin", HTTP.role))
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        // pgAdmin makes its database on first start.
        fs::create_dir_all(&dirs.data)
            .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))
    }

    fn logs(&self, dirs: &InstanceDirs) -> Vec<PathBuf> {
        vec![dirs.log.clone(), dirs.data.join("pgadmin4.log")]
    }

    /// Kiln's launcher, in Python: pgAdmin's settings pointed at this instance's folder (never
    /// `%APPDATA%\pgAdmin`, another pgAdmin's), loopback, the given port, no update check,
    /// Kiln's PostgreSQL tools; then Kiln's servers loaded, then pgAdmin started.
    fn config(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Option<String> {
        let port = ports.get(HTTP.role).copied().unwrap_or(HTTP.usual);
        let postgres_bin = dirs
            .bin
            .parent()
            .and_then(Path::parent)
            .map(|bins| bins.join("postgres").join(POSTGRES_VERSION).join("bin"))
            .unwrap_or_default();
        Some(format!(
            "# Kiln's pgAdmin launcher (ADR 0071), rewritten before every start.\n\
             bind = '127.0.0.1'\n\
             port = {port}\n\
             data = {data}\n\
             servers = {servers}\n\
             web = {web}\n\
             pg_bin = {pg_bin}\n\
             \n\
             import os, sys, subprocess, runpy\n\
             os.makedirs(data, exist_ok=True)\n\
             sys.path.insert(0, web)\n\
             os.chdir(web)\n\
             import config\n\
             config.SERVER_MODE = False\n\
             config.DEFAULT_SERVER = bind\n\
             config.DEFAULT_SERVER_PORT = port\n\
             config.DATA_DIR = data\n\
             config.LOG_FILE = os.path.join(data, 'pgadmin4.log')\n\
             config.SQLITE_PATH = os.path.join(data, 'pgadmin4.db')\n\
             config.SESSION_DB_PATH = os.path.join(data, 'sessions')\n\
             config.STORAGE_DIR = os.path.join(data, 'storage')\n\
             config.AZURE_CREDENTIAL_CACHE_DIR = os.path.join(data, 'azurecredentialcache')\n\
             config.KERBEROS_CCACHE_DIR = os.path.join(data, 'kerberoscache')\n\
             config.UPGRADE_CHECK_ENABLED = False\n\
             config.MASTER_PASSWORD_REQUIRED = False\n\
             config.DEFAULT_BINARY_PATHS = {{'pg': pg_bin, 'ppas': ''}}\n\
             if sys.argv[1:] == ['--load-servers']:\n\
             \x20   sys.argv = ['setup.py', 'load-servers', servers, '--replace']\n\
             \x20   runpy.run_path(os.path.join(web, 'setup.py'), run_name='__main__')\n\
             else:\n\
             \x20   if os.path.isfile(servers):\n\
             \x20       done = subprocess.run([sys.executable, __file__, '--load-servers'])\n\
             \x20       print('Kiln: servers loaded' if done.returncode == 0 else 'Kiln: loading the servers failed', flush=True)\n\
             \x20   sys.argv = ['pgAdmin4.py']\n\
             \x20   runpy.run_path(os.path.join(web, 'pgAdmin4.py'), run_name='__main__')\n",
            data = py(&dirs.data),
            servers = py(&servers_file(dirs)),
            web = py(&web(&dirs.bin)),
            pg_bin = py(&postgres_bin),
        ))
    }

    fn launch(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: self.executable(&dirs.bin),
            // The launcher sets the port and address itself (its `port` and `bind`).
            args: vec![dirs.config().to_string_lossy().into_owned()],
            // Anything left on pgAdmin's defaults lands in this instance's folder, not the
            // user's own pgAdmin settings.
            env: BTreeMap::from([
                (
                    "APPDATA".to_owned(),
                    dirs.data.to_string_lossy().into_owned(),
                ),
                (
                    "LOCALAPPDATA".to_owned(),
                    dirs.data.to_string_lossy().into_owned(),
                ),
                ("PYTHONDONTWRITEBYTECODE".to_owned(), "1".to_owned()),
            ]),
            cwd: Some(web(&dirs.bin)),
            stop_command: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_the_same_download_as_postgresql_18() {
        let pg = crate::postgres::Postgres
            .releases()
            .iter()
            .find(|r| r.version == POSTGRES_VERSION)
            .unwrap();
        let ours = &RELEASES[0];
        assert_eq!(
            (ours.archive, ours.url, ours.sha256),
            (pg.archive, pg.url, pg.sha256)
        );
    }

    #[test]
    fn only_its_python_and_web_app_are_unpacked() {
        assert!(Pgadmin.keep("pgAdmin 4/python/python.exe"));
        assert!(Pgadmin.keep("pgAdmin 4/web/pgAdmin4.py"));
        assert!(!Pgadmin.keep("pgAdmin 4/runtime/pgAdmin4.exe"));
        assert!(!Pgadmin.keep("pgAdmin 4/docs/en_US/html/index.html"));
        assert!(!Pgadmin.keep("bin/postgres.exe"));
    }

    #[test]
    fn the_launcher_keeps_pgadmin_on_loopback_in_its_own_folder() {
        let dirs = InstanceDirs::new(Path::new("C:/k/services"), "pgadmin", "9.18", "pgadmin-9");
        let text = Pgadmin
            .config(&dirs, &BTreeMap::from([("http".into(), 5051)]))
            .unwrap();
        assert!(text.contains("bind = '127.0.0.1'\nport = 5051\n"), "{text}");
        assert!(text.contains("config.UPGRADE_CHECK_ENABLED = False"));
        assert!(text.contains("pgadmin-9"));
        assert!(text.contains("postgres"));
        // Indented blocks are real Python indentation.
        assert!(text.contains("else:\n    if os.path.isfile(servers):\n        done = "));
        let launch = Pgadmin.launch(&dirs, &BTreeMap::new());
        assert_eq!(launch.args, [dirs.config().to_string_lossy().into_owned()]);
        assert!(
            launch
                .program
                .ends_with("pgAdmin 4/python/kiln-pgadmin.exe")
                || launch
                    .program
                    .ends_with("pgAdmin 4\\python\\kiln-pgadmin.exe")
        );
    }

    #[test]
    fn a_quote_in_a_folder_name_stays_inside_its_string() {
        assert_eq!(py(Path::new("C:/Users/O'Brien")), "\"C:/Users/O'Brien\"");
        assert_eq!(py(Path::new("C:\\a\"b")), "\"C:\\\\a\\\"b\"");
    }

    #[test]
    fn the_servers_are_kilns_postgresql_instances() {
        let text = servers_json(&[("postgres-18".into(), 5432), ("postgres-17".into(), 5433)]);
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["Servers"]["1"]["Name"], "postgres-18");
        assert_eq!(json["Servers"]["2"]["Port"], 5433);
        assert_eq!(json["Servers"]["2"]["Username"], "postgres");
    }
}
