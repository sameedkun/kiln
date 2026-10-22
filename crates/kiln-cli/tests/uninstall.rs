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
    /// The sandbox root is spelled with 8.3 short names where the volume has them (as
    /// `LOCALAPPDATA` is on GitHub's runners), so Kiln must compare real paths, not spellings.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = short_path(dir.path());
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

/// `path` in 8.3 form (`cmd`'s `%~s`), or unchanged where short names aren't available.
fn short_path(path: &Path) -> PathBuf {
    use std::os::windows::process::CommandExt;
    // raw_arg: cmd doesn't understand the C-style quote escaping `arg` would add.
    let output = Command::new("cmd")
        .raw_arg(format!(
            "/C for %I in (\"{}\") do @echo %~sI",
            path.display()
        ))
        .output()
        .unwrap();
    let short = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && !short.is_empty() && Path::new(&short).is_dir() {
        PathBuf::from(short)
    } else {
        path.to_path_buf()
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
    // The plan names folders by their real (long) path, however APPDATA spells them.
    for dir in [sandbox.local().join("Kiln"), sandbox.roaming().join("Kiln")] {
        let real = std::fs::canonicalize(&dir).unwrap();
        let real = real.display().to_string();
        let real = real.trim_start_matches(r"\\?\");
        assert!(out.contains(real), "{real} not in:\n{out}");
    }
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
