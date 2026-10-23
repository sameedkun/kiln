//! `kiln uninstall [--dry-run] [--keep-files] [--yes]` ([ADR 0018] §3–7): stop Kiln's
//! processes, undo every system change newest first, then delete Kiln's own folders, with the
//! state folder last. The full plan is always printed first.
//!
//! [ADR 0018]: ../../../docs/adr/0018-history-undo-uninstall.md

use std::path::{Path, PathBuf};

use kiln_ops::{Journal, OpState};
use kiln_sys::background::ProcStatus;

/// One of Kiln's folders, and whether it may be deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub path: PathBuf,
    pub bytes: u64,
    /// Why it must be kept, if it must.
    pub keep_because: Option<String>,
}

/// Everything uninstall will do, for the preview and the executor.
#[derive(Debug, Default)]
pub struct Plan {
    pub stop: Vec<String>,
    /// (operation, description), newest first.
    pub undo: Vec<(u64, String)>,
    /// (operation, description, state): half-done or stuck, need attention.
    pub attention: Vec<(u64, String, String)>,
    /// Local data first, the state folder last.
    pub folders: Vec<Folder>,
    /// The root key is in the desktop keyring (Linux), outside Kiln's folders.
    pub key_in_keyring: bool,
}

/// Whether `dir` looks like one of Kiln's folders: its last component is `Kiln` (any case),
/// and it isn't a drive or file-system root.
#[must_use]
pub fn is_kiln_dir(dir: &Path) -> bool {
    dir.parent().is_some_and(|p| p.parent().is_some())
        && dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("kiln"))
}

/// Decides, for each of Kiln's folders, whether it may be deleted ([ADR 0018] §5): it must be
/// a Kiln folder, and no project (linked or parked) may lie inside it.
#[must_use]
pub fn check_folders(candidates: &[(PathBuf, u64)], projects: &[PathBuf]) -> Vec<Folder> {
    candidates
        .iter()
        .map(|(path, bytes)| {
            let keep_because = if is_kiln_dir(path) {
                projects
                    .iter()
                    .find(|p| p.starts_with(path))
                    .map(|p| format!("the project {} is inside it", p.display()))
            } else {
                Some(format!(
                    "{} doesn't look like Kiln's folder",
                    path.display()
                ))
            };
            Folder {
                path: path.clone(),
                bytes: *bytes,
                keep_because,
            }
        })
        .collect()
}

/// The real path of `path` (long names, resolved case, no `\\?\` prefix); `path` itself if it
/// doesn't exist.
fn canonical(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(real) => {
            let text = real.to_string_lossy();
            match text.strip_prefix(r"\\?\") {
                Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
                _ => real,
            }
        }
        Err(_) => path.to_path_buf(),
    }
}

/// Total size of the files under `dir`, not following links or junctions.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match std::fs::symlink_metadata(entry.path()) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => dir_size(&entry.path()),
            Ok(meta) if meta.is_file() => meta.len(),
            _ => 0,
        })
        .sum()
}

fn mb(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // display only
    let mb = bytes as f64 / 1_048_576.0;
    format!("{mb:.1} MB")
}

fn build_plan() -> Result<Plan, String> {
    let mut plan = Plan::default();
    let web = crate::web::server()?;
    if let Ok(ProcStatus::Running { pid }) = web.status() {
        plan.stop.push(format!("Kiln's web server (PID {pid})"));
    }
    let supervisor = crate::pool::supervisor()?;
    if let Ok(ProcStatus::Running { pid }) = supervisor.status() {
        plan.stop
            .push(format!("Kiln's PHP workers (supervisor PID {pid})"));
    }

    let mut projects = Vec::new();
    if let Ok(store) = crate::web::open_store() {
        for entry in store
            .entries()
            .map_err(|e| e.to_string())?
            .into_iter()
            .rev()
        {
            let what = kiln_platform::describe::describe(&entry.kind, &entry.params);
            match entry.state {
                OpState::Applied => plan.undo.push((entry.id, what)),
                OpState::Prepared | OpState::Stuck => plan.attention.push((
                    entry.id,
                    what,
                    crate::apply::state_word(entry.state).to_owned(),
                )),
                OpState::RolledBack | OpState::Reverted => {}
            }
        }
        if let Ok(resolved) = kiln_runtime::sites(&store) {
            projects.extend(resolved.sites.into_iter().map(|s| s.project));
        }
        projects.extend(store.parked().unwrap_or_default());
    }

    let mut candidates = Vec::new();
    for dir in [
        kiln_state::default_local_dir(),
        kiln_state::default_state_dir(),
    ]
    .into_iter()
    .flatten()
    {
        if dir.exists() && !candidates.iter().any(|(d, _)| d == &dir) {
            let size = dir_size(&dir);
            candidates.push((dir, size));
        }
    }
    // Compare real, canonical paths: the same folder can be spelled differently, e.g. an 8.3
    // short name in LOCALAPPDATA (`C:\Users\RUNNER~1\…`) against the long name a project was
    // stored under. Comparing the spellings let a project inside Kiln's folder be deleted.
    let canonical_candidates: Vec<(PathBuf, u64)> = candidates
        .iter()
        .map(|(dir, size)| (canonical(dir), *size))
        .collect();
    let canonical_projects: Vec<PathBuf> = projects.iter().map(|p| canonical(p)).collect();
    plan.folders = check_folders(&canonical_candidates, &canonical_projects);
    plan.key_in_keyring = crate::apply::ca_store()
        .ok()
        .and_then(|ca| ca.root_key_description())
        .is_some_and(|kept| kept.contains("keyring"));
    Ok(plan)
}

fn print_plan(plan: &Plan, keep_files: bool) {
    println!("Uninstall plan:");
    println!("1. Stop");
    if plan.stop.is_empty() {
        println!("     (nothing running)");
    }
    for item in &plan.stop {
        println!("     {item}");
    }
    println!("2. Undo, newest first (each puts back what was there before)");
    if plan.undo.is_empty() {
        println!("     (no system changes in effect)");
    }
    for (id, what) in &plan.undo {
        println!("     #{id}  {what}");
    }
    for (id, what, state) in &plan.attention {
        println!(
            "     #{id}  {what}  [{state}: not undone automatically; see `kiln undo {id} --force`]"
        );
    }
    println!("3. Delete Kiln's folders");
    if keep_files {
        println!("     (skipped: --keep-files)");
    }
    for folder in &plan.folders {
        match &folder.keep_because {
            _ if keep_files => println!("     keep {}", folder.path.display()),
            Some(why) => println!("     keep {} ({why})", folder.path.display()),
            None => println!("     {}  ({})", folder.path.display(), mb(folder.bytes)),
        }
    }
    if plan.folders.is_empty() {
        println!("     (none exist)");
    }
    if plan.key_in_keyring && !keep_files {
        println!("     and the CA root key's entry in your desktop keyring");
    }
    println!(
        "Never touched: your project folders (linked or parked), other tools such as Laravel Herd"
    );
    println!("and other PHP installs, and the kiln programs themselves.");
}

pub fn uninstall(dry_run: bool, keep_files: bool, yes: bool) -> Result<(), String> {
    let plan = build_plan()?;
    print_plan(&plan, keep_files);
    if dry_run {
        println!("Dry run: nothing was changed.");
        return Ok(());
    }
    if !yes && !crate::apply::confirm("Uninstall Kiln as shown?") {
        println!("Nothing was changed.");
        return Ok(());
    }

    // 1. Stop.
    if let Some(pid) = crate::web::server()?.stop().map_err(|e| e.to_string())? {
        println!("Stopped the web server (PID {pid}).");
    }
    if let Some(pid) = crate::pool::supervisor()?
        .stop()
        .map_err(|e| e.to_string())?
    {
        println!("Stopped the PHP workers (supervisor PID {pid}).");
    }

    // 2. Undo, newest first; stop at the first failure and delete nothing.
    if !plan.undo.is_empty() {
        let undone = crate::apply::undo_all().map_err(|e| {
            format!("{e}\nUninstall stopped here and deleted nothing; fix the cause and run `kiln uninstall` again.")
        })?;
        println!("Undid {} operation(s).", undone.len());
    }
    if keep_files {
        println!("Done. Kiln's folders were kept (--keep-files).");
        return Ok(());
    }
    // The root key may live outside Kiln's folders (the Secret Service, ADR 0021 §3).
    if let Err(e) =
        crate::apply::ca_store().and_then(|ca| ca.forget_root_key().map_err(|e| e.to_string()))
    {
        println!("Couldn't remove the CA root key from your keyring: {e}");
    } else if plan.key_in_keyring {
        println!("Removed the CA root key from your keyring.");
    }

    // 3. Delete; the state folder only if nothing is left in effect or stuck.
    let unresolved = crate::web::open_store()
        .and_then(|store| store.entries().map_err(|e| e.to_string()))
        .map_or(0, |entries| {
            entries
                .iter()
                .filter(|e| {
                    matches!(
                        e.state,
                        OpState::Applied | OpState::Prepared | OpState::Stuck
                    )
                })
                .count()
        });
    // Canonical, like the planned folders, so the comparison below can't miss on spelling.
    let state_dir = kiln_state::default_state_dir().map(|d| canonical(&d));
    for folder in &plan.folders {
        if let Some(why) = &folder.keep_because {
            println!("Kept {} ({why}).", folder.path.display());
            continue;
        }
        if unresolved > 0 && Some(&folder.path) == state_dir.as_ref() {
            println!(
                "Kept {}: {unresolved} operation(s) still need attention (`kiln history`).",
                folder.path.display()
            );
            continue;
        }
        match std::fs::remove_dir_all(&folder.path) {
            Ok(()) => println!("Deleted {}.", folder.path.display()),
            Err(e) => println!("Couldn't delete {}: {e}", folder.path.display()),
        }
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    println!(
        "Kiln is uninstalled. The kiln programs themselves{} are left in place.",
        exe_dir.map_or_else(String::new, |d| format!(" ({})", d.display()))
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(parts: &[&str]) -> PathBuf {
        parts.iter().collect()
    }

    #[test]
    fn only_folders_named_kiln_below_a_parent_count() {
        assert!(is_kiln_dir(&p(&["home", "dev", "AppData", "Kiln"])));
        assert!(is_kiln_dir(&p(&["home", ".local", "state", "kiln"])));
        assert!(!is_kiln_dir(&p(&["home", "dev", "Projects"])));
        assert!(!is_kiln_dir(&p(&["Kiln"])), "no parent");
    }

    #[test]
    fn a_folder_holding_a_project_is_kept() {
        let local = p(&["Users", "dev", "AppData", "Local", "Kiln"]);
        let roaming = p(&["Users", "dev", "AppData", "Roaming", "Kiln"]);
        let odd = p(&["Users", "dev", "Documents"]);
        let projects = [
            p(&["Sites", "blog"]),
            local.join("php").join("weird-project"),
        ];
        let folders = check_folders(
            &[
                (local.clone(), 10),
                (roaming.clone(), 20),
                (odd.clone(), 30),
            ],
            &projects,
        );
        assert!(
            folders[0]
                .keep_because
                .as_deref()
                .unwrap()
                .contains("weird-project")
        );
        assert_eq!(folders[1].keep_because, None);
        assert!(
            folders[2]
                .keep_because
                .as_deref()
                .unwrap()
                .contains("doesn't look like")
        );
    }

    #[test]
    fn sizes_add_up_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a").join("b")).unwrap();
        std::fs::write(dir.path().join("a").join("one"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.path().join("a").join("b").join("two"), vec![0u8; 50]).unwrap();
        assert_eq!(dir_size(dir.path()), 150);
        assert_eq!(dir_size(&dir.path().join("missing")), 0);
        assert_eq!(mb(1_572_864), "1.5 MB");
    }
}
