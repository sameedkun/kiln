//! `kiln config show|save|backups|restore` ([ADR 0089]): a PHP's `php.ini`, a site's `.env`
//! and a service's own settings file, edited whole, checked before they land (or, for a
//! service, by starting it), with the last [`KEEP`] versions kept as backups.
//!
//! [ADR 0089]: ../../../docs/adr/0089-editing-config-files.md

use std::path::{Path, PathBuf};

use kiln_ops::{Change, Preview};

/// Backups kept per target.
const KEEP: usize = 10;

/// What is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Php(String),
    Env(String),
    Service(String),
}

impl Target {
    fn parse(text: &str) -> Result<Self, String> {
        let (kind, name) = text
            .split_once(':')
            .filter(|(_, n)| !n.is_empty() && !n.starts_with('-'))
            .ok_or_else(|| {
                format!("{text:?} isn't a target: php:<version>, env:<site> or service:<instance>")
            })?;
        let name = name.to_owned();
        match kind {
            "php" => Ok(Self::Php(name)),
            "env" => Ok(Self::Env(name)),
            "service" => Ok(Self::Service(name)),
            other => Err(format!("{other:?} isn't php, env or service")),
        }
    }

    /// A folder name for its backups.
    fn slug(&self) -> String {
        let (kind, name) = match self {
            Self::Php(n) => ("php", n),
            Self::Env(n) => ("env", n),
            Self::Service(n) => ("service", n),
        };
        let name: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{kind}-{name}")
    }

    fn label(&self) -> String {
        match self {
            Self::Php(v) => format!("php.ini of PHP {v}"),
            Self::Env(s) => format!(".env of {s}"),
            Self::Service(i) => format!("{i}'s settings"),
        }
    }
}

/// The file a target is, and for PHP its build.
fn file_of(target: &Target) -> Result<PathBuf, String> {
    let store = crate::web::open_store()?;
    match target {
        Target::Php(spec) => Ok(crate::ext::target(&store, Some(spec))?.dir.join("php.ini")),
        Target::Env(site) => {
            let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
            let host = crate::web::host_name_under(site, &tld)?;
            let found = crate::web::resolved(&store)?
                .sites
                .into_iter()
                .find(|s| s.host == host)
                .ok_or_else(|| format!("Kiln doesn't serve {host}"))?;
            Ok(found.project.join(".env"))
        }
        Target::Service(name) => {
            let record = crate::service::record_of(&store, name)?;
            if record.version == crate::external::VERSION {
                return Err(format!(
                    "{name} isn't run by Kiln, so Kiln doesn't change its settings (ADR 0091): edit them where that server keeps them"
                ));
            }
            let provider = crate::service::provider_of(&record.kind)?;
            provider
                .user_config(&crate::service::dirs_of(&record)?)
                .ok_or_else(|| {
                    format!(
                        "{} has no settings file Kiln lets you edit (MySQL, MariaDB and PostgreSQL do)",
                        provider.display_name()
                    )
                })
        }
    }
}

fn backups_dir(target: &Target) -> Result<PathBuf, String> {
    let local =
        kiln_state::default_local_dir().ok_or("cannot determine Kiln's local data directory")?;
    Ok(local.join("backups").join(target.slug()))
}

/// The staged text's file for `token` (written by kilnd's `config.stage`).
pub(crate) fn staged_path(token: &str) -> Result<PathBuf, String> {
    if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{token:?} isn't a staged text's token"));
    }
    let local =
        kiln_state::default_local_dir().ok_or("cannot determine Kiln's local data directory")?;
    Ok(local.join("staged").join(format!("{token}.txt")))
}

/// The lines that change, as a preview's changes (at most 20).
fn changed_lines(before: &str, after: &str) -> Vec<Change> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let mut out = Vec::new();
    for i in 0..old.len().max(new.len()) {
        let (a, b) = (old.get(i).copied(), new.get(i).copied());
        if a != b {
            out.push(Change {
                target: format!("line {}", i + 1),
                before: a.map(str::to_owned),
                after: b.map(str::to_owned),
            });
            if out.len() == 20 {
                break;
            }
        }
    }
    out
}

/// A `.env` text's problems, by line: each line is blank, a comment, or `KEY=value` with a
/// valid name and its quotes closed (a double-quoted value may run over lines).
fn env_problems(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut open: Option<usize> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        if let Some(start) = open {
            if raw.contains('"') {
                open = None;
            } else if n - start > 50 {
                problems.push(format!("line {start}: its quote is never closed"));
                open = None;
            }
            continue;
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            problems.push(format!("line {n}: not KEY=value"));
            continue;
        };
        let key = key.trim();
        let valid = !key.is_empty()
            && !key.starts_with(|c: char| c.is_ascii_digit())
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
        if !valid {
            problems.push(format!("line {n}: {key:?} isn't a key's name"));
            continue;
        }
        let value = value.trim();
        if let Some(rest) = value.strip_prefix('"') {
            let closed = rest.chars().scan(false, |escaped, c| {
                let close = c == '"' && !*escaped;
                *escaped = c == '\\' && !*escaped;
                Some(close)
            });
            if !closed.into_iter().any(|c| c) {
                open = Some(n);
            }
        } else if let Some(rest) = value.strip_prefix('\'')
            && !rest.contains('\'')
        {
            problems.push(format!("line {n}: its single quote is never closed"));
        }
    }
    if let Some(start) = open {
        problems.push(format!("line {start}: its quote is never closed"));
    }
    problems
}

/// PHP's own words about `text` as a php.ini, or nothing if it starts cleanly.
fn php_ini_problems(spec: &str, text: &str) -> Result<Option<String>, String> {
    let store = crate::web::open_store()?;
    let record = crate::ext::target(&store, Some(spec))?;
    drop(store);
    let dir = tempfile_dir()?;
    let ini = dir.join("php.ini");
    std::fs::write(&ini, text).map_err(|e| format!("cannot write {}: {e}", ini.display()))?;
    let out = std::process::Command::new(record.dir.join(kiln_php::PHP_EXE))
        .arg("-c")
        .arg(&ini)
        .args(["-d", "display_startup_errors=1", "-r", "echo 'kiln-ok';"])
        .current_dir(&record.dir)
        .output()
        .map_err(|e| format!("cannot run PHP {}: {e}", record.version));
    let _ = std::fs::remove_dir_all(&dir);
    let out = out?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let warned: Vec<&str> = text
        .lines()
        .filter(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("warning") || l.contains("error") || l.contains("unable to load")
        })
        .collect();
    Ok(
        if out.status.success() && text.contains("kiln-ok") && warned.is_empty() {
            None
        } else {
            Some(if warned.is_empty() {
                text.trim().to_owned()
            } else {
                warned.join("\n")
            })
        },
    )
}

fn tempfile_dir() -> Result<PathBuf, String> {
    let local =
        kiln_state::default_local_dir().ok_or("cannot determine Kiln's local data directory")?;
    let dir = local
        .join("staged")
        .join(format!("check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// A backup's file name: `<time>.bak`.
fn is_backup(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bak"))
}

/// The backups of a target, newest first.
fn backups(target: &Target) -> Result<Vec<String>, String> {
    let dir = backups_dir(target)?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_backup(n))
        .collect();
    names.sort_by(|a, b| b.cmp(a));
    Ok(names)
}

/// Keeps `text` (the file as it was) as a backup; drops the oldest beyond [`KEEP`].
fn back_up(target: &Target, text: &str) -> Result<String, String> {
    let dir = backups_dir(target)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let now = time::OffsetDateTime::now_utc();
    let mut name = format!(
        "{}{:02}{:02}-{:02}{:02}{:02}.bak",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let mut n = 1;
    while dir.join(&name).exists() {
        n += 1;
        name = format!("{}-{n}.bak", name.trim_end_matches(".bak"));
    }
    std::fs::write(dir.join(&name), text).map_err(|e| format!("cannot keep a backup: {e}"))?;
    for old in backups(target)?.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(dir.join(old));
    }
    Ok(name)
}

/// Writes `text` to `path` whole: a temporary file beside it, swapped in.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let temp = path.with_extension("kiln-tmp");
    std::fs::write(&temp, text)
        .and_then(|()| kiln_sys::rename_replace(&temp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            format!("cannot write {}: {e}", path.display())
        })
}

/// Whether the instance's main port answers within `wait`.
fn answers(name: &str, wait: std::time::Duration) -> bool {
    let Ok(store) = crate::web::open_store() else {
        return false;
    };
    let Ok(record) = crate::service::record_of(&store, name) else {
        return false;
    };
    let port = crate::service::provider_of(&record.kind)
        .ok()
        .and_then(|p| p.ports().first())
        .and_then(|role| record.ports.get(role.role).copied());
    let Some(port) = port else { return true };
    let deadline = std::time::Instant::now() + wait;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(
            &(std::net::Ipv4Addr::LOCALHOST, port).into(),
            std::time::Duration::from_millis(500),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    false
}

/// `kiln config show <target>`.
pub fn show(target: &str) -> Result<(), String> {
    let target = Target::parse(target)?;
    let path = file_of(&target)?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let kept = backups(&target)?;
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "label": target.label(),
            "path": path,
            "exists": path.is_file(),
            "text": text,
            "backups": kept,
        }));
        return Ok(());
    }
    say!("{}: {}", target.label(), path.display());
    print!("{text}");
    Ok(())
}

/// Checks, backs up, writes and applies `text` as the target's file. `false` if the user
/// declined (or a preview stopped at the question).
fn save_text(target: &Target, text: &str, yes: bool) -> Result<bool, String> {
    let path = file_of(target)?;
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    if before == text {
        say!("{} is unchanged.", target.label());
        return Ok(true);
    }
    // Checked before anything is asked: a broken file is refused at the preview.
    match target {
        Target::Env(_) => {
            let problems = env_problems(text);
            if !problems.is_empty() {
                return Err(format!(
                    "not saved; the .env has problems:\n{}",
                    problems.join("\n")
                ));
            }
        }
        Target::Php(spec) => {
            if let Some(words) = php_ini_problems(spec, text)? {
                return Err(format!(
                    "not saved; PHP doesn't start cleanly with it:\n{words}"
                ));
            }
        }
        Target::Service(_) => {}
    }
    let changes = changed_lines(&before, text);
    crate::apply::print_preview(
        &Preview {
            summary: format!("Write {} ({})", target.label(), path.display()),
            changes,
        },
        "(none)",
    );
    if !yes && !crate::apply::confirm(&format!("Save {}?", target.label())) {
        say!("Nothing was changed.");
        return Ok(false);
    }
    let backup = back_up(target, &before)?;
    write(&path, text)?;
    say!(
        "Saved {} (the previous one is kept as {backup}).",
        target.label()
    );
    match target {
        Target::Php(spec) => {
            let store = crate::web::open_store()?;
            let record = crate::ext::target(&store, Some(spec))?;
            crate::web::restart_php(&store, &record.version)?;
        }
        Target::Service(name) => {
            let running = crate::service::record_of(&crate::web::open_store()?, name)?.enabled;
            if running {
                // A start that fails may say so itself; either way the port decides.
                let restarted = crate::service::restart(name);
                if restarted.is_err() || !answers(name, std::time::Duration::from_secs(30)) {
                    write(&path, &before)?;
                    let _ = crate::service::restart(name);
                    // Back on the last good file before saying so.
                    let _ = answers(name, std::time::Duration::from_secs(30));
                    return Err(format!(
                        "{name} didn't start with the new settings, so the previous ones are back; `kiln service logs {name}` says why"
                    ));
                }
            }
        }
        Target::Env(_) => {
            say!("Cached config? `php artisan config:clear` so Laravel reads it again.");
        }
    }
    Ok(true)
}

/// `kiln config save <target> (--from <file> | --staged <token>) [--yes]`.
pub fn save(
    target: &str,
    from: Option<&Path>,
    staged: Option<&str>,
    yes: bool,
) -> Result<(), String> {
    let target = Target::parse(target)?;
    let file = match (from, staged) {
        (Some(file), None) => file.to_path_buf(),
        (None, Some(token)) => staged_path(token)?,
        _ => {
            return Err(
                "give the new text: --from <file>, or --staged <token> from the app".into(),
            );
        }
    };
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    // A staged text is used once: kept through the preview, gone once saved.
    if save_text(&target, &text, yes)? && staged.is_some() {
        let _ = std::fs::remove_file(&file);
    }
    Ok(())
}

/// `kiln config backups <target>`.
pub fn list_backups(target: &str) -> Result<(), String> {
    let target = Target::parse(target)?;
    let kept = backups(&target)?;
    if crate::out::json() {
        crate::out::data(serde_json::json!({ "backups": kept, "folder": backups_dir(&target)? }));
        return Ok(());
    }
    if kept.is_empty() {
        say!(
            "No backups of {} yet: one is kept before each save.",
            target.label()
        );
    }
    for name in &kept {
        say!("{name}");
    }
    Ok(())
}

/// `kiln config restore <target> <backup> [--yes]`: a save from a backup.
pub fn restore(target: &str, backup: &str, yes: bool) -> Result<(), String> {
    let target = Target::parse(target)?;
    if backup.contains(['/', '\\']) || !is_backup(backup) {
        return Err(format!(
            "{backup:?} isn't one of the backups (`kiln config backups`)"
        ));
    }
    let path = backups_dir(&target)?.join(backup);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("no backup {backup} of {}: {e}", target.label()))?;
    save_text(&target, &text, yes).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_read_and_named() {
        assert_eq!(Target::parse("php:8.4"), Ok(Target::Php("8.4".into())));
        assert_eq!(Target::parse("env:shop"), Ok(Target::Env("shop".into())));
        assert_eq!(
            Target::parse("service:mysql-8.4"),
            Ok(Target::Service("mysql-8.4".into()))
        );
        for bad in ["php", "php:", "env:-x", "file:/etc/passwd"] {
            assert!(Target::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            Target::Service("my sql/x".into()).slug(),
            "service-my_sql_x"
        );
    }

    #[test]
    fn env_problems_name_their_lines() {
        assert_eq!(
            env_problems("A=1\n# c\n\nexport B=\"two\nlines\"\nC='x'\n"),
            Vec::<String>::new()
        );
        assert_eq!(
            env_problems("A=1\nnot a setting\n1X=2\nB=\"open\nC='open\n"),
            [
                "line 2: not KEY=value",
                "line 3: \"1X\" isn't a key's name",
                "line 4: its quote is never closed",
            ]
        );
        assert_eq!(
            env_problems("C='open\n"),
            ["line 1: its single quote is never closed"]
        );
    }

    #[test]
    fn the_preview_shows_the_lines_that_change() {
        let changes = changed_lines("a\nb\nc\n", "a\nB\nc\nd\n");
        let shown: Vec<(String, Option<String>, Option<String>)> = changes
            .into_iter()
            .map(|c| (c.target, c.before, c.after))
            .collect();
        assert_eq!(
            shown,
            [
                ("line 2".into(), Some("b".into()), Some("B".into())),
                ("line 4".into(), None, Some("d".into())),
            ]
        );
        let many = changed_lines(&"x\n".repeat(30), &"y\n".repeat(30));
        assert_eq!(many.len(), 20);
    }
}
