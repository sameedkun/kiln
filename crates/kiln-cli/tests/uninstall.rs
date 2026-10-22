//! `kiln uninstall` with the real binary in a sandbox (its own APPDATA / LOCALAPPDATA): the
//! dry run changes nothing; a real run deletes only Kiln's folders, never projects, and never
//! follows a junction out of them ([ADR 0018] §3–5).
//!
//! No journaled operations are created here: those touch the real hosts file, trust store or
//! PATH. Undoing them in order is covered by the engine's own tests (`revert_all`).
//!
//! [ADR 0018]: ../../../docs/adr/0018-history-undo-uninstall.md

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

struct Sandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        Self { _dir: dir, root }
    }

    fn roaming(&self) -> PathBuf {
        self.root.join("Roaming")
    }

    fn local(&self) -> PathBuf {
        self.root.join("Local")
    }

    fn kiln(&self, args: &[&str]) -> (bool, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_kiln"))
            .args(args)
            .env("APPDATA", self.roaming())
            .env("LOCALAPPDATA", self.local())
            .output()
            .unwrap();
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.success(), text)
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn dry_run_changes_nothing_and_uninstall_removes_only_kiln_s_folders() {
    let sandbox = Sandbox::new();
    let sites = sandbox.root.join("Sites");
    write(&sites.join("blog").join("index.php"), "<?php echo 'mine';");
    write(&sandbox.local().join(r"Kiln\php\8.4.27\php.exe"), "MZ");
    write(&sandbox.local().join(r"Kiln\downloads\x.zip"), "zip");

    // Something outside Kiln's folders, reachable through a junction inside them.
    let outside = sandbox.root.join("Outside");
    write(&outside.join("precious.txt"), "keep me");
    let junction = sandbox.local().join(r"Kiln\link-out");
    let made = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stdout)
    );

    let (ok, out) = sandbox.kiln(&["park", sites.to_str().unwrap()]);
    assert!(ok, "{out}");
    let (ok, out) = sandbox.kiln(&["history"]);
    assert!(ok && out.contains("hasn't changed anything"), "{out}");

    let (ok, out) = sandbox.kiln(&["uninstall", "--dry-run"]);
    assert!(ok, "{out}");
    assert!(out.contains("no system changes in effect"), "{out}");
    assert!(
        out.contains(&sandbox.local().join("Kiln").display().to_string()),
        "{out}"
    );
    assert!(
        out.contains(&sandbox.roaming().join("Kiln").display().to_string()),
        "{out}"
    );
    assert!(out.contains("Dry run: nothing was changed."), "{out}");
    assert!(sandbox.local().join("Kiln").is_dir() && sandbox.roaming().join("Kiln").is_dir());
    let (_, same) = sandbox.kiln(&["plan", "uninstall"]);
    assert!(
        same.contains("Dry run"),
        "kiln plan uninstall is the dry run"
    );

    let (ok, out) = sandbox.kiln(&["uninstall", "--yes"]);
    assert!(ok, "{out}");
    assert!(!sandbox.local().join("Kiln").exists(), "{out}");
    assert!(!sandbox.roaming().join("Kiln").exists(), "{out}");
    assert!(
        sites.join("blog").join("index.php").is_file(),
        "projects are never touched"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("precious.txt")).unwrap(),
        "keep me",
        "the junction was removed, not followed"
    );
}

#[test]
fn a_kiln_folder_holding_a_project_is_kept_and_keep_files_keeps_everything() {
    let sandbox = Sandbox::new();
    let inside = sandbox.local().join(r"Kiln\odd-place\shop");
    write(&inside.join("index.php"), "<?php");
    let (ok, out) = sandbox.kiln(&["link", "shop", "--path", inside.to_str().unwrap()]);
    assert!(ok, "{out}");

    let (ok, out) = sandbox.kiln(&["uninstall", "--yes", "--keep-files"]);
    assert!(ok, "{out}");
    assert!(
        sandbox.roaming().join("Kiln").is_dir(),
        "--keep-files: {out}"
    );

    let (ok, out) = sandbox.kiln(&["uninstall", "--yes"]);
    assert!(ok, "{out}");
    assert!(out.contains("is inside it"), "{out}");
    assert!(inside.join("index.php").is_file(), "the project survives");
    assert!(!sandbox.roaming().join("Kiln").exists(), "{out}");
}
