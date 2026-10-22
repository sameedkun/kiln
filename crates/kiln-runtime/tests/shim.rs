//! The real `kiln-shim` binary acting as `php.exe` / `composer.exe` ([ADR 0016] §8–10), against
//! a throwaway Kiln state (its own APPDATA / LOCALAPPDATA), with two real PHP builds.
//!
//! Needs `KILN_TEST_PHP` and `KILN_TEST_PHP2`: two installed PHP folders of different minor
//! versions (e.g. `%LOCALAPPDATA%\Kiln\php\8.4.27` and `…\8.3.36`). `KILN_TEST_COMPOSER` (a
//! `composer.phar`) adds the Composer check. Without them the test only prints a note.
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

use kiln_state::{PhpRecord, StateStore};

fn env_dir(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    assert!(path.exists(), "{name}={} doesn't exist", path.display());
    Some(path)
}

/// The version a PHP folder holds, from its name (`…\8.4.27`).
fn version_of(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().into_owned()
}

struct Sandbox {
    _dir: tempfile::TempDir,
    appdata: PathBuf,
    local: PathBuf,
    sites: PathBuf,
}

impl Sandbox {
    fn run(&self, shim: &str, cwd: &Path, args: &[&str]) -> (i32, String) {
        let exe = self.local.join(r"Kiln\bin\shims").join(shim);
        let output = Command::new(exe)
            .args(args)
            .current_dir(cwd)
            .env("APPDATA", &self.appdata)
            .env("LOCALAPPDATA", &self.local)
            .output()
            .unwrap();
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.code().unwrap_or(-1), text)
    }
}

#[test]
fn php_and_composer_follow_each_site_s_pinned_version() {
    let (Some(newer), Some(older)) = (env_dir("KILN_TEST_PHP"), env_dir("KILN_TEST_PHP2")) else {
        eprintln!("skipped: set KILN_TEST_PHP and KILN_TEST_PHP2 to two PHP folders");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sandbox = Sandbox {
        appdata: dir.path().join("Roaming"),
        local: dir.path().join("Local"),
        sites: dir.path().join("Sites"),
        _dir: dir,
    };

    // State: both PHP builds installed, a parked folder with two projects, one pinned.
    let (mut store, _) = StateStore::open_dir(&sandbox.appdata.join("Kiln")).unwrap();
    for php in [&newer, &older] {
        store
            .put_php(&PhpRecord {
                version: version_of(php),
                dir: php.clone(),
                archive: String::new(),
                sha256: String::new(),
            })
            .unwrap();
    }
    for name in ["legacy", "modern"] {
        std::fs::create_dir_all(sandbox.sites.join(name).join("src")).unwrap();
        std::fs::write(sandbox.sites.join(name).join("index.php"), "").unwrap();
    }
    store.park(&sandbox.sites).unwrap();
    let older_version = version_of(&older);
    let older_minor = older_version.rsplit_once('.').unwrap().0;
    store.isolate("legacy.test", older_minor).unwrap();
    drop(store);

    // The shims, as `kiln shims install` writes them.
    let shims = sandbox.local.join(r"Kiln\bin\shims");
    std::fs::create_dir_all(&shims).unwrap();
    for name in ["php.exe", "composer.exe"] {
        std::fs::copy(env!("CARGO_BIN_EXE_kiln-shim"), shims.join(name)).unwrap();
    }

    let version = |cwd: &Path| sandbox.run("php.exe", cwd, &["-r", "echo PHP_VERSION;"]);
    assert_eq!(
        version(&sandbox.sites.join("legacy").join("src")),
        (0, version_of(&older))
    );
    assert_eq!(
        version(&sandbox.sites.join("modern")),
        (0, version_of(&newer))
    );
    assert_eq!(
        version(&sandbox.sites),
        (0, version_of(&newer)),
        "outside a site: default"
    );

    // Exit codes and awkward arguments pass through untouched.
    let modern = sandbox.sites.join("modern");
    assert_eq!(sandbox.run("php.exe", &modern, &["-r", "exit(7);"]).0, 7);
    let tricky = r#"a b "c" ^%PATH%! & | < >"#;
    let (code, out) = sandbox.run("php.exe", &modern, &["-r", "echo $argv[1];", tricky]);
    assert_eq!((code, out.as_str()), (0, tricky));

    // Composer runs with the site's PHP.
    let (code, out) = sandbox.run("composer.exe", &modern, &["--version"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("kiln composer install"), "{out}");
    if let Some(phar) = env_dir("KILN_TEST_COMPOSER") {
        std::fs::copy(phar, sandbox.local.join(r"Kiln\bin\composer.phar")).unwrap();
        let (code, out) = sandbox.run(
            "composer.exe",
            &sandbox.sites.join("legacy"),
            &["--version", "--no-ansi"],
        );
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("Composer version"), "{out}");
        assert!(
            out.contains(&format!("PHP version {}", version_of(&older))),
            "{out}"
        );
    }
}
