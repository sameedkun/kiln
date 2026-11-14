//! Stripe's webhooks for a site ([ADR 0073]): the Stripe CLI (Apache-2.0) running
//! `stripe listen` as one of the project's processes, forwarding Stripe's test events to the
//! site's `APP_URL` at Cashier's `/stripe/webhook`.
//!
//! [ADR 0073]: ../../../docs/adr/0073-stripe-webhooks.md

use std::collections::BTreeMap;
use std::path::Path;

use crate::{Launch, PortRole, Release, Runtime, ServiceProvider};

#[derive(Debug)]
pub struct Stripe;

/// Pinned from GitHub's asset digest, also published in `stripe-windows-checksums.txt`
/// (checked again by the Scenario). Released 2026-11-11.
const RELEASES: &[Release] = &[Release {
    version: "1.55.11",
    os: "windows",
    arch: "x86_64",
    archive: "stripe_1.55.11_windows_x86_64.zip",
    url: "https://github.com/stripe/stripe-cli/releases/download/v1.55.11/stripe_1.55.11_windows_x86_64.zip",
    sha256: "b869b8bb84d1ad4f7435a246ff83cca36337f9eb209264c9057675a99de6d31e",
    executable: "stripe.exe",
    tree: None,
}];

/// Cashier's webhook route, where events are forwarded.
pub const WEBHOOK_PATH: &str = "/stripe/webhook";

/// A key's value in a project's `.env` (unquoted), if set and not empty.
fn env_value(project: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(project.join(".env")).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key)
            .then(|| v.trim().trim_matches(['"', '\'']).to_owned())
            .filter(|v| !v.is_empty())
    })
}

/// Where `stripe listen` forwards to: the site's `APP_URL` and Cashier's route.
#[must_use]
pub fn forward_to(project: &Path) -> String {
    let base = env_value(project, "APP_URL").unwrap_or_else(|| "http://localhost".to_owned());
    format!("{}{WEBHOOK_PATH}", base.trim_end_matches('/'))
}

impl ServiceProvider for Stripe {
    fn kind(&self) -> &'static str {
        "stripe"
    }

    fn display_name(&self) -> &'static str {
        "Stripe webhooks"
    }

    fn license(&self) -> &'static str {
        "Apache-2.0 (the Stripe CLI)"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[]
    }

    fn releases(&self) -> &'static [Release] {
        RELEASES
    }

    fn executable_name(&self) -> &'static str {
        "kiln-stripe"
    }

    fn runs_in_project(&self) -> bool {
        true
    }

    /// Its own pinned program, not the project's PHP.
    fn runtime(&self) -> Runtime {
        Runtime::Own
    }

    /// The pinned CLI's version, once the project has what it needs: a Stripe secret key in
    /// its `.env` (Cashier's `STRIPE_SECRET`) and an `APP_URL` to forward to.
    fn project_version(&self, project: &Path) -> Result<String, String> {
        if env_value(project, "STRIPE_SECRET").is_none() {
            return Err(format!(
                "{} has no STRIPE_SECRET in its .env: add your Stripe test key (sk_test_…) first",
                project.display()
            ));
        }
        if env_value(project, "APP_URL").is_none() {
            return Err(format!(
                "{} has no APP_URL in its .env, which Stripe's events are forwarded to",
                project.display()
            ));
        }
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

    /// `stripe listen --forward-to <APP_URL>/stripe/webhook`. The secret key goes in the
    /// process's environment (`STRIPE_API_KEY`), read from the project's `.env` at each start:
    /// never on the command line, in Kiln's state or in History. Local certificates are
    /// Kiln's own, so an HTTPS site is forwarded to without checking them.
    fn project_launch(
        &self,
        project: &Path,
        runtime: &Path,
        _ports: &BTreeMap<String, u16>,
    ) -> Option<Launch> {
        let to = forward_to(project);
        let mut args = vec!["listen".to_owned(), "--forward-to".to_owned(), to.clone()];
        if to.starts_with("https://") {
            args.push("--skip-verify".to_owned());
        }
        let mut env = BTreeMap::from([("STRIPE_CLI_TELEMETRY_OPTOUT".to_owned(), "1".to_owned())]);
        if let Some(key) = env_value(project, "STRIPE_SECRET") {
            env.insert("STRIPE_API_KEY".to_owned(), key);
        }
        Some(Launch {
            program: runtime.to_path_buf(),
            args,
            env,
            cwd: Some(project.to_path_buf()),
            stop_command: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(env: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), env).unwrap();
        dir
    }

    #[test]
    fn it_forwards_to_the_sites_app_url_with_the_key_in_its_environment() {
        let dir = project("APP_URL=https://shop.test:8443/\nSTRIPE_SECRET=\"sk_test_123\"\n");
        let launch = Stripe
            .project_launch(dir.path(), Path::new("/k/stripe.exe"), &BTreeMap::new())
            .unwrap();
        assert_eq!(
            launch.args,
            [
                "listen",
                "--forward-to",
                "https://shop.test:8443/stripe/webhook",
                "--skip-verify"
            ]
        );
        assert_eq!(launch.env["STRIPE_API_KEY"], "sk_test_123");
        assert!(!launch.args.iter().any(|a| a.contains("sk_test")));
        assert_eq!(Stripe.project_version(dir.path()).unwrap(), "1.55.11");
    }

    #[test]
    fn a_project_without_a_key_or_url_is_refused_with_what_it_lacks() {
        let none = project("APP_URL=http://shop.test\nSTRIPE_SECRET=\n");
        assert!(
            Stripe
                .project_version(none.path())
                .unwrap_err()
                .contains("STRIPE_SECRET")
        );
        let no_url = project("STRIPE_SECRET=sk_test_1\n");
        assert!(
            Stripe
                .project_version(no_url.path())
                .unwrap_err()
                .contains("APP_URL")
        );
    }
}
