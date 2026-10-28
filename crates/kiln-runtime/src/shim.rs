//! The `php` / `composer` shims ([ADR 0016] §8–10): one binary that acts as either, depending
//! on its own file name, and runs the PHP for the current folder.
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::DirPhp;

/// Folder (under Kiln's local data) holding the shims (`php.exe` / `php`, `composer.exe` /
/// `composer`).
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
}

impl Tool {
    /// From the shim's own file name: `php.exe` → `Php`, `composer.exe` → `Composer`.
    #[must_use]
    pub fn from_exe(path: &Path) -> Option<Self> {
        let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
        match stem.as_str() {
            "php" => Some(Self::Php),
            "composer" => Some(Self::Composer),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::Composer => "composer",
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
    }
    Ok(command)
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
    let mut command = command(tool, &here, &local_dir()?.join(COMPOSER_PHAR), args)?;

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

    #[test]
    fn without_php_the_shim_says_how_to_get_it() {
        let err = command(Tool::Php, &here(None), Path::new("x"), vec![]).unwrap_err();
        assert!(err.contains("kiln php install"), "{err}");
    }
}
