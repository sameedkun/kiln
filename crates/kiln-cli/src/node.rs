//! Node runtimes ([ADR 0028]): `kiln node available|install|list|remove|default`.
//!
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use std::path::PathBuf;

use kiln_node::index::{self, INDEX_LIMIT, INDEX_URL};
use kiln_node::{NodeRelease, Platform, Spec};
use kiln_runtime::node::{DEFAULT_KEY, NODE_DIR};

fn node_root() -> Result<PathBuf, String> {
    kiln_state::default_local_dir()
        .map(|local| local.join(NODE_DIR))
        .ok_or_else(|| "cannot determine Kiln's local data directory".to_owned())
}

fn platform() -> Result<Platform, String> {
    Platform::current().ok_or_else(|| {
        format!(
            "Kiln doesn't install Node on {} {} yet",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })
}

fn releases(fetcher: &kiln_fetch::Fetcher) -> Result<Vec<NodeRelease>, String> {
    let json = fetcher
        .get_text(INDEX_URL, INDEX_LIMIT)
        .map_err(|e| e.to_string())?;
    index::parse(&json).map_err(|e| e.to_string())
}

fn default_spec() -> Result<Option<String>, String> {
    crate::web::open_store()?
        .setting(DEFAULT_KEY)
        .map_err(|e| e.to_string())
}

/// `kiln node available`: the newest release of each line.
pub fn available() -> Result<(), String> {
    let platform = platform()?;
    let releases = releases(&crate::php::fetcher()?)?;
    let installed = kiln_node::installed(&node_root()?);
    println!(
        "Node releases for {} from nodejs.org (newest of each line):",
        platform.suffix
    );
    for release in index::newest_per_line(&releases, platform) {
        let lts = release
            .lts
            .as_deref()
            .map(|name| format!("LTS {name}"))
            .unwrap_or_default();
        let mark = if installed.iter().any(|n| n.version == release.version) {
            "  (installed)"
        } else {
            ""
        };
        println!(
            "  {:<4} {:<9} {:<12} {}{mark}",
            release.numbers[0], release.version, lts, release.date
        );
    }
    println!("Install one with `kiln node install <version>`, e.g. `kiln node install lts`.");
    Ok(())
}

/// `kiln node install <spec>` ([ADR 0028] §2–3).
///
/// [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md
pub fn install(spec: &str) -> Result<(), String> {
    let spec = Spec::parse(spec)?;
    let platform = platform()?;
    let fetcher = crate::php::fetcher()?;
    let releases = releases(&fetcher)?;
    let release = index::resolve(&releases, &spec, platform).map_err(|e| e.to_string())?;
    println!(
        "Node {}{} for {}",
        release.version,
        release
            .lts
            .as_deref()
            .map(|name| format!(" (LTS {name})"))
            .unwrap_or_default(),
        platform.suffix
    );
    let label = format!("Downloading {}", platform.archive(&release.version));
    let installed = kiln_node::install::install(
        &fetcher,
        release,
        platform,
        &node_root()?,
        &mut crate::php::progress_printer(&label),
        &kiln_node::install::check_runs,
    )
    .map_err(|e| e.to_string())?;
    let node = &installed.node;
    if installed.already {
        println!(
            "Node {} is already installed in {}.",
            node.version,
            node.dir.display()
        );
    } else {
        println!(
            "Checked SHASUMS256.txt's signature (key {}), then the archive's SHA-256.",
            installed.signer.as_deref().unwrap_or("?")
        );
        println!(
            "Installed Node {} in {}. It checked out: `node --version` reports v{}.",
            node.version,
            node.dir.display(),
            node.version
        );
    }
    let shims = kiln_state::default_local_dir()
        .map(|local| local.join(kiln_runtime::shim::SHIMS_DIR))
        .unwrap_or_default();
    if !shims
        .join(kiln_runtime::shim::Tool::Node.file_name())
        .is_file()
    {
        println!("Run `kiln shims install` to get `node`, `npm`, `npx` and `corepack` on PATH.");
    }
    Ok(())
}

/// `kiln node list`.
pub fn list() -> Result<(), String> {
    let installed = kiln_node::installed(&node_root()?);
    if installed.is_empty() {
        println!("No Node installed by Kiln yet. See `kiln node available`.");
        return Ok(());
    }
    let default_spec = default_spec()?;
    let default = kiln_runtime::node::default_node(&installed, default_spec.as_deref());
    for node in &installed {
        let lts = node
            .lts
            .as_deref()
            .map(|name| format!("LTS {name}"))
            .unwrap_or_default();
        let mark = if default.is_some_and(|d| d.version == node.version) {
            "  (default)"
        } else {
            ""
        };
        println!(
            "{:<9} {:<12} {}{mark}",
            node.version,
            lts,
            node.dir.display()
        );
    }
    Ok(())
}

/// `kiln node remove <version>`: deletes the folder, global packages included.
pub fn remove(version: &str) -> Result<(), String> {
    let root = node_root()?;
    let installed = kiln_node::installed(&root);
    let Some(node) = installed
        .iter()
        .find(|n| n.version == version.trim_start_matches('v'))
    else {
        return Err(format!(
            "Node {version} isn't installed by Kiln (see `kiln node list`)"
        ));
    };
    // Only ever delete a folder inside Kiln's own Node directory.
    if node.dir.parent() != Some(root.as_path()) {
        return Err(format!(
            "refusing to delete {}: it isn't inside {}",
            node.dir.display(),
            root.display()
        ));
    }
    std::fs::remove_dir_all(&node.dir).map_err(|e| {
        format!(
            "cannot remove {} (is a Node process still running?): {e}",
            node.dir.display()
        )
    })?;
    println!("Removed Node {} ({}).", node.version, node.dir.display());
    Ok(())
}

/// `kiln node default <spec>`.
pub fn set_default(spec: &str) -> Result<(), String> {
    let parsed = Spec::parse(spec)?;
    let installed = kiln_node::installed(&node_root()?);
    let Some(node) = parsed.newest(&installed, |n| (n.numbers, n.lts.as_deref())) else {
        return Err(format!(
            "no installed Node matches {parsed}; see `kiln node list`, or `kiln node install {parsed}`"
        ));
    };
    crate::web::open_store()?
        .set_setting(DEFAULT_KEY, &parsed.to_string())
        .map_err(|e| e.to_string())?;
    println!(
        "Node {} is now the default (where no project asks for a version; it follows {parsed}).",
        node.version
    );
    Ok(())
}
