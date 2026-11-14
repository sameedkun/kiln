//! Sharing a site through a Cloudflare quick tunnel ([ADR 0076]): `cloudflared` (Apache-2.0)
//! as one of the project's processes. It connects out to Cloudflare and forwards to Kiln's own
//! web server on loopback, with the site's name as the host, so nothing new listens on the
//! network; the address it prints (`https://….trycloudflare.com`) reaches the site from
//! anywhere while it runs.
//!
//! [ADR 0076]: ../../../docs/adr/0076-sharing-sites.md

use std::collections::BTreeMap;
use std::path::Path;

use crate::{Launch, PortRole, Release, Runtime, ServiceProvider};

#[derive(Debug)]
pub struct Tunnel;

/// The download is the program itself. Pinned from GitHub's asset digest, the same SHA-256
/// the release notes list. Released 2026-11-12.
const RELEASES: &[Release] = &[Release {
    version: "2026.11.0",
    os: "windows",
    arch: "x86_64",
    archive: "cloudflared-windows-amd64.exe",
    url: "https://github.com/cloudflare/cloudflared/releases/download/2026.11.0/cloudflared-windows-amd64.exe",
    sha256: "d5c1d82031da6253dcdcff64c944cd0fe063016edeb2e4fd088075fc9bd52d0a",
    executable: "cloudflared-windows-amd64.exe",
    tree: None,
}];

/// The public address a quick tunnel prints once it's up, if `log` has one (the newest).
#[must_use]
pub fn shared_url(log: &str) -> Option<String> {
    log.lines().rev().find_map(|line| {
        let start = line.find("https://")?;
        let rest = &line[start..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-')))
            .unwrap_or(rest.len());
        let url = &rest[..end];
        url.ends_with(".trycloudflare.com").then(|| url.to_owned())
    })
}

impl ServiceProvider for Tunnel {
    fn kind(&self) -> &'static str {
        "tunnel"
    }

    fn display_name(&self) -> &'static str {
        "Sharing (Cloudflare quick tunnel)"
    }

    fn license(&self) -> &'static str {
        "Apache-2.0 (cloudflared)"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[]
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-cloudflared"
    }

    fn runs_in_project(&self) -> bool {
        true
    }

    fn runtime(&self) -> Runtime {
        Runtime::Own
    }

    /// It forwards to the site serving the project: Kiln adds where (`shares_site`).
    fn shares_site(&self) -> bool {
        true
    }

    fn project_version(&self, _project: &Path) -> Result<String, String> {
        Ok(RELEASES[0].version.to_owned())
    }

    fn init_data(
        &self,
        dirs: &crate::InstanceDirs,
        _ports: &BTreeMap<String, u16>,
    ) -> Result<(), String> {
        std::fs::create_dir_all(&dirs.data)
            .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))
    }

    /// Unused: it runs in a project, through [`ServiceProvider::project_launch`].
    fn launch(&self, dirs: &crate::InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: self.executable(&dirs.bin),
            ..Launch::default()
        }
    }

    /// `cloudflared tunnel --no-autoupdate`: Kiln adds `--url`, `--http-host-header` and an
    /// empty `--config` of its own (so a `~/.cloudflared/config.yml` never applies).
    fn project_launch(
        &self,
        project: &Path,
        runtime: &Path,
        _ports: &BTreeMap<String, u16>,
    ) -> Option<Launch> {
        Some(Launch {
            program: runtime.to_path_buf(),
            args: vec!["tunnel".to_owned(), "--no-autoupdate".to_owned()],
            env: BTreeMap::new(),
            cwd: Some(project.to_path_buf()),
            stop_command: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_address_is_read_from_its_log() {
        let log = "2026-11-14T10:00:00Z INF Requesting new quick Tunnel on trycloudflare.com...\n\
                   2026-11-14T10:00:02Z INF |  https://quiet-river-1234.trycloudflare.com                    |\n\
                   2026-11-14T10:00:03Z INF Registered tunnel connection";
        assert_eq!(
            shared_url(log).as_deref(),
            Some("https://quiet-river-1234.trycloudflare.com")
        );
        assert_eq!(
            shared_url("INF see https://developers.cloudflare.com/x"),
            None
        );
        assert_eq!(shared_url(""), None);
    }
}
