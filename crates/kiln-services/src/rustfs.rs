//! RustFS (Apache-2.0): S3-compatible object storage, for Laravel's `s3` disk
//! ([ADR 0042] §3).
//!
//! [ADR 0042]: ../../../docs/adr/0042-meilisearch-rustfs-mongodb.md

use std::collections::BTreeMap;
use std::fs;

use crate::{InstanceDirs, Launch, PortRole, Release, ServiceProvider};

pub struct Rustfs;

const S3: PortRole = PortRole {
    role: "s3",
    usual: 9000,
    what: "the S3 API, where apps store files",
};
const CONSOLE: PortRole = PortRole {
    role: "console",
    usual: 9001,
    what: "the web console",
};

/// The credentials apps use: RustFS's own defaults, set explicitly so an upstream change
/// doesn't lock a project out. Local only: the server listens on 127.0.0.1.
pub const ACCESS_KEY: &str = "rustfsadmin";
pub const SECRET_KEY: &str = "rustfsadmin";

/// Pinned from GitHub's asset digest. Windows only: Linux services wait for containers
/// (ADR 0039).
const RELEASES: &[Release] = &[Release {
    version: "1.0.4",
    os: "windows",
    arch: "x86_64",
    archive: "rustfs-windows-x86_64-v1.0.4.zip",
    url: "https://github.com/rustfs/rustfs/releases/download/1.0.4/rustfs-windows-x86_64-v1.0.4.zip",
    sha256: "2e3af0b187dd70435dfdefe5f61aae7eede5a21fc9a2a2f07e289f727adbcfa6",
    executable: "rustfs.exe",
    tree: None,
}];

impl ServiceProvider for Rustfs {
    fn kind(&self) -> &'static str {
        "rustfs"
    }

    fn display_name(&self) -> &'static str {
        "RustFS"
    }

    fn license(&self) -> &'static str {
        "Apache-2.0"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[S3, CONSOLE]
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-rustfs"
    }

    /// Laravel's `s3` disk, pointed at RustFS with path-style URLs. The bucket stays the
    /// project's own choice (`AWS_BUCKET`).
    fn env_keys(&self, ports: &BTreeMap<String, u16>) -> Vec<(&'static str, String)> {
        let s3 = ports.get(S3.role).copied().unwrap_or(S3.usual);
        vec![
            ("AWS_ACCESS_KEY_ID", ACCESS_KEY.to_owned()),
            ("AWS_SECRET_ACCESS_KEY", SECRET_KEY.to_owned()),
            ("AWS_DEFAULT_REGION", "us-east-1".to_owned()),
            ("AWS_ENDPOINT", format!("http://127.0.0.1:{s3}")),
            ("AWS_USE_PATH_STYLE_ENDPOINT", "true".to_owned()),
        ]
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        fs::create_dir_all(&dirs.data)
            .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))
    }

    /// Both addresses are given: RustFS's defaults (`:9000`, `:9001`) listen on every
    /// interface. The volume is `data`, relative to the instance folder it runs in, because
    /// RustFS splits its volume list at spaces and a user folder may have one.
    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let port = |role: &PortRole| ports.get(role.role).copied().unwrap_or(role.usual);
        Launch {
            program: self.executable(&dirs.bin),
            args: vec![
                "server".into(),
                "--address".into(),
                format!("127.0.0.1:{}", port(&S3)),
                "--console-address".into(),
                format!("127.0.0.1:{}", port(&CONSOLE)),
                "data".into(),
            ],
            env: BTreeMap::from([
                ("RUSTFS_ACCESS_KEY".to_owned(), ACCESS_KEY.to_owned()),
                ("RUSTFS_SECRET_KEY".to_owned(), SECRET_KEY.to_owned()),
                ("RUSTFS_CONSOLE_ENABLE".to_owned(), "true".to_owned()),
            ]),
            cwd: Some(dirs.instance().to_path_buf()),
            stop_command: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn both_addresses_are_loopback_and_the_volume_is_relative() {
        let dirs = InstanceDirs::new(Path::new("/k/my services"), "rustfs", "1.0.4", "r-1");
        let ports = BTreeMap::from([("s3".to_owned(), 9100), ("console".to_owned(), 9101)]);
        let launch = Rustfs.launch(&dirs, &ports);
        assert_eq!(
            launch.args,
            [
                "server",
                "--address",
                "127.0.0.1:9100",
                "--console-address",
                "127.0.0.1:9101",
                "data"
            ]
        );
        assert_eq!(launch.cwd.as_deref(), Some(dirs.instance()));
        assert_eq!(dirs.data, dirs.instance().join("data"));
        assert_eq!(launch.env["RUSTFS_ACCESS_KEY"], ACCESS_KEY);
    }
}
