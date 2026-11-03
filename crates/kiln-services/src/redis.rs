//! Redis ([ADR 0041]): upstream Redis built for Windows with MSYS2 by the redis-windows
//! project, a config Kiln writes, and a shutdown through `redis-cli` that saves the data.
//!
//! [ADR 0041]: ../../../docs/adr/0041-redis.md

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{EXE_SUFFIX, InstanceDirs, Launch, PortRole, Release, ServiceProvider, run, tail};

pub struct Redis;

const PORTS: &[PortRole] = &[PortRole {
    role: "redis",
    usual: 6379,
    what: "Redis protocol, where apps connect",
}];

/// Windows only: Linux services wait for containers ([ADR 0039]).
///
/// [ADR 0039]: ../../../docs/adr/0039-windows-first-and-services-catalog.md
const RELEASES: &[Release] = &[
    Release {
        version: "8.10.3",
        os: "windows",
        arch: "x86_64",
        archive: "Redis-8.10.3-Windows-x64-msys2.zip",
        url: "https://github.com/redis-windows/redis-windows/releases/download/8.10.3/Redis-8.10.3-Windows-x64-msys2.zip",
        sha256: "af16df340525b431e9ec8dabfd625d5e7a233f82805b5418819904ea909c447f",
        executable: "redis-server.exe",
        tree: Some("Redis-8.10.3-Windows-x64-msys2"),
    },
    Release {
        version: "8.8.3",
        os: "windows",
        arch: "x86_64",
        archive: "Redis-8.8.3-Windows-x64-msys2.zip",
        url: "https://github.com/redis-windows/redis-windows/releases/download/8.8.3/Redis-8.8.3-Windows-x64-msys2.zip",
        sha256: "c2ebc76c007a12c96d7dd64e252a194d9e6a06fe801dfb834ca071314fd662e7",
        executable: "redis-server.exe",
        tree: Some("Redis-8.8.3-Windows-x64-msys2"),
    },
];

/// What the archive carries besides the server, its client, their checkers and the MSYS2
/// runtime: Sentinel, the benchmark, sample configs, a start script and readmes.
const SKIPPED_FILES: &[&str] = &[
    "redis-sentinel.exe",
    "redis-benchmark.exe",
    "redis.conf",
    "sentinel.conf",
    "start.bat",
    "README.md",
    "README.zh_CN.md",
];

fn port_of(ports: &BTreeMap<String, u16>) -> u16 {
    ports.get(PORTS[0].role).copied().unwrap_or(PORTS[0].usual)
}

/// [`InstanceDirs::config`]'s file name, which the server opens from the instance folder.
const CONFIG_NAME: &str = "kiln.cnf";

fn tool(bin: &Path, name: &str) -> PathBuf {
    bin.join(format!("{name}{EXE_SUFFIX}"))
}

/// The server writes this at start and removes it once it has saved its data and exited. It is
/// in the data folder: the config names it relative to `dir`, which Redis has moved into by then.
fn pid_file(dirs: &InstanceDirs) -> PathBuf {
    dirs.data.join("server.pid")
}

fn client_args(ports: &BTreeMap<String, u16>) -> Vec<String> {
    vec![
        "-h".to_owned(),
        "127.0.0.1".to_owned(),
        "-p".to_owned(),
        port_of(ports).to_string(),
    ]
}

impl ServiceProvider for Redis {
    fn kind(&self) -> &'static str {
        "redis"
    }

    fn display_name(&self) -> &'static str {
        "Redis"
    }

    fn license(&self) -> &'static str {
        "RSALv2, SSPLv1 or AGPLv3; built from Redis's source by the redis-windows project and \
         downloaded from its GitHub releases at your request; Kiln doesn't redistribute it"
    }

    fn ports(&self) -> &'static [PortRole] {
        PORTS
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-redis"
    }

    fn keep(&self, path: &str) -> bool {
        !SKIPPED_FILES.contains(&path)
    }

    fn series(&self, version: &str) -> String {
        version.splitn(3, '.').take(2).collect::<Vec<_>>().join(".")
    }

    /// Every path is relative to the instance folder, where it runs: the MSYS2 build reads
    /// `C:/...` as a relative path and puts its current folder in front ([ADR 0041] §3).
    ///
    /// Redis's defaults, written out where Kiln depends on them: snapshots in the data
    /// folder at the default save points and at shutdown, so queued jobs and sessions
    /// survive a restart; the log on standard output, which is the instance's log.
    fn config(&self, _dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Option<String> {
        Some(format!(
            "# Written by Kiln (ADR 0041); rewritten before every start.\n\
             bind 127.0.0.1\n\
             port {}\n\
             protected-mode yes\n\
             daemonize no\n\
             dir data\n\
             dbfilename dump.rdb\n\
             save 3600 1 300 100 60 10000\n\
             appendonly no\n\
             pidfile server.pid\n\
             logfile \"\"\n",
            port_of(ports),
        ))
    }

    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let mut stop = vec![tool(&dirs.bin, "redis-cli").to_string_lossy().into_owned()];
        stop.extend(client_args(ports));
        stop.push("shutdown".to_owned());
        Launch {
            program: self.executable(&dirs.bin),
            args: vec![CONFIG_NAME.to_owned()],
            env: BTreeMap::new(),
            cwd: Some(dirs.instance().to_path_buf()),
            stop_command: Some(stop),
        }
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        fs::create_dir_all(&dirs.data)
            .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))
    }

    fn check(&self, bin: &Path) -> Result<(), String> {
        let (code, output) = run(
            &self.executable(bin),
            &["--version".to_owned()],
            &BTreeMap::new(),
        )?;
        if code != 0 {
            return Err(format!(
                "Redis didn't start its check (exit code {code}):\n{}",
                tail(&output, 8)
            ));
        }
        Ok(())
    }

    fn stopped(&self, dirs: &InstanceDirs) -> bool {
        !pid_file(dirs).exists()
    }

    /// `predis`, Laravel's other client: Kiln's Windows PHP has no `phpredis` extension.
    fn env_keys(&self, ports: &BTreeMap<String, u16>) -> Vec<(&'static str, String)> {
        vec![
            ("REDIS_CLIENT", "predis".to_owned()),
            ("REDIS_HOST", "127.0.0.1".to_owned()),
            ("REDIS_PASSWORD", "null".to_owned()),
            ("REDIS_PORT", port_of(ports).to_string()),
        ]
    }

    fn client(
        &self,
        dirs: &InstanceDirs,
        ports: &BTreeMap<String, u16>,
        database: Option<&str>,
    ) -> Option<Launch> {
        let mut args = client_args(ports);
        if let Some(number) = database {
            args.extend(["-n".to_owned(), number.to_owned()]);
        }
        Some(Launch {
            program: tool(&dirs.bin, "redis-cli"),
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
        InstanceDirs::new(Path::new("/k/services"), "redis", "8.10.3", "redis-8.10")
    }

    #[test]
    fn the_config_listens_on_loopback_and_keeps_snapshots_in_the_data_folder() {
        let ports = BTreeMap::from([("redis".to_owned(), 6380)]);
        let config = Redis.config(&dirs(), &ports).unwrap();
        for line in [
            "bind 127.0.0.1",
            "port 6380",
            "protected-mode yes",
            "dir data",
            "pidfile server.pid",
            "logfile \"\"",
        ] {
            assert!(config.lines().any(|l| l == line), "{line} in:\n{config}");
        }
        assert!(!config.contains('\\'), "forward slashes only");
    }

    #[test]
    fn it_runs_its_config_and_stops_through_redis_cli() {
        let ports = BTreeMap::from([("redis".to_owned(), 6380)]);
        let launch = Redis.launch(&dirs(), &ports);
        assert!(launch.program.ends_with(format!("kiln-redis{EXE_SUFFIX}")));
        assert_eq!(launch.args, ["kiln.cnf"]);
        assert_eq!(dirs().config(), dirs().instance().join(CONFIG_NAME));
        assert_eq!(
            launch.cwd.as_deref(),
            Some(Path::new("/k/services/redis-8.10"))
        );
        let stop = launch.stop_command.unwrap();
        assert!(stop[0].ends_with(&format!("redis-cli{EXE_SUFFIX}")));
        assert_eq!(stop[1..], ["-h", "127.0.0.1", "-p", "6380", "shutdown"]);
    }

    #[test]
    fn extras_are_not_unpacked() {
        for path in [
            "redis-sentinel.exe",
            "redis-benchmark.exe",
            "start.bat",
            "README.md",
        ] {
            assert!(!Redis.keep(path), "{path}");
        }
        for path in [
            "redis-server.exe",
            "redis-cli.exe",
            "redis-check-rdb.exe",
            "msys-2.0.dll",
            "msys-ssl-3.dll",
        ] {
            assert!(Redis.keep(path), "{path}");
        }
    }

    #[test]
    fn instances_are_named_by_release_series_and_the_client_picks_a_database() {
        assert_eq!(Redis.series("8.10.3"), "8.10");
        let ports = BTreeMap::from([("redis".to_owned(), 6380)]);
        let client = Redis.client(&dirs(), &ports, Some("1")).unwrap();
        assert!(client.program.ends_with(format!("redis-cli{EXE_SUFFIX}")));
        assert_eq!(client.args, ["-h", "127.0.0.1", "-p", "6380", "-n", "1"]);
    }
}
