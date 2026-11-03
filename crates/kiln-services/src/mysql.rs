//! MySQL and MariaDB ([ADR 0038]): the vendors' archives, a config Kiln writes, the accounts
//! Laravel uses created at first start, a clean shutdown through the vendors' admin tools,
//! and the system libraries they need.
//!
//! [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    EXE_SUFFIX, InstanceDirs, Launch, PortRole, Release, STATUS_DLL_NOT_FOUND, ServiceProvider,
    needs_vc_runtime, run, tail,
};

/// MySQL or MariaDB: what differs between the two.
pub struct Flavor {
    kind: &'static str,
    display: &'static str,
    license: &'static str,
    releases: &'static [Release],
    /// The server program's Kiln name, e.g. `kiln-mysqld`, a hard link to the vendor's.
    kiln_server: &'static str,
    /// The admin tool that shuts it down, e.g. `mysqladmin`.
    admin: &'static str,
    /// The client, e.g. `mysql`.
    client: &'static str,
    /// Oracle MySQL: an X plugin to switch off, a monitor process on Windows, and
    /// `--initialize-insecure` for the data folder. MariaDB has its own installers.
    oracle: bool,
}

const PORTS: &[PortRole] = &[PortRole {
    role: "mysql",
    usual: 3306,
    what: "MySQL protocol, where apps connect",
}];

const MYSQL_RELEASES: &[Release] = &[
    Release {
        version: "9.7.4",
        os: "windows",
        arch: "x86_64",
        archive: "mysql-9.7.4-winx64.zip",
        url: "https://cdn.mysql.com/Downloads/MySQL-9.7/mysql-9.7.4-winx64.zip",
        sha256: "641bdaf01004927194c561b956f14f915a3166c9b0f9b1b5464ae63152bf4b96",
        executable: "bin/mysqld.exe",
        tree: Some("mysql-9.7.4-winx64"),
    },
    Release {
        version: "9.7.4",
        os: "linux",
        arch: "x86_64",
        archive: "mysql-9.7.4-linux-glibc2.28-x86_64-minimal.tar.xz",
        url: "https://cdn.mysql.com/Downloads/MySQL-9.7/mysql-9.7.4-linux-glibc2.28-x86_64-minimal.tar.xz",
        sha256: "1b022d85e2901e85d6f4df5482a03b036b5b2cf55ad786c9a788f805c6687665",
        executable: "bin/mysqld",
        tree: Some("mysql-9.7.4-linux-glibc2.28-x86_64-minimal"),
    },
    Release {
        version: "8.4.13",
        os: "windows",
        arch: "x86_64",
        archive: "mysql-8.4.13-winx64.zip",
        url: "https://cdn.mysql.com/Downloads/MySQL-8.4/mysql-8.4.13-winx64.zip",
        sha256: "e9cd98b1d23f122987b1afca26ad942efec5891481103d48ae3cde237fa3ea54",
        executable: "bin/mysqld.exe",
        tree: Some("mysql-8.4.13-winx64"),
    },
    Release {
        version: "8.4.13",
        os: "linux",
        arch: "x86_64",
        archive: "mysql-8.4.13-linux-glibc2.28-x86_64-minimal.tar.xz",
        url: "https://cdn.mysql.com/Downloads/MySQL-8.4/mysql-8.4.13-linux-glibc2.28-x86_64-minimal.tar.xz",
        sha256: "5dfbb764f731df40ef9e4f5d69aed32bdac8ba1af20bd96b7d40a942d83d4ed2",
        executable: "bin/mysqld",
        tree: Some("mysql-8.4.13-linux-glibc2.28-x86_64-minimal"),
    },
];

const MARIADB_RELEASES: &[Release] = &[
    Release {
        version: "12.3.3",
        os: "windows",
        arch: "x86_64",
        archive: "mariadb-12.3.3-winx64.zip",
        url: "https://archive.mariadb.org/mariadb-12.3.3/winx64-packages/mariadb-12.3.3-winx64.zip",
        sha256: "4b5706b47148b47c8f36963c8352d1ddee37a1287b4690428f631a74b09b85a0",
        executable: "bin/mariadbd.exe",
        tree: Some("mariadb-12.3.3-winx64"),
    },
    Release {
        version: "12.3.3",
        os: "linux",
        arch: "x86_64",
        archive: "mariadb-12.3.3-linux-systemd-x86_64.tar.gz",
        url: "https://archive.mariadb.org/mariadb-12.3.3/bintar-linux-systemd-x86_64/mariadb-12.3.3-linux-systemd-x86_64.tar.gz",
        sha256: "a12bf7bce08a896cdd01f174f04b4ce1d5da43ec250a5e7fceb9972119514329",
        executable: "bin/mariadbd",
        tree: Some("mariadb-12.3.3-linux-systemd-x86_64"),
    },
    Release {
        version: "11.8.9",
        os: "windows",
        arch: "x86_64",
        archive: "mariadb-11.8.9-winx64.zip",
        url: "https://archive.mariadb.org/mariadb-11.8.9/winx64-packages/mariadb-11.8.9-winx64.zip",
        sha256: "830c46727d9278eae212ae3eca44eeb9e71b2a68704e95f344a64fba7b1963f5",
        executable: "bin/mariadbd.exe",
        tree: Some("mariadb-11.8.9-winx64"),
    },
    Release {
        version: "11.8.9",
        os: "linux",
        arch: "x86_64",
        archive: "mariadb-11.8.9-linux-systemd-x86_64.tar.gz",
        url: "https://archive.mariadb.org/mariadb-11.8.9/bintar-linux-systemd-x86_64/mariadb-11.8.9-linux-systemd-x86_64.tar.gz",
        sha256: "32f8871a2aa38b36aa418b7f4abc6fc0c6ef1ba5a45798cacb2afa52c8a6113b",
        executable: "bin/mariadbd",
        tree: Some("mariadb-11.8.9-linux-systemd-x86_64"),
    },
];

pub static MYSQL: Flavor = Flavor {
    kind: "mysql",
    display: "MySQL",
    license: "GPL-2.0, downloaded from Oracle at your request; Kiln doesn't redistribute it",
    releases: MYSQL_RELEASES,
    kiln_server: "kiln-mysqld",
    admin: "mysqladmin",
    client: "mysql",
    oracle: true,
};

pub static MARIADB: Flavor = Flavor {
    kind: "mariadb",
    display: "MariaDB",
    license: "GPL-2.0, downloaded from the MariaDB Foundation at your request; Kiln doesn't redistribute it",
    releases: MARIADB_RELEASES,
    kiln_server: "kiln-mariadbd",
    admin: "mariadb-admin",
    client: "mariadb",
    oracle: false,
};

/// Folders skipped when unpacking ([ADR 0038] §2): tests, docs and headers. MariaDB's
/// plugins too (see [`Flavor::keep`]).
///
/// [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md
const SKIPPED_FOLDERS: &[&str] = &[
    "mysql-test/",
    "mariadb-test/",
    "sql-bench/",
    "include/",
    "docs/",
    "man/",
    "share/doc/",
    "lib/pkgconfig/",
];

/// The port of the instance's one role.
fn port_of(ports: &BTreeMap<String, u16>) -> u16 {
    ports.get(PORTS[0].role).copied().unwrap_or(PORTS[0].usual)
}

/// A path for the config file: forward slashes, which both servers read on every OS.
fn conf_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Kiln's own copy of a system library the server needs, when the system has it under
/// another name ([ADR 0038] §6). Linux only.
fn kiln_lib(bin: &Path) -> PathBuf {
    bin.join("kiln-lib")
}

/// The system library folders a library is looked for in (Linux).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const LIBRARY_DIRS: &[&str] = &[
    "/lib/x86_64-linux-gnu",
    "/usr/lib/x86_64-linux-gnu",
    "/lib64",
    "/usr/lib64",
    "/lib",
    "/usr/lib",
];

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn find_library(name: &str) -> Option<PathBuf> {
    LIBRARY_DIRS
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|path| path.exists())
}

/// The library a Linux program couldn't load, from its error output.
fn missing_library(output: &str) -> Option<&str> {
    let rest = output
        .split("error while loading shared libraries: ")
        .nth(1)?;
    rest.split(':').next().map(str::trim)
}

/// The package to install for a missing library, for the message.
fn package_for(library: &str) -> String {
    match library {
        "libaio.so.1" => "libaio1t64 (Ubuntu 24.04 and later; libaio1 on older systems)".to_owned(),
        "libnuma.so.1" => "libnuma1".to_owned(),
        "libncurses.so.6" | "libtinfo.so.6" => "libncurses6".to_owned(),
        other => format!("the package that provides {other}"),
    }
}

impl Flavor {
    fn tool(bin: &Path, name: &str) -> PathBuf {
        bin.join("bin").join(format!("{name}{EXE_SUFFIX}"))
    }

    fn env(bin: &Path) -> BTreeMap<String, String> {
        let lib = kiln_lib(bin);
        if cfg!(target_os = "linux") && lib.is_dir() {
            BTreeMap::from([(
                "LD_LIBRARY_PATH".to_owned(),
                lib.to_string_lossy().into_owned(),
            )])
        } else {
            BTreeMap::new()
        }
    }
}

impl ServiceProvider for Flavor {
    fn kind(&self) -> &'static str {
        self.kind
    }

    fn display_name(&self) -> &'static str {
        self.display
    }

    fn license(&self) -> &'static str {
        self.license
    }

    fn ports(&self) -> &'static [PortRole] {
        PORTS
    }

    fn releases(&self) -> &'static [Release] {
        self.releases
    }

    fn executable_name(&self) -> &'static str {
        self.kiln_server
    }

    fn executable(&self, bin: &Path) -> PathBuf {
        Self::tool(bin, self.kiln_server)
    }

    fn keep(&self, path: &str) -> bool {
        let skipped_type = [".pdb", ".lib", ".a"].iter().any(|ext| path.ends_with(ext));
        // MySQL loads a component from its plugin folder at every start; MariaDB needs none,
        // and its Linux plugins carry 469 MB of debug information.
        let skipped_plugins = !self.oracle && path.starts_with("lib/plugin/");
        !skipped_type
            && !skipped_plugins
            && !SKIPPED_FOLDERS.iter().any(|dir| path.starts_with(dir))
    }

    fn series(&self, version: &str) -> String {
        version.splitn(3, '.').take(2).collect::<Vec<_>>().join(".")
    }

    fn config(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Option<String> {
        let instance = dirs.instance();
        let mut text = format!(
            "# Written by Kiln (ADR 0038); rewritten before every start.\n\
             [mysqld]\n\
             basedir={}\n\
             datadir={}\n\
             bind-address=127.0.0.1\n\
             port={}\n\
             skip-name-resolve\n\
             pid-file={}\n\
             log-error={}\n",
            conf_path(&dirs.bin),
            conf_path(&dirs.data),
            port_of(ports),
            conf_path(&instance.join("server.pid")),
            conf_path(&instance.join("error.log")),
        );
        if cfg!(unix) {
            let _ = writeln!(text, "socket={}", conf_path(&instance.join("server.sock")));
        }
        if self.oracle {
            // Otherwise MySQL's X plugin listens on 33060 as well.
            text.push_str("mysqlx=OFF\n");
        }
        Some(text)
    }

    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let mut args = vec![format!("--defaults-file={}", dirs.config().display())];
        if self.oracle && cfg!(windows) {
            // Started directly, mysqld would relaunch itself under a monitor process.
            args.push("--no-monitor".to_owned());
        }
        Launch {
            program: self.executable(&dirs.bin),
            args,
            env: Self::env(&dirs.bin),
            cwd: Some(dirs.instance().to_path_buf()),
            stop_command: Some(vec![
                Self::tool(&dirs.bin, self.admin)
                    .to_string_lossy()
                    .into_owned(),
                "--host=127.0.0.1".to_owned(),
                format!("--port={}", port_of(ports)),
                "--user=root".to_owned(),
                "shutdown".to_owned(),
            ]),
        }
    }

    fn init_data(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Result<(), String> {
        let has_files = fs::read_dir(&dirs.data).is_ok_and(|mut d| d.next().is_some());
        if has_files {
            return Err(format!(
                "{} already holds data; Kiln doesn't set up a database over it",
                dirs.data.display()
            ));
        }
        let instance = dirs.instance();
        fs::create_dir_all(instance)
            .map_err(|e| format!("cannot create {}: {e}", instance.display()))?;
        let config = format!("--defaults-file={}", dirs.config().display());
        let (code, output) = if self.oracle {
            // Only root@localhost is made, which a TCP connection to 127.0.0.1 doesn't match
            // under skip-name-resolve: the init file adds root for 127.0.0.1 and ::1.
            let init = instance.join("init.sql");
            fs::write(
                &init,
                "CREATE USER IF NOT EXISTS 'root'@'127.0.0.1' IDENTIFIED BY '';\n\
                 GRANT ALL PRIVILEGES ON *.* TO 'root'@'127.0.0.1' WITH GRANT OPTION;\n\
                 CREATE USER IF NOT EXISTS 'root'@'::1' IDENTIFIED BY '';\n\
                 GRANT ALL PRIVILEGES ON *.* TO 'root'@'::1' WITH GRANT OPTION;\n",
            )
            .map_err(|e| format!("cannot write {}: {e}", init.display()))?;
            let result = run(
                &self.executable(&dirs.bin),
                &[
                    config,
                    "--initialize-insecure".to_owned(),
                    format!("--init-file={}", init.display()),
                ],
                &Self::env(&dirs.bin),
            );
            let _ = fs::remove_file(&init);
            result?
        } else if cfg!(windows) {
            run(
                &Self::tool(&dirs.bin, "mariadb-install-db"),
                &[
                    format!("--datadir={}", dirs.data.display()),
                    format!("--port={}", port_of(ports)),
                ],
                &Self::env(&dirs.bin),
            )?
        } else {
            run(
                &dirs.bin.join("scripts").join("mariadb-install-db"),
                &[
                    config,
                    "--auth-root-authentication-method=normal".to_owned(),
                    "--skip-test-db".to_owned(),
                ],
                &Self::env(&dirs.bin),
            )?
        };
        if code != 0 {
            let log = fs::read_to_string(instance.join("error.log")).unwrap_or_default();
            return Err(format!(
                "setting up {}'s data folder failed (exit code {code}):\n{}",
                self.display,
                tail(&format!("{output}\n{log}"), 12)
            ));
        }
        Ok(())
    }

    fn check(&self, bin: &Path) -> Result<(), String> {
        let server = self.executable(bin);
        let version = || run(&server, &["--version".to_owned()], &Self::env(bin));
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))] // changed on Linux only
        let (mut code, mut output) = version()?;
        // Ubuntu 24.04 renamed libaio.so.1 to libaio.so.1t64: link the old name to it in
        // Kiln's own folder, and run with LD_LIBRARY_PATH (ADR 0038 §6).
        #[cfg(target_os = "linux")]
        if missing_library(&output) == Some("libaio.so.1")
            && let Some(renamed) = find_library("libaio.so.1t64")
        {
            let lib = kiln_lib(bin);
            fs::create_dir_all(&lib)
                .map_err(|e| format!("cannot create {}: {e}", lib.display()))?;
            let link = lib.join("libaio.so.1");
            let _ = fs::remove_file(&link);
            std::os::unix::fs::symlink(&renamed, &link)
                .map_err(|e| format!("cannot link {}: {e}", link.display()))?;
            (code, output) = version()?;
        }
        if cfg!(windows) && code == STATUS_DLL_NOT_FOUND {
            return Err(needs_vc_runtime(self.display));
        }
        if let Some(library) = missing_library(&output) {
            return Err(format!(
                "{} needs {library}, which this system lacks. Install {} with your package \
                 manager (e.g. `sudo apt install libaio1t64`), then try again. Kiln doesn't \
                 install system packages itself.",
                self.display,
                package_for(library)
            ));
        }
        if code != 0 {
            return Err(format!(
                "{} didn't start its check (exit code {code}):\n{}",
                self.display,
                tail(&output, 8)
            ));
        }
        Ok(())
    }

    /// The server removes its pid file only at the end of a clean shutdown, once its data is
    /// flushed; it closes its port well before that.
    fn stopped(&self, dirs: &InstanceDirs) -> bool {
        !dirs.instance().join("server.pid").exists()
    }

    fn logs(&self, dirs: &InstanceDirs) -> Vec<PathBuf> {
        vec![dirs.instance().join("error.log"), dirs.log.clone()]
    }

    /// Laravel's connection of the same name: `mysql` or `mariadb` (Laravel 11 and later).
    /// The CLI uses `mysql` for a project that has no `mariadb` connection.
    fn env_keys(&self, ports: &BTreeMap<String, u16>) -> Vec<(&'static str, String)> {
        vec![
            ("DB_CONNECTION", self.kind.to_owned()),
            ("DB_HOST", "127.0.0.1".to_owned()),
            ("DB_PORT", port_of(ports).to_string()),
            ("DB_USERNAME", "root".to_owned()),
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
        let client = self.client(dirs, ports, None)?;
        let query = |sql: String| {
            let mut args = client.args.clone();
            args.extend([
                "--batch".into(),
                "--skip-column-names".into(),
                "-e".into(),
                sql,
            ]);
            match run(&client.program, &args, &client.env) {
                Ok((0, output)) => Ok(output),
                Ok((code, output)) => Err(format!(
                    "{} answered with exit code {code}:\n{}",
                    self.display,
                    tail(&output, 4)
                )),
                Err(e) => Err(e),
            }
        };
        Some((|| {
            let found = query(format!(
                "SELECT COUNT(*) FROM information_schema.schemata WHERE schema_name = '{name}'"
            ))?;
            if found.trim() != "0" {
                return Ok(false);
            }
            query(format!("CREATE DATABASE `{name}`"))?;
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
            "--user=root".to_owned(),
        ];
        args.extend(database.map(str::to_owned));
        Some(Launch {
            program: Self::tool(&dirs.bin, self.client),
            args,
            env: Self::env(&dirs.bin),
            cwd: None,
            stop_command: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> InstanceDirs {
        InstanceDirs::new(Path::new("/k/services"), "mysql", "8.4.13", "mysql-8.4")
    }

    #[test]
    fn the_config_listens_on_loopback_with_the_instance_port_and_no_x_plugin_for_mysql() {
        let ports = BTreeMap::from([("mysql".to_owned(), 3307)]);
        let mysql = MYSQL.config(&dirs(), &ports).unwrap();
        assert!(mysql.contains("bind-address=127.0.0.1\n"));
        assert!(mysql.contains("port=3307\n"));
        assert!(mysql.contains("skip-name-resolve\n"));
        assert!(mysql.contains("mysqlx=OFF\n"));
        assert!(mysql.contains("datadir=/k/services/mysql-8.4/data\n"));
        assert!(!mysql.contains('\\'), "forward slashes only");
        let mariadb = MARIADB.config(&dirs(), &ports).unwrap();
        assert!(!mariadb.contains("mysqlx"));
    }

    #[test]
    fn it_stops_through_the_admin_tool_and_runs_under_its_kiln_name() {
        let ports = BTreeMap::from([("mysql".to_owned(), 3307)]);
        let launch = MARIADB.launch(&dirs(), &ports);
        assert!(
            launch
                .program
                .ends_with(format!("kiln-mariadbd{EXE_SUFFIX}"))
        );
        let stop = launch.stop_command.unwrap();
        assert!(stop[0].ends_with(&format!("mariadb-admin{EXE_SUFFIX}")));
        assert_eq!(
            stop[1..],
            ["--host=127.0.0.1", "--port=3307", "--user=root", "shutdown"]
        );
        let mysql = MYSQL.launch(&dirs(), &ports);
        assert_eq!(
            mysql.args.contains(&"--no-monitor".to_owned()),
            cfg!(windows)
        );
    }

    #[test]
    fn extras_are_not_unpacked() {
        for path in [
            "bin/mysqld.pdb",
            "lib/mysqlclient.lib",
            "lib/libmysqlservices.a",
            "mysql-test/t/x.test",
            "mariadb-test/x",
            "include/mysql.h",
            "lib/plugin/auth_pam.pdb",
            "man/man1/mysql.1",
        ] {
            assert!(!MYSQL.keep(path), "{path}");
        }
        for path in [
            "bin/mysqld.exe",
            "bin/mysqladmin",
            "share/english/errmsg.sys",
            "scripts/mariadb-install-db",
            "lib/private/libssl.so.3",
            "lib/plugin/component_reference_cache.dll",
        ] {
            assert!(MYSQL.keep(path), "{path}");
        }
    }

    #[test]
    fn mariadb_leaves_its_plugins_out_and_mysql_keeps_them() {
        assert!(MYSQL.keep("lib/plugin/component_reference_cache.so"));
        assert!(!MARIADB.keep("lib/plugin/ha_rocksdb.so"));
        assert!(MARIADB.keep("bin/mariadbd"));
    }
    #[test]
    fn instances_are_named_by_release_series() {
        assert_eq!(MYSQL.series("8.4.13"), "8.4");
        assert_eq!(MARIADB.series("12.3.3"), "12.3");
    }

    #[test]
    fn a_missing_library_is_named_with_its_package() {
        let output = "/k/bin/kiln-mysqld: error while loading shared libraries: libaio.so.1: \
                      cannot open shared object file: No such file or directory";
        assert_eq!(missing_library(output), Some("libaio.so.1"));
        assert!(package_for("libaio.so.1").contains("libaio1t64"));
        assert_eq!(missing_library("mysqld  Ver 8.4.13"), None);
    }
}
