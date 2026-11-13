//! A project's development processes ([ADR 0060]): Laravel's queue worker and scheduler, and
//! Vite's dev server, run in the project like Reverb ([ADR 0047]) and kept up like any
//! service.
//!
//! [ADR 0047]: ../../../docs/adr/0047-reverb.md
//! [ADR 0060]: ../../../docs/adr/0060-dev-processes.md

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::{InstanceDirs, Launch, PortRole, Release, Runtime, ServiceProvider};

/// A package's version in a project's `composer.lock`, without a leading `v`.
#[must_use]
pub fn composer_version(project: &Path, package: &str) -> Option<String> {
    let lock = fs::read_to_string(project.join("composer.lock")).ok()?;
    let lock: serde_json::Value = serde_json::from_str(&lock).ok()?;
    lock["packages"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == package)?["version"]
        .as_str()
        .map(|v| v.trim_start_matches('v').to_owned())
}

/// The Laravel version of a project that has `artisan`, or why it can't run Laravel's
/// processes.
fn laravel_version(project: &Path) -> Result<String, String> {
    if !project.join("artisan").is_file() {
        return Err(format!(
            "{} isn't a Laravel project (it has no artisan)",
            project.display()
        ));
    }
    composer_version(project, "laravel/framework").ok_or_else(|| {
        format!(
            "{} has no laravel/framework installed: run `composer install` there first",
            project.display()
        )
    })
}

/// Only a log lives with Kiln; the process's state is the project's.
fn log_folder(dirs: &InstanceDirs) -> Result<(), String> {
    fs::create_dir_all(dirs.instance())
        .map_err(|e| format!("cannot create {}: {e}", dirs.instance().display()))
}

/// `php artisan <args>` in the project.
fn artisan(project: &Path, php: &Path, args: &[&str]) -> Launch {
    Launch {
        program: php.to_path_buf(),
        args: std::iter::once("artisan")
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect(),
        env: BTreeMap::new(),
        cwd: Some(project.to_path_buf()),
        stop_command: None,
    }
}

/// Laravel's queue worker: `php artisan queue:listen`, which starts a fresh worker for each
/// job, so changed code is picked up without a restart.
#[derive(Debug)]
pub struct Queue;

impl ServiceProvider for Queue {
    fn kind(&self) -> &'static str {
        "queue"
    }

    fn display_name(&self) -> &'static str {
        "Queue worker"
    }

    fn license(&self) -> &'static str {
        "the project's own Laravel"
    }

    /// It listens on nothing: it takes jobs from the queue the project's `.env` names.
    fn ports(&self) -> &'static [PortRole] {
        &[]
    }

    fn releases(&self) -> &'static [Release] {
        &[]
    }

    fn executable_name(&self) -> &'static str {
        "php"
    }

    fn runs_in_project(&self) -> bool {
        true
    }

    fn project_version(&self, project: &Path) -> Result<String, String> {
        laravel_version(project)
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        log_folder(dirs)
    }

    /// Not used: an instance starts through [`Queue::project_launch`].
    fn launch(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: dirs.bin.join("php"),
            ..Launch::default()
        }
    }

    fn project_launch(
        &self,
        project: &Path,
        runtime: &Path,
        _ports: &BTreeMap<String, u16>,
    ) -> Option<Launch> {
        Some(artisan(project, runtime, &["queue:listen"]))
    }
}

/// Laravel's scheduler: `php artisan schedule:work`, which runs the schedule every minute.
#[derive(Debug)]
pub struct Scheduler;

impl ServiceProvider for Scheduler {
    fn kind(&self) -> &'static str {
        "scheduler"
    }

    fn display_name(&self) -> &'static str {
        "Scheduler"
    }

    fn license(&self) -> &'static str {
        "the project's own Laravel"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[]
    }

    fn releases(&self) -> &'static [Release] {
        &[]
    }

    fn executable_name(&self) -> &'static str {
        "php"
    }

    fn runs_in_project(&self) -> bool {
        true
    }

    fn project_version(&self, project: &Path) -> Result<String, String> {
        laravel_version(project)
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        log_folder(dirs)
    }

    /// Not used: an instance starts through [`Scheduler::project_launch`].
    fn launch(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: dirs.bin.join("php"),
            ..Launch::default()
        }
    }

    fn project_launch(
        &self,
        project: &Path,
        runtime: &Path,
        _ports: &BTreeMap<String, u16>,
    ) -> Option<Launch> {
        Some(artisan(project, runtime, &["schedule:work"]))
    }
}

/// Vite's dev server, from the project's own `node_modules`, run with the Node Kiln picks for
/// the folder.
#[derive(Debug)]
pub struct Vite;

const VITE_PORTS: &[PortRole] = &[PortRole {
    role: "vite",
    usual: 5173,
    what: "the dev server pages load scripts and styles from",
}];

/// Where Vite's command line is in a project.
const VITE_BIN: &str = "node_modules/vite/bin/vite.js";

fn vite_port(ports: &BTreeMap<String, u16>) -> u16 {
    ports
        .get(VITE_PORTS[0].role)
        .copied()
        .unwrap_or(VITE_PORTS[0].usual)
}

/// The address Vite serves on, as Laravel's Vite plugin writes it to `public/hot`.
fn vite_url(ports: &BTreeMap<String, u16>) -> String {
    format!("http://127.0.0.1:{}", vite_port(ports))
}

impl ServiceProvider for Vite {
    fn kind(&self) -> &'static str {
        "vite"
    }

    fn display_name(&self) -> &'static str {
        "Vite"
    }

    fn license(&self) -> &'static str {
        "MIT, the project's own vite package"
    }

    fn ports(&self) -> &'static [PortRole] {
        VITE_PORTS
    }

    fn releases(&self) -> &'static [Release] {
        &[]
    }

    fn executable_name(&self) -> &'static str {
        "node"
    }

    fn runs_in_project(&self) -> bool {
        true
    }

    fn runtime(&self) -> Runtime {
        Runtime::Node
    }

    fn project_version(&self, project: &Path) -> Result<String, String> {
        let manifest = project
            .join("node_modules")
            .join("vite")
            .join("package.json");
        let text = fs::read_to_string(&manifest).map_err(|_| {
            if project.join("package.json").is_file() {
                format!(
                    "{} has no Vite installed: run `npm install` there first",
                    project.display()
                )
            } else {
                format!(
                    "{} has no package.json, so no Vite to run",
                    project.display()
                )
            }
        })?;
        serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["version"].as_str().map(str::to_owned))
            .ok_or_else(|| format!("cannot read Vite's version from {}", manifest.display()))
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        log_folder(dirs)
    }

    /// Not used: an instance starts through [`Vite::project_launch`].
    fn launch(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: dirs.bin.join("node"),
            ..Launch::default()
        }
    }

    /// `node node_modules/vite/bin/vite.js` on 127.0.0.1 and the instance's port, refusing
    /// another port if that one is taken (`--strictPort`), so `public/hot` names the port
    /// Kiln gave it. Not `npm run dev`: ending npm can leave Vite running.
    fn project_launch(
        &self,
        project: &Path,
        runtime: &Path,
        ports: &BTreeMap<String, u16>,
    ) -> Option<Launch> {
        Some(Launch {
            program: runtime.to_path_buf(),
            args: vec![
                VITE_BIN.to_owned(),
                "--host=127.0.0.1".to_owned(),
                format!("--port={}", vite_port(ports)),
                "--strictPort".to_owned(),
            ],
            env: BTreeMap::new(),
            cwd: Some(project.to_path_buf()),
            stop_command: None,
        })
    }

    /// Laravel's Vite plugin removes `public/hot` when Vite exits cleanly; ended on Windows,
    /// it can't, and the site would keep asking a stopped server for its scripts. Only a
    /// `hot` naming this instance's address is removed.
    fn after_stop(&self, project: &Path, ports: &BTreeMap<String, u16>) {
        let hot = project.join("public").join("hot");
        if fs::read_to_string(&hot).is_ok_and(|text| text.trim() == vite_url(ports)) {
            let _ = fs::remove_file(hot);
        }
    }
}

/// Symfony `VarDumper`'s dump server ([ADR 0062]): `dump()` output from the project's
/// requests, sent to it instead of into the page, and kept in its log for the Logs page and the
/// site's Dumps tab. The project brings it: `symfony/var-dumper`, which Laravel requires.
///
/// [ADR 0062]: ../../../docs/adr/0062-dump-capture.md
#[derive(Debug)]
pub struct Dumps;

const DUMPS_PORTS: &[PortRole] = &[PortRole {
    role: "dumps",
    usual: 9912,
    what: "where the project sends dump() output",
}];

/// Where the dump server's command line is in a project.
const DUMP_SERVER: &str = "vendor/symfony/var-dumper/Resources/bin/var-dump-server";

fn dumps_port(ports: &BTreeMap<String, u16>) -> u16 {
    ports
        .get(DUMPS_PORTS[0].role)
        .copied()
        .unwrap_or(DUMPS_PORTS[0].usual)
}

impl ServiceProvider for Dumps {
    fn kind(&self) -> &'static str {
        "dumps"
    }

    fn display_name(&self) -> &'static str {
        "Dumps"
    }

    fn license(&self) -> &'static str {
        "MIT, the project's own symfony/var-dumper"
    }

    fn ports(&self) -> &'static [PortRole] {
        DUMPS_PORTS
    }

    fn releases(&self) -> &'static [Release] {
        &[]
    }

    fn executable_name(&self) -> &'static str {
        "php"
    }

    fn runs_in_project(&self) -> bool {
        true
    }

    fn project_version(&self, project: &Path) -> Result<String, String> {
        if !project.join(DUMP_SERVER).is_file() {
            return Err(format!(
                "{} has no Symfony dump server: run `composer require --dev symfony/var-dumper` there first (Laravel projects have it after `composer install`)",
                project.display()
            ));
        }
        composer_version(project, "symfony/var-dumper").ok_or_else(|| {
            format!(
                "cannot read symfony/var-dumper's version from {}'s composer.lock",
                project.display()
            )
        })
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        log_folder(dirs)
    }

    /// Not used: an instance starts through [`Dumps::project_launch`].
    fn launch(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Launch {
        Launch {
            program: dirs.bin.join("php"),
            ..Launch::default()
        }
    }

    /// `php vendor/symfony/var-dumper/Resources/bin/var-dump-server` on 127.0.0.1, printing
    /// each dump as text (with where it came from) into its log.
    fn project_launch(
        &self,
        project: &Path,
        runtime: &Path,
        ports: &BTreeMap<String, u16>,
    ) -> Option<Launch> {
        Some(Launch {
            program: runtime.to_path_buf(),
            args: vec![
                DUMP_SERVER.to_owned(),
                format!("--host=127.0.0.1:{}", dumps_port(ports)),
                "--format=cli".to_owned(),
            ],
            env: BTreeMap::new(),
            cwd: Some(project.to_path_buf()),
            stop_command: None,
        })
    }

    /// `dump()` goes to the server instead of the page. `dd()` still ends the request.
    fn env_keys(&self, ports: &BTreeMap<String, u16>) -> Vec<(&'static str, String)> {
        vec![
            ("VAR_DUMPER_FORMAT", "server".to_owned()),
            (
                "VAR_DUMPER_SERVER",
                format!("127.0.0.1:{}", dumps_port(ports)),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dump_server_runs_from_vendor_on_loopback_and_is_wired_by_env() {
        let ports = BTreeMap::from([("dumps".to_owned(), 9913)]);
        let launch = Dumps
            .project_launch(Path::new("/sites/shop"), Path::new("/k/php/php"), &ports)
            .unwrap();
        assert_eq!(
            launch.args,
            [
                "vendor/symfony/var-dumper/Resources/bin/var-dump-server",
                "--host=127.0.0.1:9913",
                "--format=cli"
            ]
        );
        assert_eq!(
            Dumps.env_keys(&ports),
            [
                ("VAR_DUMPER_FORMAT", "server".to_owned()),
                ("VAR_DUMPER_SERVER", "127.0.0.1:9913".to_owned())
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        assert!(
            Dumps
                .project_version(dir.path())
                .unwrap_err()
                .contains("symfony/var-dumper")
        );
    }

    #[test]
    fn laravel_processes_run_artisan_in_the_project() {
        let php = Path::new("/k/php/8.4/php");
        let project = Path::new("/sites/shop");
        let queue = Queue
            .project_launch(project, php, &BTreeMap::new())
            .unwrap();
        assert_eq!(queue.program, php);
        assert_eq!(queue.args, ["artisan", "queue:listen"]);
        assert_eq!(queue.cwd.as_deref(), Some(project));
        let scheduler = Scheduler
            .project_launch(project, php, &BTreeMap::new())
            .unwrap();
        assert_eq!(scheduler.args, ["artisan", "schedule:work"]);
        assert!(Queue.ports().is_empty() && Scheduler.ports().is_empty());
    }

    #[test]
    fn vite_runs_from_node_modules_on_loopback_and_its_port_only() {
        let ports = BTreeMap::from([("vite".to_owned(), 5174)]);
        let launch = Vite
            .project_launch(Path::new("/sites/shop"), Path::new("/k/node/node"), &ports)
            .unwrap();
        assert_eq!(launch.program, Path::new("/k/node/node"));
        assert_eq!(
            launch.args,
            [
                "node_modules/vite/bin/vite.js",
                "--host=127.0.0.1",
                "--port=5174",
                "--strictPort"
            ]
        );
        assert_eq!(Vite.runtime(), Runtime::Node);
    }

    #[test]
    fn versions_come_from_the_project_or_say_what_to_run() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        assert!(
            Queue
                .project_version(project)
                .unwrap_err()
                .contains("no artisan")
        );
        fs::write(project.join("artisan"), "<?php").unwrap();
        assert!(
            Queue
                .project_version(project)
                .unwrap_err()
                .contains("composer install")
        );
        fs::write(
            project.join("composer.lock"),
            r#"{"packages": [{"name": "laravel/framework", "version": "v12.38.1"}]}"#,
        )
        .unwrap();
        assert_eq!(Scheduler.project_version(project).unwrap(), "12.38.1");

        assert!(
            Vite.project_version(project)
                .unwrap_err()
                .contains("no package.json")
        );
        fs::write(project.join("package.json"), "{}").unwrap();
        assert!(
            Vite.project_version(project)
                .unwrap_err()
                .contains("npm install")
        );
        let vite = project.join("node_modules").join("vite");
        fs::create_dir_all(&vite).unwrap();
        fs::write(vite.join("package.json"), r#"{"version": "7.1.2"}"#).unwrap();
        assert_eq!(Vite.project_version(project).unwrap(), "7.1.2");
    }

    #[test]
    fn a_stopped_vite_takes_only_its_own_hot_file() {
        let dir = tempfile::tempdir().unwrap();
        let public = dir.path().join("public");
        fs::create_dir_all(&public).unwrap();
        let ports = BTreeMap::from([("vite".to_owned(), 5173)]);
        // Another dev server's: kept.
        fs::write(public.join("hot"), "http://[::1]:5173").unwrap();
        Vite.after_stop(dir.path(), &ports);
        assert!(public.join("hot").exists());
        // Its own: removed.
        fs::write(public.join("hot"), "http://127.0.0.1:5173\n").unwrap();
        Vite.after_stop(dir.path(), &ports);
        assert!(!public.join("hot").exists());
        // Nothing to remove: nothing happens.
        Vite.after_stop(dir.path(), &ports);
    }
}
