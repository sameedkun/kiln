//! The shims ([ADR 0016] §8–10, [ADR 0028] §6): one binary that acts as `php`, `composer`,
//! `node`, `npm`, `npx` or `corepack`, depending on its own file name, and runs the PHP or Node
//! for the current folder.
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use kiln_node::InstalledNode;

use crate::DirPhp;

/// Folder (under Kiln's local data) holding the shims (`php.exe` / `php`, `composer.exe` /
/// `composer`, and the Node ones).
pub const SHIMS_DIR: &str = if cfg!(windows) {
    r"bin\shims"
} else {
    "bin/shims"
};
/// Where `kiln composer install` puts Composer (under Kiln's local data).
pub const COMPOSER_PHAR: &str = if cfg!(windows) {
    r"bin\composer.phar"
} else {
    "bin/composer.phar"
};

/// What a shim pretends to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Php,
    Composer,
    Node,
    Npm,
    Npx,
    Corepack,
}

impl Tool {
    /// Every shim `kiln shims install` writes.
    pub const ALL: [Self; 6] = [
        Self::Php,
        Self::Composer,
        Self::Node,
        Self::Npm,
        Self::Npx,
        Self::Corepack,
    ];

    /// From the shim's own file name: `php.exe` → `Php`, `npm` → `Npm`.
    #[must_use]
    pub fn from_exe(path: &Path) -> Option<Self> {
        let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
        Self::ALL.into_iter().find(|tool| tool.name() == stem)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::Composer => "composer",
            Self::Node => "node",
            Self::Npm => "npm",
            Self::Npx => "npx",
            Self::Corepack => "corepack",
        }
    }

    /// Whether it runs Node (else PHP).
    #[must_use]
    pub fn is_node(self) -> bool {
        !matches!(self, Self::Php | Self::Composer)
    }

    /// The script a Node tool runs, inside the version's bundled packages.
    fn node_script(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Npm => Some(&["npm", "bin", "npm-cli.js"]),
            Self::Npx => Some(&["npm", "bin", "npx-cli.js"]),
            Self::Corepack => Some(&["corepack", "dist", "corepack.js"]),
            Self::Php | Self::Composer | Self::Node => None,
        }
    }

    /// The file names `kiln shims install` creates.
    #[must_use]
    pub fn file_name(self) -> String {
        format!("{}{}", self.name(), std::env::consts::EXE_SUFFIX)
    }
}

/// The command to run for `tool` with `args`, given the folder's PHP; or why there is none.
pub fn command(
    tool: Tool,
    here: &DirPhp,
    composer_phar: &Path,
    args: Vec<OsString>,
) -> Result<Command, String> {
    let Some(php) = &here.php.php else {
        return Err(
            "no PHP installed by Kiln yet; run `kiln php install 8.4` (see `kiln php available`)"
                .into(),
        );
    };
    let exe = php.dir.join(kiln_php::PHP_EXE);
    let mut command = Command::new(&exe);
    match tool {
        Tool::Php => {
            command.args(args);
        }
        Tool::Composer => {
            if !composer_phar.is_file() {
                return Err(
                    "Composer isn't installed by Kiln yet; run `kiln composer install`".into(),
                );
            }
            command.arg(composer_phar).args(args);
        }
        _ => return Err(format!("{} isn't a PHP tool", tool.name())),
    }
    Ok(command)
}

/// The command to run a Node `tool` with `args` on `node` ([ADR 0028] §6): `node` itself, or
/// `node <bundled script>` for npm, npx and corepack (no `.cmd` file, so no `cmd.exe`). The
/// version's folder goes first on the child's `PATH`, so scripts that start `node` get the
/// same one.
///
/// [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md
pub fn node_command(
    tool: Tool,
    node: &InstalledNode,
    path: Option<&OsStr>,
    args: Vec<OsString>,
) -> Result<Command, String> {
    let mut command = Command::new(node.node_exe());
    if let Some(parts) = tool.node_script() {
        let script = parts
            .iter()
            .fold(node.modules_dir(), |path, part| path.join(part));
        if !script.is_file() {
            return Err(if tool == Tool::Corepack {
                format!(
                    "Node {} doesn't include corepack (Node 25 dropped it); run `npx corepack` instead",
                    node.version
                )
            } else {
                format!(
                    "{} is missing; reinstall with `kiln node install {}`",
                    script.display(),
                    node.version
                )
            });
        }
        command.arg(script);
    } else if !tool.is_node() {
        return Err(format!("{} isn't a Node tool", tool.name()));
    }
    command.args(args);
    let rest = path.map(std::env::split_paths).into_iter().flatten();
    let joined = std::env::join_paths(std::iter::once(node.bin_dir()).chain(rest))
        .map_err(|e| format!("cannot build PATH: {e}"))?;
    command.env("PATH", joined);
    Ok(command)
}

/// The first `name` on `path` that isn't in `skip_dir` (Kiln's shims) and isn't `me`, as a
/// shell would find it: with `PATHEXT` on Windows, executable on Unix.
#[must_use]
pub fn find_on_path(
    path: &OsStr,
    name: &str,
    skip_dir: &Path,
    me: Option<&Path>,
) -> Option<PathBuf> {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let names: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| format!("{name}{}", ext.to_ascii_lowercase()))
            .collect()
    } else {
        vec![name.to_owned()]
    };
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty() && !same(dir, skip_dir))
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)).collect::<Vec<_>>())
        .find(|candidate| is_executable(candidate) && !me.is_some_and(|me| same(candidate, me)))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Kiln's local data folder, as the CLI uses it.
fn local_dir() -> Result<PathBuf, String> {
    kiln_state::default_local_dir().ok_or_else(|| "can't find Kiln's data folder".to_owned())
}

/// Runs the shim: resolves the PHP for the current folder and runs `tool` with `args`.
/// Returns the exit code to end with.
#[must_use]
pub fn run(tool: Tool, args: Vec<OsString>) -> i32 {
    match run_inner(tool, args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("kiln {}: {message}", tool.name());
            1
        }
    }
}

fn run_inner(tool: Tool, args: Vec<OsString>) -> Result<i32, String> {
    if tool.is_node() {
        return run_node(tool, args);
    }
    let state_dir = kiln_state::default_state_dir().ok_or("can't find Kiln's state folder")?;
    let db = state_dir.join(kiln_state::DB_FILE);
    if !db.is_file() {
        return Err("Kiln has no state yet; run `kiln php install 8.4` first".into());
    }
    // Read-only use: never migrate or import from a shim.
    let store = kiln_state::StateStore::open(&db).map_err(|e| e.to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let here = crate::php_for_dir(&store, &cwd).map_err(|e| e.to_string())?;
    drop(store);
    if here.php.fell_back {
        eprintln!(
            "kiln {}: this site {} PHP {}, which isn't installed; using {} instead",
            tool.name(),
            if here.php.from_project {
                "asks (in kiln.toml) for"
            } else {
                "is pinned to"
            },
            here.php.pinned.as_deref().unwrap_or("?"),
            here.php.php.as_ref().map_or("none", |p| p.version.as_str())
        );
    }
    let command = command(tool, &here, &local_dir()?.join(COMPOSER_PHAR), args)?;
    finish(command)
}

/// A Node tool ([ADR 0028] §5–6): the folder's Node, or whatever comes next on `PATH` while
/// Kiln has no Node at all.
///
/// [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md
fn run_node(tool: Tool, args: Vec<OsString>) -> Result<i32, String> {
    let local = local_dir()?;
    let installed = kiln_node::installed(&local.join(crate::node::NODE_DIR));
    let path = std::env::var_os("PATH");
    if installed.is_empty() {
        let me = std::env::current_exe().ok();
        let other = path.as_deref().and_then(|path| {
            find_on_path(path, tool.name(), &local.join(SHIMS_DIR), me.as_deref())
        });
        let Some(other) = other else {
            return Err(format!(
                "no Node installed by Kiln yet (`kiln node install lts`), and no other `{}` on PATH",
                tool.name()
            ));
        };
        let mut command = Command::new(other);
        command.args(args);
        return finish(command);
    }
    let default = read_setting(crate::node::DEFAULT_KEY);
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let here = crate::node::node_for_dir(&installed, default.as_deref(), &cwd);
    let node = here.node.ok_or("no Node installed by Kiln")?;
    if here.fell_back
        && let Some(wanted) = &here.wanted
    {
        let why = match &wanted.spec {
            Ok(spec) => {
                format!("asks for Node {spec}, which isn't installed (`kiln node install {spec}`)")
            }
            Err(e) => e.clone(),
        };
        eprintln!(
            "kiln {}: {} {why}; using {} instead",
            tool.name(),
            wanted.file.display(),
            node.version
        );
    }
    finish(node_command(tool, &node, path.as_deref(), args)?)
}

/// A setting from Kiln's state, if there is any state.
fn read_setting(key: &str) -> Option<String> {
    let db = kiln_state::default_state_dir()?.join(kiln_state::DB_FILE);
    if !db.is_file() {
        return None;
    }
    let store = kiln_state::StateStore::open(&db).ok()?;
    store.setting(key).ok().flatten()
}

/// Runs `command` in the shim's place and returns the exit code to end with.
fn finish(mut command: Command) -> Result<i32, String> {
    #[cfg(windows)]
    {
        // Ctrl+C goes to PHP (same console); the shim waits and passes its exit code on.
        let _ = kiln_sys::win::survive_ctrl_c();
        let status = command.status().map_err(|e| {
            format!(
                "cannot run {}: {e}",
                Path::new(command.get_program()).display()
            )
        })?;
        Ok(status.code().unwrap_or(1))
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        Err(format!(
            "cannot run {}: {error}",
            Path::new(command.get_program()).display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SitePhp;
    use kiln_state::PhpRecord;

    fn here(version: Option<&str>) -> DirPhp {
        DirPhp {
            site: None,
            php: SitePhp {
                php: version.map(|v| PhpRecord {
                    version: v.into(),
                    dir: PathBuf::from(format!("C:/kiln/php/{v}")),
                    archive: String::new(),
                    sha256: String::new(),
                }),
                pinned: None,
                from_project: false,
                fell_back: false,
            },
        }
    }

    #[test]
    fn the_tool_comes_from_the_shim_file_name() {
        let dir = Path::new("shims");
        assert_eq!(Tool::from_exe(&dir.join("php.exe")), Some(Tool::Php));
        assert_eq!(Tool::from_exe(&dir.join("composer")), Some(Tool::Composer));
        assert_eq!(Tool::from_exe(&dir.join("PHP.EXE")), Some(Tool::Php));
        assert_eq!(Tool::from_exe(&dir.join("kiln-shim.exe")), None);
    }

    #[test]
    fn php_runs_the_resolved_build_with_the_same_arguments() {
        let command = command(
            Tool::Php,
            &here(Some("8.1.34")),
            Path::new("composer.phar"),
            vec!["-r".into(), "echo PHP_VERSION;".into()],
        )
        .unwrap();
        assert_eq!(
            Path::new(command.get_program()),
            Path::new("C:/kiln/php/8.1.34").join(kiln_php::PHP_EXE)
        );
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-r", "echo PHP_VERSION;"]);
    }

    #[test]
    fn composer_runs_the_phar_with_the_resolved_php() {
        let dir = tempfile::tempdir().unwrap();
        let phar = dir.path().join("composer.phar");
        assert!(command(Tool::Composer, &here(Some("8.4.27")), &phar, vec![]).is_err());
        std::fs::write(&phar, "").unwrap();
        let command = command(
            Tool::Composer,
            &here(Some("8.4.27")),
            &phar,
            vec!["install".into()],
        )
        .unwrap();
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, [phar.as_os_str(), "install".as_ref()]);
    }

    fn installed_node(dir: &Path, with_corepack: bool) -> InstalledNode {
        let node = InstalledNode {
            version: "22.23.4".into(),
            numbers: [22, 23, 4],
            lts: Some("jod".into()),
            dir: dir.to_owned(),
        };
        let modules = node.modules_dir();
        std::fs::create_dir_all(modules.join("npm").join("bin")).unwrap();
        std::fs::write(modules.join("npm").join("bin").join("npm-cli.js"), "").unwrap();
        if with_corepack {
            std::fs::create_dir_all(modules.join("corepack").join("dist")).unwrap();
            std::fs::write(
                modules.join("corepack").join("dist").join("corepack.js"),
                "",
            )
            .unwrap();
        }
        node
    }

    #[test]
    fn the_shim_names_cover_node_tools() {
        assert_eq!(Tool::from_exe(Path::new("npm.exe")), Some(Tool::Npm));
        assert_eq!(Tool::from_exe(Path::new("corepack")), Some(Tool::Corepack));
        assert!(Tool::Node.is_node() && !Tool::Composer.is_node());
        assert_eq!(Tool::ALL.len(), 6);
    }

    #[test]
    fn npm_runs_its_script_with_the_chosen_node_first_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let node = installed_node(dir.path(), true);
        let old_path = std::env::join_paths([Path::new("/usr/bin")]).unwrap();
        let command = node_command(
            Tool::Npm,
            &node,
            Some(&old_path),
            vec!["run".into(), "build".into()],
        )
        .unwrap();
        assert_eq!(Path::new(command.get_program()), node.node_exe());
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(
            args,
            [
                node.modules_dir()
                    .join("npm")
                    .join("bin")
                    .join("npm-cli.js")
                    .as_os_str(),
                "run".as_ref(),
                "build".as_ref()
            ]
        );
        let path = command
            .get_envs()
            .find(|(k, _)| *k == "PATH")
            .and_then(|(_, v)| v)
            .unwrap();
        let entries: Vec<PathBuf> = std::env::split_paths(path).collect();
        assert_eq!(entries, [node.bin_dir(), PathBuf::from("/usr/bin")]);

        let node_only = node_command(Tool::Node, &node, None, vec!["-v".into()]).unwrap();
        assert_eq!(node_only.get_args().collect::<Vec<_>>(), ["-v"]);
        assert!(node_command(Tool::Php, &node, None, vec![]).is_err());
    }

    #[test]
    fn corepack_without_its_package_says_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let node = installed_node(dir.path(), false);
        let err = node_command(Tool::Corepack, &node, None, vec![]).unwrap_err();
        assert!(err.contains("npx corepack"), "{err}");
    }

    #[test]
    fn the_path_search_skips_kilns_shims() {
        let dir = tempfile::tempdir().unwrap();
        let shims = dir.path().join("shims");
        let other = dir.path().join("other");
        for folder in [&shims, &other] {
            std::fs::create_dir_all(folder).unwrap();
            let file = folder.join(Tool::Node.file_name());
            std::fs::write(&file, "").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let path = std::env::join_paths([&shims, &other]).unwrap();
        let found = find_on_path(&path, "node", &shims, None).unwrap();
        assert_eq!(found, other.join(Tool::Node.file_name()));
        let only_shims = std::env::join_paths([&shims]).unwrap();
        assert_eq!(find_on_path(&only_shims, "node", &shims, None), None);
        // Nor the running shim itself, wherever it is.
        let me = other.join(Tool::Node.file_name());
        assert_eq!(find_on_path(&path, "node", &shims, Some(&me)), None);
    }

    #[test]
    fn without_php_the_shim_says_how_to_get_it() {
        let err = command(Tool::Php, &here(None), Path::new("x"), vec![]).unwrap_err();
        assert!(err.contains("kiln php install"), "{err}");
    }
}
