//! `kiln git branches|switch|worktree` ([ADR 0075]): a site's Git branch switched, and Git
//! worktrees served as sites of their own, through the user's own Git. Kiln never forces:
//! a tree with uncommitted changes isn't switched, and Git refuses to remove a worktree that
//! has any.
//!
//! [ADR 0075]: ../../../docs/adr/0075-git-branches-and-worktrees.md

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

/// The user's Git: `git.exe` (or `git`) on `PATH`.
fn git_exe() -> Result<PathBuf, String> {
    let path = std::env::var("PATH").unwrap_or_default();
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let found = if cfg!(windows) {
        crate::apply::which_on(&path, "git")
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
    } else {
        crate::apply::which_on(&path, name)
    };
    found.ok_or_else(|| "Git isn't on PATH: install Git for Windows first".to_owned())
}

/// Runs Git in `dir` without a console window; its output, or its last lines as the error.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(git_exe()?);
    command.args(args).current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let errors = String::from_utf8_lossy(&output.stderr);
        let lines: Vec<&str> = errors.lines().filter(|l| !l.trim().is_empty()).collect();
        Err(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            lines[lines.len().saturating_sub(4)..].join(" ")
        ))
    }
}

/// A branch name Kiln passes to Git: not an option, and one Git itself accepts.
fn check_branch(dir: &Path, branch: &str) -> Result<(), String> {
    if branch.is_empty() || branch.starts_with('-') || branch.chars().any(char::is_whitespace) {
        return Err(format!("{branch:?} isn't a branch name"));
    }
    git(dir, &["check-ref-format", "--branch", branch])
        .map(|_| ())
        .map_err(|_| format!("{branch:?} isn't a branch name Git accepts"))
}

/// A branch as a site label: `feature/Checkout-2` is `feature-checkout-2`.
#[must_use]
pub fn slug(branch: &str) -> String {
    let mut out = String::new();
    for c in branch.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    out.chars()
        .take(40)
        .collect::<String>()
        .trim_end_matches('-')
        .to_owned()
}

/// A served site's host and folder, by its name.
fn site(name: &str) -> Result<(String, PathBuf), String> {
    let store = crate::web::open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let host = crate::web::host_name_under(name, &tld)?.as_str().to_owned();
    crate::web::resolved(&store)?
        .sites
        .iter()
        .find(|s| s.host.as_str() == host && s.alias_of.is_none())
        .map(|s| (host.clone(), s.project.clone()))
        .ok_or_else(|| format!("Kiln doesn't serve {host}"))
}

/// Whether the tree has changes Git would carry or lose: anything but ignored files.
fn dirty(dir: &Path) -> Result<bool, String> {
    Ok(!git(dir, &["status", "--porcelain"])?.trim().is_empty())
}

/// The worktrees of the repository at `dir`: (path, branch or None when detached).
fn worktrees(dir: &Path) -> Result<Vec<(PathBuf, Option<String>)>, String> {
    let text = git(dir, &["worktree", "list", "--porcelain"])?;
    let mut all = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    for line in text.lines().chain(std::iter::once("")) {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.trim_start_matches("refs/heads/").to_owned());
        } else if line.is_empty()
            && let Some(p) = path.take()
        {
            all.push((p, branch.take()));
        }
    }
    Ok(all)
}

/// `kiln git branches <site>`: its branch, the local ones, whether it has uncommitted changes,
/// and its worktrees with the sites that serve them.
pub fn branches(name: &str) -> Result<(), String> {
    let (host, project) = site(name)?;
    let current = git(&project, &["branch", "--show-current"])?
        .trim()
        .to_owned();
    let all: Vec<String> = git(
        &project,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    let dirty = dirty(&project)?;
    let store = crate::web::open_store()?;
    let served = crate::web::resolved(&store)?;
    let same = |a: &Path, b: &Path| {
        std::fs::canonicalize(a)
            .ok()
            .map(crate::web::strip_verbatim)
            == std::fs::canonicalize(b)
                .ok()
                .map(crate::web::strip_verbatim)
    };
    let trees: Vec<_> = worktrees(&project)?
        .into_iter()
        .filter(|(path, _)| !same(path, &project))
        .map(|(path, branch)| {
            let site = served
                .sites
                .iter()
                .find(|s| same(&s.project, &path))
                .map(|s| s.host.as_str().to_owned());
            json!({"path": path, "branch": branch, "host": site})
        })
        .collect();
    if crate::out::json() {
        crate::out::data(json!({
            "host": host,
            "current": current,
            "branches": all,
            "dirty": dirty,
            "worktrees": trees,
        }));
        return Ok(());
    }
    say!(
        "{host}: on {}{}",
        if current.is_empty() {
            "a detached HEAD"
        } else {
            &current
        },
        if dirty {
            ", with uncommitted changes"
        } else {
            ""
        }
    );
    for b in &all {
        say!("  {}{b}", if *b == current { "* " } else { "  " });
    }
    for t in &trees {
        say!(
            "  worktree {} ({})",
            t["path"].as_str().unwrap_or_default(),
            t["host"].as_str().unwrap_or("not served")
        );
    }
    Ok(())
}

/// `kiln git switch <site> <branch>`: only on a clean tree; the site serves that branch next.
pub fn switch(name: &str, branch: &str) -> Result<(), String> {
    let (host, project) = site(name)?;
    check_branch(&project, branch)?;
    if dirty(&project)? {
        return Err(format!(
            "{} has uncommitted changes: commit or stash them first; Kiln doesn't switch over them",
            project.display()
        ));
    }
    git(&project, &["switch", branch])?;
    say!("{host} is on {branch} now.");
    Ok(())
}

/// The folder a worktree of `project` for `branch` goes in: beside it, `<project>-<slug>`.
#[must_use]
pub fn worktree_folder(project: &Path, branch: &str) -> PathBuf {
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    project.with_file_name(format!("{name}-{}", slug(branch)))
}

/// `kiln git worktree add <site> <branch> [--new]`: a worktree beside the project, served as
/// `<branch>.<site>`. With `--new`, the branch is made from the current one.
pub fn worktree_add(name: &str, branch: &str, new: bool) -> Result<(), String> {
    let (host, project) = site(name)?;
    check_branch(&project, branch)?;
    let label = slug(branch);
    if label.is_empty() {
        return Err(format!("{branch:?} gives no name for a site"));
    }
    let folder = worktree_folder(&project, branch);
    if folder.exists() {
        return Err(format!("{} already exists", folder.display()));
    }
    let path = folder.to_string_lossy().into_owned();
    if new {
        git(&project, &["worktree", "add", "-b", branch, &path])?;
    } else {
        git(&project, &["worktree", "add", &path, branch])?;
    }
    say!("Made a worktree of {branch} in {}.", folder.display());
    let site_name = format!("{label}.{}", host.split('.').next().unwrap_or_default());
    crate::web::link(Some(site_name), Some(folder), false)
}

/// `kiln git worktree remove <site> <branch>`: no longer served, then removed by Git, which
/// refuses when it has uncommitted changes (Kiln never forces).
pub fn worktree_remove(name: &str, branch: &str) -> Result<(), String> {
    let (host, project) = site(name)?;
    check_branch(&project, branch)?;
    let tree = worktrees(&project)?
        .into_iter()
        .find(|(_, b)| b.as_deref() == Some(branch))
        .map(|(p, _)| p)
        .ok_or_else(|| format!("{host} has no worktree on {branch}"))?;
    if dirty(&tree)? {
        return Err(format!(
            "{} has uncommitted changes: commit or stash them first; Kiln doesn't remove them",
            tree.display()
        ));
    }
    let label = format!(
        "{}.{}",
        slug(branch),
        host.split('.').next().unwrap_or_default()
    );
    if let Err(e) = crate::web::unlink(&label) {
        say!("Note: {e}.");
    }
    git(&project, &["worktree", "remove", &tree.to_string_lossy()])?;
    say!(
        "Removed the worktree in {}; the branch {branch} stays.",
        tree.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_become_site_labels() {
        assert_eq!(slug("feature/Checkout-2"), "feature-checkout-2");
        assert_eq!(slug("--x__y--"), "x-y");
        assert_eq!(slug(&"a".repeat(60)).len(), 40);
    }

    #[test]
    fn a_worktree_goes_beside_its_project() {
        let project = Path::new("/code/shop");
        assert_eq!(
            worktree_folder(project, "feature/checkout"),
            Path::new("/code/shop-feature-checkout")
        );
    }

    /// With real Git, where there is one (CI's runners have it): a repository's worktrees are
    /// read back, and a change makes the tree "dirty".
    #[test]
    fn worktrees_and_changes_are_read_from_real_git() {
        if git_exe().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| git(&repo, args).unwrap();
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "ci@kiln.invalid"]);
        run(&["config", "user.name", "Kiln CI"]);
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "first"]);
        assert!(!dirty(&repo).unwrap());
        assert!(check_branch(&repo, "feature/x").is_ok());
        assert!(check_branch(&repo, "-x").is_err());
        assert!(check_branch(&repo, "a..b").is_err());
        let folder = worktree_folder(&repo, "feature/x");
        run(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature/x",
            &folder.to_string_lossy(),
        ]);
        let trees = worktrees(&repo).unwrap();
        assert_eq!(trees.len(), 2);
        assert_eq!(trees[1].1.as_deref(), Some("feature/x"));
        std::fs::write(repo.join("a.txt"), "changed").unwrap();
        assert!(dirty(&repo).unwrap());
    }
}
