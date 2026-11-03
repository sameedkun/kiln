//! Laravel Reverb ([ADR 0047]): the project's own WebSocket server, `php artisan
//! reverb:start`, run by Kiln with the project's PHP and kept up like any service.
//!
//! [ADR 0047]: ../../../docs/adr/0047-reverb.md

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::{InstanceDirs, Launch, PortRole, Release, ServiceProvider};

#[derive(Debug)]
pub struct Reverb;

const PORTS: &[PortRole] = &[PortRole {
    role: "reverb",
    usual: 8080,
    what: "WebSocket connections and the broadcast API",
}];

fn port_of(ports: &BTreeMap<String, u16>) -> u16 {
    ports.get(PORTS[0].role).copied().unwrap_or(PORTS[0].usual)
}

/// The `laravel/reverb` version a project has installed, from its `composer.lock`.
#[must_use]
pub fn installed_version(project: &Path) -> Option<String> {
    let lock = fs::read_to_string(project.join("composer.lock")).ok()?;
    let lock: serde_json::Value = serde_json::from_str(&lock).ok()?;
    lock["packages"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == "laravel/reverb")?["version"]
        .as_str()
        .map(|v| v.trim_start_matches('v').to_owned())
}

impl ServiceProvider for Reverb {
    fn kind(&self) -> &'static str {
        "reverb"
    }

    fn display_name(&self) -> &'static str {
        "Laravel Reverb"
    }

    fn license(&self) -> &'static str {
        "MIT, the project's own laravel/reverb package"
    }

    fn ports(&self) -> &'static [PortRole] {
        PORTS
    }

    /// Nothing to download: the project brings it.
    fn releases(&self) -> &'static [Release] {
        &[]
    }

    fn executable_name(&self) -> &'static str {
        "php"
    }

    fn runs_in_project(&self) -> bool {
        true
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        // Only its log lives with Kiln; its state is the project's.
        fs::create_dir_all(dirs.instance())
            .map_err(|e| format!("cannot create {}: {e}", dirs.instance().display()))
    }

    /// Not used: an instance starts through [`Reverb::project_launch`].
    fn launch(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: dirs.bin.join("php"),
            ..Launch::default()
        }
    }

    /// `php artisan reverb:start` on 127.0.0.1 and the instance's port; the arguments win
    /// over the project's own `REVERB_SERVER_*`. Stopping ends the process: Reverb holds
    /// connections, not data.
    fn project_launch(
        &self,
        project: &Path,
        php: &Path,
        ports: &BTreeMap<String, u16>,
    ) -> Option<Launch> {
        Some(Launch {
            program: php.to_path_buf(),
            args: vec![
                "artisan".into(),
                "reverb:start".into(),
                "--host=127.0.0.1".into(),
                format!("--port={}", port_of(ports)),
            ],
            env: BTreeMap::new(),
            cwd: Some(project.to_path_buf()),
            stop_command: None,
        })
    }

    /// Where Reverb listens, and where the project's clients connect: the same loopback
    /// address. Browsers treat `127.0.0.1` as secure, so an HTTPS site may open
    /// `ws://127.0.0.1` too. The app's own keys (`REVERB_APP_*`) stay as they are.
    fn env_keys(&self, ports: &BTreeMap<String, u16>) -> Vec<(&'static str, String)> {
        let port = port_of(ports).to_string();
        vec![
            ("BROADCAST_CONNECTION", "reverb".to_owned()),
            ("REVERB_SERVER_HOST", "127.0.0.1".to_owned()),
            ("REVERB_SERVER_PORT", port.clone()),
            ("REVERB_HOST", "127.0.0.1".to_owned()),
            ("REVERB_PORT", port),
            ("REVERB_SCHEME", "http".to_owned()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_runs_artisan_in_the_project_on_loopback() {
        let ports = BTreeMap::from([("reverb".to_owned(), 8081)]);
        let launch = Reverb
            .project_launch(
                Path::new("/sites/shop"),
                Path::new("/k/php/8.5/php"),
                &ports,
            )
            .unwrap();
        assert_eq!(launch.program, Path::new("/k/php/8.5/php"));
        assert_eq!(
            launch.args,
            ["artisan", "reverb:start", "--host=127.0.0.1", "--port=8081"]
        );
        assert_eq!(launch.cwd.as_deref(), Some(Path::new("/sites/shop")));
    }

    #[test]
    fn the_installed_version_comes_from_composer_lock() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed_version(dir.path()), None);
        fs::write(
            dir.path().join("composer.lock"),
            r#"{"packages": [{"name": "laravel/framework", "version": "v12.1.0"},
                             {"name": "laravel/reverb", "version": "v1.6.3"}]}"#,
        )
        .unwrap();
        assert_eq!(installed_version(dir.path()).as_deref(), Some("1.6.3"));
    }
}
