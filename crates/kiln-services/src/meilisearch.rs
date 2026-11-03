//! Meilisearch (MIT, Community Edition): the search engine Laravel Scout talks to
//! ([ADR 0042] §2).
//!
//! [ADR 0042]: ../../../docs/adr/0042-meilisearch-rustfs-mongodb.md

use std::collections::BTreeMap;
use std::fs;

use crate::{InstanceDirs, Launch, PortRole, Release, ServiceProvider};

pub struct Meilisearch;

const HTTP: PortRole = PortRole {
    role: "http",
    usual: 7700,
    what: "the search API, where Laravel Scout connects",
};

/// The Community Edition binary, not `meilisearch-enterprise-…` (other license), pinned from
/// GitHub's asset digest. The download is the executable itself. Windows only: Linux
/// services wait for containers (ADR 0039).
const RELEASES: &[Release] = &[Release {
    version: "1.56.1",
    os: "windows",
    arch: "x86_64",
    archive: "meilisearch-windows-amd64.exe",
    url: "https://github.com/meilisearch/meilisearch/releases/download/v1.56.1/meilisearch-windows-amd64.exe",
    sha256: "ac6d2506cafe862c93a6add533e125a19852339ec21303413120bde84aad3a48",
    executable: "meilisearch-windows-amd64.exe",
    tree: None,
}];

impl ServiceProvider for Meilisearch {
    fn kind(&self) -> &'static str {
        "meilisearch"
    }

    fn display_name(&self) -> &'static str {
        "Meilisearch"
    }

    fn license(&self) -> &'static str {
        "MIT (Community Edition)"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[HTTP]
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-meilisearch"
    }

    /// What Laravel Scout's Meilisearch engine reads; development mode needs no key.
    fn env_keys(&self, ports: &BTreeMap<String, u16>) -> Vec<(&'static str, String)> {
        let port = ports.get(HTTP.role).copied().unwrap_or(HTTP.usual);
        vec![
            ("MEILISEARCH_HOST", format!("http://127.0.0.1:{port}")),
            ("MEILISEARCH_KEY", String::new()),
        ]
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        // Meilisearch creates its database on first start.
        fs::create_dir_all(&dirs.data)
            .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))
    }

    /// Development mode needs no master key, which is what a local Scout setup expects.
    /// Stopping kills it on Windows: its LMDB store keeps committed writes through that.
    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let port = ports.get(HTTP.role).copied().unwrap_or(HTTP.usual);
        let instance = dirs.instance();
        Launch {
            program: self.executable(&dirs.bin),
            args: vec![
                "--http-addr".into(),
                format!("127.0.0.1:{port}"),
                "--db-path".into(),
                dirs.data.to_string_lossy().into_owned(),
                "--dump-dir".into(),
                instance.join("dumps").to_string_lossy().into_owned(),
                "--snapshot-dir".into(),
                instance.join("snapshots").to_string_lossy().into_owned(),
                "--env".into(),
                "development".into(),
                // It would otherwise send usage data to Meilisearch.
                "--no-analytics".into(),
            ],
            env: BTreeMap::new(),
            cwd: Some(instance.to_path_buf()),
            stop_command: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn it_listens_on_loopback_in_development_mode_without_analytics() {
        let dirs = InstanceDirs::new(Path::new("/k/services"), "meilisearch", "1.56.1", "m-1");
        let ports = BTreeMap::from([("http".to_owned(), 7701)]);
        let launch = Meilisearch.launch(&dirs, &ports);
        let args = launch.args.join(" ");
        assert!(args.contains("--http-addr 127.0.0.1:7701"), "{args}");
        assert!(args.contains("--env development"), "{args}");
        assert!(args.contains("--no-analytics"), "{args}");
        assert!(
            launch
                .args
                .contains(&dirs.data.to_string_lossy().into_owned())
        );
    }
}
