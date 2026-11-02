//! Mailpit (MIT): catches the mail apps send, with a web UI to read it ([ADR 0036] §6).
//!
//! [ADR 0036]: ../../../docs/adr/0036-service-framework.md

use std::collections::BTreeMap;
use std::fs;

use crate::{InstanceDirs, Launch, PortRole, Release, ServiceProvider};

pub struct Mailpit;

const SMTP: PortRole = PortRole {
    role: "smtp",
    usual: 1025,
    what: "SMTP, where apps send mail",
};
const HTTP: PortRole = PortRole {
    role: "http",
    usual: 8025,
    what: "the web UI and API",
};

/// Pinned from GitHub's asset digests (Mailpit publishes no checksum file), re-checked by
/// downloading the Windows x64 archive. Released 2026-10-15.
const RELEASES: &[Release] = &[
    Release {
        version: "1.31.6",
        os: "windows",
        arch: "x86_64",
        archive: "mailpit-windows-amd64.zip",
        url: "https://github.com/axllent/mailpit/releases/download/v1.31.6/mailpit-windows-amd64.zip",
        sha256: "5d5ad094bfb3bac9004f9d1105d2335fea9f8952c09951df73ae0a280e34d3b1",
        executable: "mailpit.exe",
        tree: None,
    },
    Release {
        version: "1.31.6",
        os: "windows",
        arch: "aarch64",
        archive: "mailpit-windows-arm64.zip",
        url: "https://github.com/axllent/mailpit/releases/download/v1.31.6/mailpit-windows-arm64.zip",
        sha256: "c04c77ac9fd9da27c91259d0484b93b163e8534964682f543204cf3301a6b967",
        executable: "mailpit.exe",
        tree: None,
    },
    Release {
        version: "1.31.6",
        os: "linux",
        arch: "x86_64",
        archive: "mailpit-linux-amd64.tar.gz",
        url: "https://github.com/axllent/mailpit/releases/download/v1.31.6/mailpit-linux-amd64.tar.gz",
        sha256: "10fa12ccc610f653d4ed4df848c1817f9935dcf4e9a6a458a19ab0fa02093f91",
        executable: "mailpit",
        tree: None,
    },
    Release {
        version: "1.31.6",
        os: "linux",
        arch: "aarch64",
        archive: "mailpit-linux-arm64.tar.gz",
        url: "https://github.com/axllent/mailpit/releases/download/v1.31.6/mailpit-linux-arm64.tar.gz",
        sha256: "de07955b783236d9129da76da9a5970f1777834a18b2c07611de957be784dafe",
        executable: "mailpit",
        tree: None,
    },
];

impl ServiceProvider for Mailpit {
    fn kind(&self) -> &'static str {
        "mailpit"
    }

    fn display_name(&self) -> &'static str {
        "Mailpit"
    }

    fn license(&self) -> &'static str {
        "MIT"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[SMTP, HTTP]
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-mailpit"
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        // Mailpit creates its database on first start.
        fs::create_dir_all(&dirs.data)
            .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))
    }

    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let port = |role: &PortRole| ports.get(role.role).copied().unwrap_or(role.usual);
        Launch {
            program: self.executable(&dirs.bin),
            args: vec![
                "--smtp".into(),
                format!("127.0.0.1:{}", port(&SMTP)),
                "--listen".into(),
                format!("127.0.0.1:{}", port(&HTTP)),
                "--database".into(),
                dirs.data.join("mailpit.db").to_string_lossy().into_owned(),
                // It would otherwise check GitHub for a newer version by itself.
                "--disable-version-check".into(),
            ],
            env: BTreeMap::new(),
            cwd: Some(dirs.data.clone()),
            stop_command: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn runs_on_the_given_ports_with_its_database_in_the_data_folder() {
        let dirs = InstanceDirs::new(Path::new("/k/services"), "mailpit", "1.31.6", "mail");
        let ports = BTreeMap::from([("smtp".into(), 1026), ("http".into(), 8026)]);
        let launch = Mailpit.launch(&dirs, &ports);
        assert_eq!(
            launch.args,
            [
                "--smtp",
                "127.0.0.1:1026",
                "--listen",
                "127.0.0.1:8026",
                "--database",
                &dirs.data.join("mailpit.db").to_string_lossy(),
                "--disable-version-check"
            ]
        );
        assert_eq!(launch.program, Mailpit.executable(&dirs.bin));
    }
}
