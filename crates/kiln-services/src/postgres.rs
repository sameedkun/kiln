//! PostgreSQL ([ADR 0040]): EDB's Windows binaries, a data folder made by `initdb` with
//! `trust` for loopback connections only, a config Kiln writes on top of `initdb`'s, and a
//! clean shutdown through `pg_ctl`.
//!
//! [ADR 0040]: ../../../docs/adr/0040-postgresql.md

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    EXE_SUFFIX, InstanceDirs, Launch, PortRole, Release, STATUS_DLL_NOT_FOUND, ServiceProvider,
    needs_vc_runtime, run, tail,
};

pub struct Postgres;

const PORTS: &[PortRole] = &[PortRole {
    role: "postgres",
    usual: 5432,
    what: "PostgreSQL protocol, where apps connect",
}];

/// The superuser `initdb` makes; Laravel's `DB_USERNAME` for these instances.
const SUPERUSER: &str = "postgres";

/// Windows only: PostgreSQL publishes no Linux binaries, and Linux services wait for
/// containers ([ADR 0039]).
///
/// [ADR 0039]: ../../../docs/adr/0039-windows-first-and-services-catalog.md
const RELEASES: &[Release] = &[
    Release {
        version: "18.6",
        os: "windows",
        arch: "x86_64",
        archive: "postgresql-18.6-5-windows-x64-binaries.zip",
        url: "https://get.enterprisedb.com/postgresql/postgresql-18.6-5-windows-x64-binaries.zip",
        sha256: "e2246ba91d22345bc3d017586c09ede52d9df180b1eeb480f050445f1cad84e2",
        executable: "bin/postgres.exe",
        tree: Some("pgsql"),
    },
    Release {
        version: "17.11",
        os: "windows",
        arch: "x86_64",
        archive: "postgresql-17.11-5-windows-x64-binaries.zip",
        url: "https://get.enterprisedb.com/postgresql/postgresql-17.11-5-windows-x64-binaries.zip",
        sha256: "80379b2c04d51c30225532e0ae04509899141e9957ed096fe749d7fd9df8f82f",
        executable: "bin/postgres.exe",
        tree: Some("pgsql"),
    },
];

/// What EDB bundles besides the server and its tools ([ADR 0040] §1): pgAdmin (about 780 MB
/// unpacked), Stack Builder and its wxWidgets libraries, docs and headers.
///
/// [ADR 0040]: ../../../docs/adr/0040-postgresql.md
const SKIPPED_FOLDERS: &[&str] = &[
    "pgAdmin 4/",
    "StackBuilder/",
    "doc/",
    "include/",
    "symbols/",
];

fn port_of(ports: &BTreeMap<String, u16>) -> u16 {
    ports.get(PORTS[0].role).copied().unwrap_or(PORTS[0].usual)
}

/// A path for the config file: forward slashes, which PostgreSQL reads on every OS.
fn conf_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Whether `postmaster.pid`'s contents say the server accepts sessions: its eighth line is
/// the status, `ready` once start-up is over (`starting`, `stopping` or `standby` otherwise).
fn is_ready(pid_file: &str) -> bool {
    pid_file.lines().nth(7).is_some_and(|s| s.trim() == "ready")
}

fn tool(bin: &Path, name: &str) -> PathBuf {
    bin.join("bin").join(format!("{name}{EXE_SUFFIX}"))
}

/// The server writes this when it starts and removes it once it has shut down cleanly.
fn pid_file(dirs: &InstanceDirs) -> PathBuf {
    dirs.data.join("postmaster.pid")
}

impl ServiceProvider for Postgres {
    fn kind(&self) -> &'static str {
        "postgres"
    }

    fn display_name(&self) -> &'static str {
        "PostgreSQL"
    }

    fn license(&self) -> &'static str {
        "PostgreSQL License, binaries built by EDB, downloaded from EDB at your request"
    }

    fn ports(&self) -> &'static [PortRole] {
        PORTS
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-postgres"
    }

    fn executable(&self, bin: &Path) -> PathBuf {
        tool(bin, self.executable_name())
    }

    fn keep(&self, path: &str) -> bool {
        let skipped_type = [".pdb", ".lib", ".a"].iter().any(|ext| path.ends_with(ext));
        let stackbuilder = path == "bin/stackbuilder.exe" || path.starts_with("bin/wx");
        !skipped_type && !stackbuilder && !SKIPPED_FOLDERS.iter().any(|dir| path.starts_with(dir))
    }

    /// `initdb`'s own `postgresql.conf` first (memory, locale and time zone settings), then
    /// where to listen, which wins.
    fn config(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Option<String> {
        Some(format!(
            "# Written by Kiln (ADR 0040); rewritten before every start.\n\
             include '{}'\n\
             listen_addresses='127.0.0.1'\n\
             port={}\n",
            conf_path(&dirs.data.join("postgresql.conf")),
            port_of(ports),
        ))
    }

    fn launch(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: self.executable(&dirs.bin),
            args: vec![
                "-D".to_owned(),
                dirs.data.to_string_lossy().into_owned(),
                "-c".to_owned(),
                format!("config_file={}", conf_path(&dirs.config())),
            ],
            env: BTreeMap::new(),
            cwd: Some(dirs.instance().to_path_buf()),
            // A fast shutdown: open sessions are ended, then the data is flushed.
            stop_command: Some(vec![
                tool(&dirs.bin, "pg_ctl").to_string_lossy().into_owned(),
                "stop".to_owned(),
                "-D".to_owned(),
                dirs.data.to_string_lossy().into_owned(),
                "-m".to_owned(),
                "fast".to_owned(),
                "-w".to_owned(),
            ]),
        }
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        let has_files = fs::read_dir(&dirs.data).is_ok_and(|mut d| d.next().is_some());
        if has_files {
            return Err(format!(
                "{} already holds data; Kiln doesn't set up a database over it",
                dirs.data.display()
            ));
        }
        // `trust` applies to the loopback lines initdb writes (127.0.0.1/32 and ::1/128),
        // and the server listens on 127.0.0.1 only. UTF-8 with the C locale: the same
        // sorting on every machine, and no dependence on Windows' code page.
        let (code, output) = run(
            &tool(&dirs.bin, "initdb"),
            &[
                "-D".to_owned(),
                dirs.data.to_string_lossy().into_owned(),
                format!("--username={SUPERUSER}"),
                "--auth=trust".to_owned(),
                "--encoding=UTF8".to_owned(),
                "--locale=C".to_owned(),
                "--no-instructions".to_owned(),
            ],
            &BTreeMap::new(),
        )?;
        if code != 0 {
            return Err(format!(
                "setting up PostgreSQL's data folder failed (exit code {code}):\n{}",
                tail(&output, 12)
            ));
        }
        Ok(())
    }

    fn check(&self, bin: &Path) -> Result<(), String> {
        let (code, output) = run(
            &self.executable(bin),
            &["--version".to_owned()],
            &BTreeMap::new(),
        )?;
        if cfg!(windows) && code == STATUS_DLL_NOT_FOUND {
            return Err(needs_vc_runtime(self.display_name()));
        }
        if code != 0 {
            return Err(format!(
                "PostgreSQL didn't start its check (exit code {code}):\n{}",
                tail(&output, 8)
            ));
        }
        Ok(())
    }

    fn stopped(&self, dirs: &InstanceDirs) -> bool {
        !pid_file(dirs).exists()
    }

    /// The server opens its port while it is still starting up, and refuses sessions until
    /// it has finished; then it writes `ready` as the pid file's status line, which is what
    /// `pg_ctl start -w` waits for.
    fn ready(&self, dirs: &InstanceDirs) -> bool {
        fs::read_to_string(pid_file(dirs)).is_ok_and(|text| is_ready(&text))
    }

    fn env_keys(&self, ports: &BTreeMap<String, u16>) -> Vec<(&'static str, String)> {
        vec![
            ("DB_CONNECTION", "pgsql".to_owned()),
            ("DB_HOST", "127.0.0.1".to_owned()),
            ("DB_PORT", port_of(ports).to_string()),
            ("DB_USERNAME", SUPERUSER.to_owned()),
            ("DB_PASSWORD", String::new()),
        ]
    }

    fn has_databases(&self) -> bool {
        true
    }

    fn create_database(
        &self,
        dirs: &InstanceDirs,
        ports: &BTreeMap<String, u16>,
        name: &str,
    ) -> Option<Result<bool, String>> {
        let client = self.client(dirs, ports, Some("postgres"))?;
        let query = |sql: String| {
            let mut args = client.args.clone();
            args.extend([
                "--tuples-only".into(),
                "--no-align".into(),
                "--command".into(),
                sql,
            ]);
            match run(&client.program, &args, &client.env) {
                Ok((0, output)) => Ok(output),
                Ok((code, output)) => Err(format!(
                    "PostgreSQL answered with exit code {code}:\n{}",
                    tail(&output, 4)
                )),
                Err(e) => Err(e),
            }
        };
        Some((|| {
            let found = query(format!(
                "SELECT 1 FROM pg_database WHERE datname = '{name}'"
            ))?;
            if found.trim() == "1" {
                return Ok(false);
            }
            query(format!("CREATE DATABASE \"{name}\""))?;
            Ok(true)
        })())
    }

    fn client(
        &self,
        dirs: &InstanceDirs,
        ports: &BTreeMap<String, u16>,
        database: Option<&str>,
    ) -> Option<Launch> {
        let mut args = vec![
            "--host=127.0.0.1".to_owned(),
            format!("--port={}", port_of(ports)),
            format!("--username={SUPERUSER}"),
        ];
        args.extend(database.map(|d| format!("--dbname={d}")));
        Some(Launch {
            program: tool(&dirs.bin, "psql"),
            args,
            env: BTreeMap::new(),
            cwd: None,
            stop_command: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> InstanceDirs {
        InstanceDirs::new(Path::new("/k/services"), "postgres", "18.6", "postgres-18")
    }

    #[test]
    fn the_config_adds_loopback_and_the_port_to_initdbs() {
        let ports = BTreeMap::from([("postgres".to_owned(), 5433)]);
        let config = Postgres.config(&dirs(), &ports).unwrap();
        assert!(config.contains(
            "include '/k/services/postgres-18/data/postgresql.conf'\n\
             listen_addresses='127.0.0.1'\nport=5433\n"
        ));
        assert!(!config.contains('\\'), "forward slashes only");
    }

    #[test]
    fn it_runs_with_kilns_config_and_stops_through_pg_ctl() {
        let launch = Postgres.launch(&dirs(), &BTreeMap::new());
        assert!(
            launch
                .program
                .ends_with(format!("kiln-postgres{EXE_SUFFIX}"))
        );
        assert_eq!(
            launch.args[2..],
            ["-c", "config_file=/k/services/postgres-18/kiln.cnf"]
        );
        let stop = launch.stop_command.unwrap();
        assert!(stop[0].ends_with(&format!("pg_ctl{EXE_SUFFIX}")));
        assert_eq!(stop[1], "stop");
        assert_eq!(stop[4..], ["-m", "fast", "-w"]);
    }

    #[test]
    fn pgadmin_stackbuilder_and_extras_are_not_unpacked() {
        for path in [
            "pgAdmin 4/runtime/pgAdmin4.exe",
            "StackBuilder/bin/stackbuilder.exe",
            "bin/stackbuilder.exe",
            "bin/wxbase3211u_vc_x64_custom.dll",
            "doc/postgresql/html/index.html",
            "include/libpq-fe.h",
            "lib/libpq.lib",
            "lib/libecpg.a",
        ] {
            assert!(!Postgres.keep(path), "{path}");
        }
        for path in [
            "bin/postgres.exe",
            "bin/initdb.exe",
            "bin/libpq.dll",
            "bin/icudt77.dll",
            "lib/plpgsql.dll",
            "share/postgres.bki",
            "share/timezone/Europe/London",
            "server_license.txt",
        ] {
            assert!(Postgres.keep(path), "{path}");
        }
    }

    #[test]
    fn ready_once_the_pid_files_status_says_so() {
        let pid = |status: &str| {
            format!(
                "4242\nC:/k/data\n1762084800\n5432\n\n127.0.0.1\n  5432001     65536\n{status}\n"
            )
        };
        assert!(is_ready(&pid("ready   ")));
        assert!(!is_ready(&pid("starting")));
        assert!(!is_ready(&pid("stopping")));
        assert!(!is_ready("4242\nC:/k/data\n"));
    }

    #[test]
    fn instances_are_named_by_major_version() {
        assert_eq!(Postgres.series("18.6"), "18");
    }

    #[test]
    fn the_client_connects_as_the_superuser_over_loopback() {
        let ports = BTreeMap::from([("postgres".to_owned(), 5433)]);
        let client = Postgres.client(&dirs(), &ports, Some("app")).unwrap();
        assert!(client.program.ends_with(format!("psql{EXE_SUFFIX}")));
        assert_eq!(
            client.args,
            [
                "--host=127.0.0.1",
                "--port=5433",
                "--username=postgres",
                "--dbname=app"
            ]
        );
    }
}
