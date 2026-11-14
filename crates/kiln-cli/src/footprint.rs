//! `kiln footprint` and `kiln cache clear` ([ADR 0063]): what Kiln uses on this PC, and the
//! one part of it that is safe to throw away.
//!
//! [ADR 0063]: ../../../docs/adr/0063-footprint.md

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

/// How long CPU use is measured over.
const SAMPLE: Duration = Duration::from_millis(500);

/// What a Kiln process is, from where its program is.
#[must_use]
pub fn label_of(image: &Path, local: &Path, kiln_dir: &Path) -> Option<String> {
    let lower = |p: &Path| p.to_string_lossy().to_lowercase().replace('/', "\\");
    let (image_l, local_l, kiln_l) = (lower(image), lower(local), lower(kiln_dir));
    let stem = image
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if image_l.starts_with(&format!("{kiln_l}\\")) {
        return Some(
            match stem.as_str() {
                "kilnd" => "Kiln's daemon",
                "kiln-desktop" => "Kiln's app",
                "kiln-supervisor" => "Kiln's supervisor",
                "kiln" => "kiln (a command)",
                _ => return Some(stem),
            }
            .to_owned(),
        );
    }
    let rest = image_l.strip_prefix(&format!("{local_l}\\"))?;
    let parts: Vec<&str> = rest.split('\\').collect();
    Some(match parts.as_slice() {
        ["bin", ..] if stem.contains("caddy") => "Web server".to_owned(),
        ["php", version, ..] if stem == "php-cgi" => format!("PHP {version} workers"),
        ["php", version, ..] => format!("PHP {version} (project processes)"),
        ["node", ..] => "Node (project processes)".to_owned(),
        ["services", "bin", kind, version, ..] => format!("{kind} {version}"),
        _ => stem,
    })
}

/// Kiln's processes, grouped by what they are: (label, count, memory in bytes, CPU %).
#[cfg(windows)]
fn processes(local: &Path, kiln_dir: &Path) -> Vec<(String, usize, u64, f64)> {
    use kiln_sys::win::{Process, processes};
    let mine: Vec<(String, Process)> = processes()
        .unwrap_or_default()
        .into_iter()
        // Not this command itself.
        .filter(|(pid, _)| *pid != std::process::id())
        .filter_map(|(pid, _)| {
            let process = Process::open_for_query(pid).ok().flatten()?;
            let label = label_of(&process.image_path().ok()?, local, kiln_dir)?;
            Some((label, process))
        })
        .collect();
    let before: Vec<_> = mine.iter().map(|(_, p)| p.usage().ok()).collect();
    std::thread::sleep(SAMPLE);
    let cpus = std::thread::available_parallelism()
        .map_or(1.0, |n| f64::from(u32::try_from(n.get()).unwrap_or(1)));
    let mut groups: Vec<(String, usize, u64, f64)> = Vec::new();
    for ((label, process), before) in mine.iter().zip(before) {
        let Some((memory, cpu)) = process.usage().ok() else {
            continue;
        };
        let used = before.map_or(Duration::ZERO, |(_, b)| cpu.saturating_sub(b));
        let percent = used.as_secs_f64() / SAMPLE.as_secs_f64() / cpus * 100.0;
        match groups.iter_mut().find(|g| g.0 == *label) {
            Some(g) => {
                g.1 += 1;
                g.2 += memory;
                g.3 += percent;
            }
            None => groups.push((label.clone(), 1, memory, percent)),
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.2));
    groups
}

#[cfg(not(windows))]
fn processes(_local: &Path, _kiln_dir: &Path) -> Vec<(String, usize, u64, f64)> {
    Vec::new()
}

/// The size of everything under `path`, not following links.
#[must_use]
pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .map(|e| size_of(&e.path()))
            .sum()
    })
}

/// Kiln's folders, by what they hold: (label, path, bytes).
fn folders(local: &Path, state: Option<&Path>) -> Vec<(&'static str, PathBuf, u64)> {
    let services = local.join("services");
    let services_programs = size_of(&services.join("bin"));
    let mut all = vec![
        ("PHP", local.join("php"), size_of(&local.join("php"))),
        ("Node", local.join("node"), size_of(&local.join("node"))),
        (
            "Kiln's programs (web server, Composer)",
            local.join("bin"),
            size_of(&local.join("bin")),
        ),
        (
            "Services' programs",
            services.join("bin"),
            services_programs,
        ),
        (
            "Services' data",
            services.clone(),
            size_of(&services).saturating_sub(services_programs),
        ),
        (
            "Download cache",
            local.join("downloads"),
            size_of(&local.join("downloads")),
        ),
        (
            "Web server's files (certificates)",
            local.join("caddy"),
            size_of(&local.join("caddy")),
        ),
        ("Logs", local.join("logs"), size_of(&local.join("logs"))),
        (
            "Xdebug",
            local.join("xdebug"),
            size_of(&local.join("xdebug")),
        ),
    ];
    if let Some(state) = state {
        all.push((
            "Kiln's settings and history",
            state.to_path_buf(),
            size_of(state),
        ));
    }
    all.retain(|(_, _, bytes)| *bytes > 0);
    all
}

fn megabytes(bytes: u64) -> String {
    let tenths = bytes.saturating_mul(10) / 1_048_576;
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

fn local_dir() -> Result<PathBuf, String> {
    kiln_state::default_local_dir()
        .ok_or_else(|| "cannot determine Kiln's local data directory".to_owned())
}

/// `kiln footprint`: memory and CPU per kind of Kiln process, and disk use per folder.
pub fn footprint() -> Result<(), String> {
    let local = local_dir()?;
    let kiln_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let state = kiln_state::default_state_dir();
    let running = processes(&local, &kiln_dir);
    let disk = folders(&local, state.as_deref());
    if crate::out::json() {
        crate::out::data(json!({
            "processes": running.iter().map(|(label, count, memory, cpu)| json!({
                "label": label,
                "count": count,
                "memory_bytes": memory,
                "cpu_percent": (cpu * 10.0).round() / 10.0,
            })).collect::<Vec<_>>(),
            "memory_bytes": running.iter().map(|g| g.2).sum::<u64>(),
            "folders": disk.iter().map(|(label, path, bytes)| json!({
                "label": label,
                "path": path,
                "bytes": bytes,
            })).collect::<Vec<_>>(),
            "disk_bytes": disk.iter().map(|f| f.2).sum::<u64>(),
            "cache_bytes": size_of(&local.join("downloads")),
        }));
        return Ok(());
    }
    if running.is_empty() {
        say!("No Kiln process is running.");
    } else {
        say!("Running:");
        for (label, count, memory, cpu) in &running {
            let many = if *count > 1 {
                format!(" ×{count}")
            } else {
                String::new()
            };
            say!(
                "  {:<40} {:>10}  {cpu:>5.1}% CPU",
                format!("{label}{many}"),
                megabytes(*memory)
            );
        }
        say!(
            "  {:<40} {:>10}",
            "Together",
            megabytes(running.iter().map(|g| g.2).sum())
        );
    }
    say!("On disk:");
    for (label, path, bytes) in &disk {
        say!(
            "  {:<40} {:>10}  {}",
            label,
            megabytes(*bytes),
            path.display()
        );
    }
    say!(
        "  {:<40} {:>10}",
        "Together",
        megabytes(disk.iter().map(|f| f.2).sum())
    );
    say!("`kiln cache clear` removes the download cache; Kiln downloads again what it needs.");
    Ok(())
}

/// `kiln cache clear`: deletes Kiln's download cache, which only holds verified copies of
/// archives Kiln has already installed.
pub fn clear_cache() -> Result<(), String> {
    let cache = local_dir()?.join("downloads");
    let before = size_of(&cache);
    let mut left = 0u64;
    if let Ok(entries) = std::fs::read_dir(&cache) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if removed.is_err() {
                left += size_of(&path);
            }
        }
    }
    let freed = before.saturating_sub(left);
    if crate::out::json() {
        crate::out::data(json!({ "freed_bytes": freed, "left_bytes": left }));
        return Ok(());
    }
    say!(
        "Freed {} from the download cache ({}).",
        megabytes(freed),
        cache.display()
    );
    if left > 0 {
        say!("{} is in use and stays.", megabytes(left));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_are_named_by_where_their_program_is() {
        let local = Path::new("C:\\Users\\a\\AppData\\Local\\Kiln");
        let kiln = Path::new("C:\\Program Files\\Kiln");
        let label = |image: &str| label_of(Path::new(image), local, kiln);
        assert_eq!(
            label("C:\\Program Files\\Kiln\\kilnd.exe").as_deref(),
            Some("Kiln's daemon")
        );
        assert_eq!(
            label("C:\\Users\\a\\AppData\\Local\\Kiln\\php\\8.4.27\\php-cgi.exe").as_deref(),
            Some("PHP 8.4.27 workers")
        );
        assert_eq!(
            label("C:\\Users\\a\\AppData\\Local\\Kiln\\php\\8.4.27\\php.exe").as_deref(),
            Some("PHP 8.4.27 (project processes)")
        );
        assert_eq!(
            label("c:\\users\\a\\appdata\\local\\kiln\\services\\bin\\mysql\\8.4.7\\bin\\kiln-mysqld.exe")
                .as_deref(),
            Some("mysql 8.4.7")
        );
        assert_eq!(
            label("C:\\Users\\a\\AppData\\Local\\Kiln\\bin\\kiln-caddy.exe").as_deref(),
            Some("Web server")
        );
        assert_eq!(label("C:\\Windows\\System32\\svchost.exe"), None);
        // Another folder whose name only starts like Kiln's.
        assert_eq!(
            label("C:\\Users\\a\\AppData\\Local\\KilnOther\\x.exe"),
            None
        );
    }

    #[test]
    fn sizes_add_up_without_following_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/one"), [0u8; 10]).unwrap();
        std::fs::write(dir.path().join("a/b/two"), [0u8; 5]).unwrap();
        assert_eq!(size_of(dir.path()), 15);
        assert_eq!(size_of(&dir.path().join("missing")), 0);
    }
}
