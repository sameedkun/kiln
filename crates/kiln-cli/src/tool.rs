//! `kiln tool add|remove`: phpMyAdmin and Adminer, served as Kiln sites and pointed at Kiln's
//! database instances ([ADR 0043]).
//!
//! [ADR 0043]: ../../../docs/adr/0043-phpmyadmin-and-adminer.md

use std::fmt::Write as _;
use std::path::PathBuf;

use kiln_services::tools::{self, Tool};
use kiln_state::StateStore;

/// `<local data>/tools`: each tool's folder, and `servers.json`.
pub(crate) fn tools_dir() -> Result<PathBuf, String> {
    Ok(kiln_state::default_local_dir()
        .ok_or("cannot determine Kiln's local data directory")?
        .join("tools"))
}

fn tool_of(name: &str) -> Result<&'static Tool, String> {
    tools::tool(name).ok_or_else(|| {
        let known: Vec<_> = tools::TOOLS.iter().map(|t| t.name).collect();
        format!("no tool `{name}`; Kiln has {}", known.join(", "))
    })
}

/// Rewrites `servers.json`, the instance list the tools read at every request: each
/// instance's name, kind and main port. Only once a tool is installed (the folder exists).
pub(crate) fn write_servers(store: &StateStore) -> Result<(), String> {
    let dir = tools_dir()?;
    if !dir.is_dir() {
        return Ok(());
    }
    let services = store.services().map_err(|e| e.to_string())?;
    let list: Vec<serde_json::Value> = services
        .iter()
        .filter_map(|record| {
            let provider = kiln_services::provider(&record.kind)?;
            let main = provider.ports().first()?;
            let port = record.ports.get(main.role).copied()?;
            Some(serde_json::json!({ "name": record.name, "kind": record.kind, "port": port }))
        })
        .collect();
    let text = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    write_file(&dir.join("servers.json"), &text)
}

/// Writes one of Kiln's own files whole: a temporary file beside it, then swapped in.
fn write_file(path: &std::path::Path, text: &str) -> Result<(), String> {
    let temp = path.with_extension("partial");
    std::fs::write(&temp, text)
        .and_then(|()| kiln_sys::rename_replace(&temp, path))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// 32 random characters, for phpMyAdmin's cookie encryption.
fn secret() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random source: {e}"))?;
    Ok(bytes.iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    }))
}

/// `kiln tool add <name>`.
pub fn add(name: &str) -> Result<(), String> {
    let tool = tool_of(name)?;
    let store = crate::web::open_store()?;
    if store.php_versions().map_err(|e| e.to_string())?.is_empty() {
        return Err(format!(
            "{} runs on PHP, and Kiln has none installed yet. Install one first, e.g. \
             `kiln php install 8.5`.",
            tool.display
        ));
    }
    let dir = tools_dir()?;
    let parent = dir.join(tool.name);
    let root = tool.root(&dir);
    if root.join(tool.release.executable).is_file() {
        println!(
            "{} {} is already installed.",
            tool.display, tool.release.version
        );
    } else {
        let archive = crate::service::download(&tool.release)?;
        kiln_services::install_into_folder(&tool.release, &archive, &parent)
            .map_err(|e| e.to_string())?;
        println!(
            "Verified {} (SHA-256 matches the pinned release) and installed {} {}.",
            tool.release.archive, tool.display, tool.release.version
        );
        println!("License: {}.", tool.license);
    }
    let temp = parent.join("tmp");
    std::fs::create_dir_all(&temp).map_err(|e| format!("cannot create {}: {e}", temp.display()))?;
    for (file, text) in tool.files(&dir, &dir.join("servers.json"), &secret()?) {
        write_file(&root.join(file), &text)?;
    }
    write_servers(&store)?;
    drop(store);
    crate::web::link(Some(tool.site.to_owned()), Some(root), false)
}

/// `kiln tool remove <name>`: the site goes, and the tool's folder with it.
pub fn remove(name: &str) -> Result<(), String> {
    let tool = tool_of(name)?;
    let parent = tools_dir()?.join(tool.name);
    if !parent.exists() {
        return Err(format!("{} isn't installed", tool.display));
    }
    if let Err(e) = crate::web::unlink(tool.site) {
        println!("Note: {e}.");
    }
    std::fs::remove_dir_all(&parent)
        .map_err(|e| format!("cannot delete {}: {e}", parent.display()))?;
    println!("Removed {} ({}).", tool.display, parent.display());
    Ok(())
}
