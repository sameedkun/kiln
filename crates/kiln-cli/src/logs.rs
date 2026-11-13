//! `kiln logs` ([ADR 0059]): every log Kiln knows of, and the last lines of one.
//!
//! [ADR 0059]: ../../../docs/adr/0059-log-viewer.md

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::json;

/// One log: an id the app and the API name it by, where it is, and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub group: &'static str,
    pub label: String,
    pub path: PathBuf,
}

/// The newest `*.log` in a Laravel project's `storage/logs` (daily logs roll to a new file each
/// day), or `None` when it has none.
#[must_use]
pub fn newest_log(project: &Path) -> Option<PathBuf> {
    let dir = project.join("storage").join("logs");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("log"))
                && e.file_type().is_ok_and(|t| t.is_file())
        })
        .max_by_key(|e| (e.metadata().and_then(|m| m.modified()).ok(), e.file_name()))
        .map(|e| e.path())
}

/// Every log Kiln knows of, Kiln's own first, then the services', then the sites'.
pub fn sources() -> Result<Vec<Source>, String> {
    let mut all = Vec::new();
    let kiln = |id: &str, label: &str, path: PathBuf| Source {
        id: id.to_owned(),
        group: "Kiln",
        label: label.to_owned(),
        path,
    };
    if let Some(paths) = kiln_daemon::Paths::for_user() {
        all.push(kiln("kiln", "Kiln's daemon", paths.log));
    }
    all.push(kiln("web", "Web server", crate::web::server()?.log_file()));
    let supervisor = crate::pool::supervisor()?;
    all.push(kiln("php", "PHP errors", supervisor.errors_path()));
    all.push(kiln("php-workers", "PHP workers", supervisor.log_path()));

    let store = crate::web::open_store()?;
    for record in store.services().map_err(|e| e.to_string())? {
        let Ok(provider) = crate::service::provider_of(&record.kind) else {
            continue;
        };
        let logs = provider.logs(&crate::service::dirs_of(&record)?);
        let many = logs.len() > 1;
        for path in logs {
            let file = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            all.push(Source {
                id: if many {
                    format!("service:{}/{file}", record.name)
                } else {
                    format!("service:{}", record.name)
                },
                group: "Services",
                label: if many {
                    format!("{} ({file})", record.name)
                } else {
                    record.name.clone()
                },
                path,
            });
        }
    }

    let sites = crate::web::resolved(&store)?;
    for site in sites.sites.iter().filter(|s| s.alias_of.is_none()) {
        if let Some(path) = newest_log(&site.project) {
            all.push(Source {
                id: format!("site:{}", site.host.as_str()),
                group: "Sites",
                label: site.host.as_str().to_owned(),
                path,
            });
        }
    }
    Ok(all)
}

/// The last `lines` lines of a file, reading at most its last 1 MiB.
fn tail(path: &Path, lines: usize) -> Result<Vec<String>, String> {
    use std::io::{Read, Seek, SeekFrom};
    const MOST: u64 = 1 << 20;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let start = len.saturating_sub(MOST);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut all: Vec<&str> = text.lines().collect();
    // Started mid-file: the first line is likely cut.
    if start > 0 && !all.is_empty() {
        all.remove(0);
    }
    Ok(all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect())
}

/// `kiln logs`: the sources; `kiln logs <source>`: its last lines.
pub fn logs(source: Option<&str>, lines: usize) -> Result<(), String> {
    let all = sources()?;
    let Some(id) = source else {
        if crate::out::json() {
            crate::out::data(json!({
                "sources": all.iter().map(|s| {
                    let meta = std::fs::metadata(&s.path).ok();
                    json!({
                        "id": s.id,
                        "group": s.group,
                        "label": s.label,
                        "path": s.path,
                        "exists": meta.is_some(),
                        "bytes": meta.as_ref().map(std::fs::Metadata::len),
                        "modified_ms": meta
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                    })
                }).collect::<Vec<_>>(),
            }));
            return Ok(());
        }
        let mut group = "";
        for s in &all {
            if s.group != group {
                group = s.group;
                say!("{group}:");
            }
            say!("  {:<28} {}", s.id, s.path.display());
        }
        say!("Show one with `kiln logs <id>`.");
        return Ok(());
    };
    let s = all
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("no log called {id}; `kiln logs` lists them"))?;
    let shown = tail(&s.path, lines)?;
    if crate::out::json() {
        crate::out::data(json!({ "id": s.id, "path": s.path, "lines": shown }));
        return Ok(());
    }
    if shown.is_empty() {
        say!("{} is empty ({}).", s.label, s.path.display());
    }
    for line in shown {
        say!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_laravel_log_is_chosen() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("storage").join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        assert_eq!(newest_log(dir.path()), None);
        std::fs::write(logs.join(".gitignore"), "*").unwrap();
        std::fs::write(logs.join("laravel-2026-11-12.log"), "old").unwrap();
        let older = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(logs.join("laravel-2026-11-12.log"))
            .unwrap()
            .set_modified(older)
            .unwrap();
        std::fs::write(logs.join("laravel-2026-11-13.log"), "new").unwrap();
        assert_eq!(
            newest_log(dir.path()),
            Some(logs.join("laravel-2026-11-13.log"))
        );
    }

    #[test]
    fn a_project_without_logs_has_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(newest_log(dir.path()), None);
    }

    #[test]
    fn tail_gives_the_last_lines_and_nothing_for_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.log");
        assert_eq!(tail(&file, 3).unwrap().len(), 0);
        std::fs::write(&file, "1\n2\n3\n4\n5\n").unwrap();
        assert_eq!(tail(&file, 2).unwrap(), ["4", "5"]);
        assert_eq!(tail(&file, 10).unwrap().len(), 5);
    }
}
