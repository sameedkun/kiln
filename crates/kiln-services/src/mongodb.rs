//! MongoDB (SSPL, Community Server): MongoDB's own Windows archives, with `mongosh` as a
//! companion for the clean shutdown and `kiln db` ([ADR 0042] §4).
//!
//! [ADR 0042]: ../../../docs/adr/0042-meilisearch-rustfs-mongodb.md

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    EXE_SUFFIX, InstanceDirs, Launch, PortRole, Release, STATUS_DLL_NOT_FOUND, ServiceProvider,
    needs_vc_runtime, release_folder, run, tail,
};

pub struct Mongodb;

const PORTS: &[PortRole] = &[PortRole {
    role: "mongodb",
    usual: 27017,
    what: "MongoDB protocol, where apps connect",
}];

/// Pinned from the SHA-256 in MongoDB's release feed (`downloads.mongodb.org/full.json`).
/// 9.0 is the current line, 8.0 the long-term one. Windows only: Linux services wait for
/// containers (ADR 0039).
const RELEASES: &[Release] = &[
    Release {
        version: "9.0.4",
        os: "windows",
        arch: "x86_64",
        archive: "mongodb-windows-x86_64-9.0.4.zip",
        url: "https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-9.0.4.zip",
        sha256: "1e6234c8c33e75de3659a7e1104cb12f1cb68ae41cba60b625537c1ed9256d43",
        executable: "bin/mongod.exe",
        tree: Some("mongodb-win32-x86_64-windows-9.0.4"),
    },
    Release {
        version: "8.0.34",
        os: "windows",
        arch: "x86_64",
        archive: "mongodb-windows-x86_64-8.0.34.zip",
        url: "https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-8.0.34.zip",
        sha256: "6b021a0864494e4decf278b36477a29eb36072432773e4e62c2f42c302630db0",
        executable: "bin/mongod.exe",
        tree: Some("mongodb-win32-x86_64-windows-8.0.34"),
    },
];

/// The MongoDB Shell (Apache-2.0), pinned from GitHub's asset digest. The server has no
/// shutdown tool of its own on Windows (`mongod --shutdown` is Linux-only).
const MONGOSH: &[Release] = &[Release {
    version: "2.14.1",
    os: "windows",
    arch: "x86_64",
    archive: "mongosh-2.14.1-win32-x64.zip",
    url: "https://github.com/mongodb-js/mongosh/releases/download/v2.14.1/mongosh-2.14.1-win32-x64.zip",
    sha256: "be58d3c67a2bca990c790b359c99fa9b5da1b12bb5b01e8442bea5998b1a38e2",
    executable: "bin/mongosh.exe",
    tree: Some("mongosh-2.14.1-win32-x64"),
}];

/// `mongosh`'s global config, read from next to `mongosh.exe` on Windows: no usage data
/// sent to MongoDB, no checks for a newer shell.
const MONGOSH_CONFIG: &str = "# Written by Kiln (ADR 0042).\n\
                              mongosh:\n  enableTelemetry: false\n  updateURL: ''\n";

fn port_of(ports: &BTreeMap<String, u16>) -> u16 {
    ports.get(PORTS[0].role).copied().unwrap_or(PORTS[0].usual)
}

fn mongosh(bin: &Path) -> PathBuf {
    release_folder(bin, &MONGOSH[0]).join(MONGOSH[0].executable)
}

/// The shell keeps its history and logs under `%APPDATA%` and `%LOCALAPPDATA%`: pointed into
/// the instance folder, so nothing is left outside Kiln's folders.
fn shell_env(dirs: &InstanceDirs) -> BTreeMap<String, String> {
    let home = dirs
        .instance()
        .join("mongosh")
        .to_string_lossy()
        .into_owned();
    BTreeMap::from([
        ("APPDATA".to_owned(), home.clone()),
        ("LOCALAPPDATA".to_owned(), home),
    ])
}

fn shell_args(ports: &BTreeMap<String, u16>) -> Vec<String> {
    vec![
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port_of(ports).to_string(),
    ]
}

impl ServiceProvider for Mongodb {
    fn kind(&self) -> &'static str {
        "mongodb"
    }

    fn display_name(&self) -> &'static str {
        "MongoDB"
    }

    fn license(&self) -> &'static str {
        "SSPLv1 (Community Server), downloaded from MongoDB at your request; Kiln doesn't \
         redistribute it. The MongoDB Shell is Apache-2.0"
    }

    fn ports(&self) -> &'static [PortRole] {
        PORTS
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn companions(&self, _release: &Release) -> &'static [Release] {
        MONGOSH
    }

    fn executable_name(&self) -> &'static str {
        "kiln-mongod"
    }

    fn executable(&self, bin: &Path) -> PathBuf {
        bin.join("bin").join(format!("kiln-mongod{EXE_SUFFIX}"))
    }

    /// The server and the licence files: not the debug symbols (most of the 800–990 MB
    /// archive), the router, the bundled VC++ installer or Compass's installer script.
    fn keep(&self, path: &str) -> bool {
        !crate::install::is_type(path, ".pdb")
            && ![
                "bin/mongos.exe",
                "bin/vc_redist.x64.exe",
                "bin/Install-Compass.ps1",
            ]
            .contains(&path)
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        // mongod creates its files on first start.
        fs::create_dir_all(&dirs.data)
            .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))
    }

    /// WiredTiger's cache is capped at 256 MB: its default, half the computer's memory, is
    /// for a dedicated server. The log goes to standard output, the instance's log.
    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let mut stop = vec![mongosh(&dirs.bin).to_string_lossy().into_owned()];
        stop.extend(shell_args(ports));
        stop.extend([
            "--quiet".to_owned(),
            "--norc".to_owned(),
            "--eval".to_owned(),
            "db.getSiblingDB('admin').shutdownServer()".to_owned(),
        ]);
        Launch {
            program: self.executable(&dirs.bin),
            args: vec![
                "--bind_ip=127.0.0.1".to_owned(),
                format!("--port={}", port_of(ports)),
                format!("--dbpath={}", dirs.data.display()),
                "--wiredTigerCacheSizeGB=0.25".to_owned(),
            ],
            env: shell_env(dirs),
            cwd: Some(dirs.instance().to_path_buf()),
            stop_command: Some(stop),
        }
    }

    /// Also writes `mongosh`'s config, which keeps it from contacting MongoDB.
    fn check(&self, bin: &Path) -> Result<(), String> {
        let config = mongosh(bin).with_file_name("mongosh.cfg");
        fs::write(&config, MONGOSH_CONFIG)
            .map_err(|e| format!("cannot write {}: {e}", config.display()))?;
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
                "MongoDB didn't start its check (exit code {code}):\n{}",
                tail(&output, 8)
            ));
        }
        Ok(())
    }

    /// The server empties `mongod.lock` at the end of a clean shutdown; it closes its port
    /// before that.
    fn stopped(&self, dirs: &InstanceDirs) -> bool {
        fs::metadata(dirs.data.join("mongod.lock")).map_or(true, |m| m.len() == 0)
    }

    fn client(
        &self,
        dirs: &InstanceDirs,
        ports: &BTreeMap<String, u16>,
        database: Option<&str>,
    ) -> Option<Launch> {
        let mut args = shell_args(ports);
        args.push("--norc".to_owned());
        args.extend(database.map(str::to_owned));
        Some(Launch {
            program: mongosh(&dirs.bin),
            args,
            env: shell_env(dirs),
            cwd: None,
            stop_command: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> InstanceDirs {
        InstanceDirs::new(Path::new("/k/services"), "mongodb", "9.0.4", "mongodb-9")
    }

    #[test]
    fn it_listens_on_loopback_and_stops_through_mongosh() {
        let ports = BTreeMap::from([("mongodb".to_owned(), 27018)]);
        let launch = Mongodb.launch(&dirs(), &ports);
        assert!(launch.args.contains(&"--bind_ip=127.0.0.1".to_owned()));
        assert!(launch.args.contains(&"--port=27018".to_owned()));
        let stop = launch.stop_command.unwrap();
        assert!(Path::new(&stop[0]).starts_with(release_folder(&dirs().bin, &MONGOSH[0])));
        assert!(stop.contains(&"db.getSiblingDB('admin').shutdownServer()".to_owned()));
        assert!(launch.env["APPDATA"].starts_with(&*dirs().instance().to_string_lossy()));
    }

    #[test]
    fn only_the_server_and_its_licences_are_unpacked() {
        for path in [
            "bin/mongod.pdb",
            "bin/mongos.exe",
            "bin/vc_redist.x64.exe",
            "bin/Install-Compass.ps1",
        ] {
            assert!(!Mongodb.keep(path), "{path}");
        }
        for path in [
            "bin/mongod.exe",
            "LICENSE-Community.txt",
            "THIRD-PARTY-NOTICES",
        ] {
            assert!(Mongodb.keep(path), "{path}");
        }
    }

    #[test]
    fn the_client_opens_a_database() {
        let ports = BTreeMap::from([("mongodb".to_owned(), 27018)]);
        let client = Mongodb.client(&dirs(), &ports, Some("app")).unwrap();
        assert_eq!(
            client.args,
            ["--host", "127.0.0.1", "--port", "27018", "--norc", "app"]
        );
    }
}
