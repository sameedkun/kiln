//! `kiln uninstall [--dry-run] [--keep-files] [--yes] [--delete-data]` ([ADR 0018] §3–7): stop
//! processes, undo every system change newest first, then delete Kiln's own folders, with the
//! state folder last. The full plan is always printed first.
//!
//! [ADR 0018]: ../../../docs/adr/0018-history-undo-uninstall.md

use std::path::{Path, PathBuf};

use kiln_ops::{Journal, OpState};
use kiln_platform::env_file::ENV_KEYS_KIND;
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
    /// Services' data folders with something in them, and their sizes: deleted only with
    /// `--delete-data` ([ADR 0036] §9).
    ///
    /// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
    pub service_data: Vec<(PathBuf, u64)>,
    /// (operation, description): projects' `.env` files Kiln pointed at its services. They
    /// are the projects' own, so uninstall leaves them ([ADR 0045] §6).
    ///
    /// [ADR 0045]: ../../../docs/adr/0045-env-wiring.md
    pub wired: Vec<(u64, String)>,
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
pub(crate) fn dir_size(dir: &Path) -> u64 {
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

pub(crate) fn mb(bytes: u64) -> String {
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
    if let Ok(ProcStatus::Running { pid }) = crate::service::supervisor()?.status() {
        plan.stop
            .push(format!("Kiln's services (supervisor PID {pid})"));
    }
    plan.service_data = crate::service::data_folders();

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
                OpState::Applied if entry.kind == ENV_KEYS_KIND => {
                    plan.wired.push((entry.id, what));
                }
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
    // The web tools' sites are Kiln's own files, not projects (ADR 0043).
    let tools = crate::tool::tools_dir().ok().map(|dir| canonical(&dir));
    let canonical_projects: Vec<PathBuf> = projects
        .iter()
        .map(|p| canonical(p))
        .filter(|p| tools.as_ref().is_none_or(|tools| !p.starts_with(tools)))
        .collect();
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
    if !plan.wired.is_empty() {
        println!(
            "   Left as they are: projects' .env files Kiln pointed at its services (`kiln undo <number>` before uninstalling puts one back)"
        );
        for (id, what) in &plan.wired {
            println!("     #{id}  {what}");
        }
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
    if !plan.service_data.is_empty() && !keep_files {
        println!(
            "     including the services' data (--keep-data copies it to your home folder first; --delete-data deletes it):"
        );
        for (path, bytes) in &plan.service_data {
            println!("       {}  ({})", path.display(), mb(*bytes));
        }
    }
    println!(
        "Never touched: your project folders (linked or parked), other tools such as Laravel Herd"
    );
    println!("and other PHP installs, and the kiln programs themselves.");
}

/// `kiln uninstall`'s flags.
#[derive(Debug, Clone, Copy, Default, clap::Args)]
#[allow(clippy::struct_excessive_bools)] // one per flag
pub struct Options {
    /// Show the plan and change nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Stop and undo system changes, but keep Kiln's folders (e.g. before reinstalling).
    #[arg(long)]
    pub keep_files: bool,
    /// Do not ask for confirmation (the plan is still printed).
    #[arg(long, short)]
    pub yes: bool,
    /// Also delete the services' data (databases, caught mail). Without it, uninstall
    /// refuses while there is any.
    #[arg(long)]
    pub delete_data: bool,
    /// Copy the services' data to `Kiln data (<date>)` in your home folder, then uninstall.
    #[arg(long, conflicts_with = "delete_data")]
    pub keep_data: bool,
}

/// `Kiln data (<date>)` in the home folder, with ` 2`, ` 3`… if that name is taken.
fn kept_data_folder(home: &Path, date: &str) -> PathBuf {
    let base = format!("Kiln data ({date})");
    let mut path = home.join(&base);
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = home.join(format!("{base} {n}"));
    }
    path
}

/// Copies `from` into a new folder `to`, file by file; returns the bytes copied.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<u64> {
    std::fs::create_dir_all(to)?;
    let mut bytes = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            bytes += copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            bytes += std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(bytes)
}

/// `--keep-data` ([ADR 0049] §3): each instance's data folder copied to
/// `~/Kiln data (<date>)/<instance>`, checked by size. Returns that folder.
///
/// [ADR 0049]: ../../../docs/adr/0049-data-safety.md
fn keep_data(data: &[(PathBuf, u64)]) -> Result<PathBuf, String> {
    let home = std::env::home_dir().ok_or("cannot find your home folder")?;
    let today = time::OffsetDateTime::now_utc().date().to_string();
    let kept = kept_data_folder(&home, &today);
    for (folder, _) in data {
        // Measured now: a database writes on its way down, after the plan's sizes were taken.
        let size = dir_size(folder);
        let instance = folder
            .parent()
            .and_then(Path::file_name)
            .ok_or_else(|| format!("{} has no instance folder", folder.display()))?;
        let target = kept.join(instance);
        let copied = copy_dir(folder, &target).map_err(|e| {
            format!(
                "cannot copy {} to {}: {e}",
                folder.display(),
                target.display()
            )
        })?;
        if copied != size {
            return Err(format!(
                "copying {} gave {copied} bytes, not {size}",
                folder.display()
            ));
        }
        println!("  kept {} ({})", target.display(), mb(copied));
    }
    Ok(kept)
}

/// Stops what Kiln runs: the daemon first, so it doesn't restart what is stopped next.
fn stop_everything() -> Result<(), String> {
    if crate::daemon::stop_quietly()? {
        println!("Stopped Kiln's daemon.");
    }
    if let Some(pid) = crate::web::server()?.stop().map_err(|e| e.to_string())? {
        println!("Stopped the web server (PID {pid}).");
    }
    if let Some(pid) = crate::pool::supervisor()?
        .stop()
        .map_err(|e| e.to_string())?
    {
        println!("Stopped the PHP workers (supervisor PID {pid}).");
    }
    if let Some(pid) = crate::service::supervisor()?
        .stop()
        .map_err(|e| e.to_string())?
    {
        println!("Stopped the services (supervisor PID {pid}).");
    }
    Ok(())
}

/// Databases are the user's work: never deleted without being asked ([ADR 0036] §9).
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
fn refuse_data_loss(plan: &Plan, options: Options) -> Result<(), String> {
    if plan.service_data.is_empty()
        || options.keep_files
        || options.delete_data
        || options.keep_data
    {
        return Ok(());
    }
    Err(
        "Kiln's services hold data (listed above). Nothing was changed.\n\
         To keep it, run `kiln uninstall --keep-data`: it is copied to `Kiln data (<date>)` in \
         your home folder first. To delete it, run `kiln uninstall --delete-data`. \
         `kiln db dump` writes a database to a portable .sql file before either."
            .into(),
    )
}

pub fn uninstall(options: Options) -> Result<(), String> {
    let keep_files = options.keep_files;
    let plan = build_plan()?;
    print_plan(&plan, keep_files);
    if options.dry_run {
        println!("Dry run: nothing was changed.");
        return Ok(());
    }
    refuse_data_loss(&plan, options)?;
    if !options.yes && !crate::apply::confirm("Uninstall Kiln as shown?") {
        println!("Nothing was changed.");
        return Ok(());
    }

    // 1. Stop.
    stop_everything()?;
    // Their data out of Kiln's folder while nothing writes to it, before anything is undone:
    // a copy that fails stops uninstall with nothing changed.
    if options.keep_data && !keep_files && !plan.service_data.is_empty() {
        let kept = keep_data(&plan.service_data).map_err(|e| {
            format!("{e}\nUninstall stopped here; the services are stopped, and nothing was undone or deleted.")
        })?;
        println!("Copied the services' data to {}.", kept.display());
    }

    // 2. Undo, newest first; stop at the first failure and delete nothing.
    if !plan.undo.is_empty() {
        // Everything in the plan, newest first; projects' .env wiring isn't in it.
        let ids: Vec<u64> = plan.undo.iter().map(|(id, _)| *id).collect();
        crate::apply::revert_quietly(&ids).map_err(|e| {
            format!("{e}\nUninstall stopped here and deleted nothing; fix the cause and run `kiln uninstall` again.")
        })?;
        println!("Undid {} operation(s).", ids.len());
    }
    if keep_files {
        println!("Done. Kiln's folders were kept (--keep-files).");
        return Ok(());
    }
    // The root key may live outside Kiln's folders (the Secret Service, ADR 0021 §3). If it
    // can't be removed now, the state folder, which holds the only reference to it, is kept,
    // so a later `kiln uninstall` can finish the job instead of orphaning it (I-59).
    let forgotten =
        crate::apply::ca_store().and_then(|ca| ca.forget_root_key().map_err(|e| e.to_string()));
    let key_left = if let Err(e) = &forgotten {
        println!("Couldn't remove the CA root key from your keyring: {e}");
        true
    } else {
        if plan.key_in_keyring {
            println!("Removed the CA root key from your keyring.");
        }
        false
    };

    // 3. Delete; the state folder only if nothing is left in effect or stuck.
    let unresolved = crate::web::open_store()
        .and_then(|store| store.entries().map_err(|e| e.to_string()))
        .map_or(0, |entries| {
            entries
                .iter()
                .filter(|e| {
                    e.kind != ENV_KEYS_KIND
                        && matches!(
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
        if key_left && Some(&folder.path) == state_dir.as_ref() {
            println!(
                "Kept {}: it refers to the CA root key still in your keyring. Run `kiln uninstall` again from a terminal on the desktop to finish.",
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

    #[test]
    fn kept_data_goes_to_a_dated_folder_never_over_an_old_one() {
        let home = tempfile::tempdir().unwrap();
        let first = kept_data_folder(home.path(), "2026-11-11");
        assert_eq!(first, home.path().join("Kiln data (2026-11-11)"));
        std::fs::create_dir(&first).unwrap();
        assert_eq!(
            kept_data_folder(home.path(), "2026-11-11"),
            home.path().join("Kiln data (2026-11-11) 2")
        );
    }

    #[test]
    fn a_data_folder_is_copied_whole() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("mysql-8.4").join("data");
        std::fs::create_dir_all(from.join("shop")).unwrap();
        std::fs::write(from.join("ibdata1"), vec![1u8; 4096]).unwrap();
        std::fs::write(from.join("shop").join("users.ibd"), b"rows").unwrap();
        let to = dir.path().join("kept").join("mysql-8.4");
        assert_eq!(copy_dir(&from, &to).unwrap(), dir_size(&from));
        assert_eq!(
            std::fs::read(to.join("shop").join("users.ibd")).unwrap(),
            b"rows"
        );
    }

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
