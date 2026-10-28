//! `kiln import herd` ([ADR 0027] §5): a project's `herd.yml` becomes its `kiln.toml`.
//!
//! [ADR 0027]: ../../../docs/adr/0027-project-config-kiln-toml.md

use std::path::PathBuf;

use kiln_sites::{herd, manifest};

/// Reads `<folder>/herd.yml`, writes `<folder>/kiln.toml`, and links the folder if it isn't
/// a site yet.
pub fn import_herd(folder: Option<PathBuf>, force: bool) -> Result<(), String> {
    let folder = match folder {
        Some(folder) => folder,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let folder = std::fs::canonicalize(&folder)
        .map_err(|e| format!("cannot use {}: {e}", folder.display()))?;
    let folder = crate::web::strip_verbatim(folder);
    let source = folder.join(herd::FILE);
    let text = std::fs::read_to_string(&source)
        .map_err(|e| format!("cannot read {}: {e}", source.display()))?;
    let imported = herd::parse(&text).map_err(|e| format!("{}: {e}", source.display()))?;

    let target = folder.join(manifest::FILE);
    if target.exists() && !force {
        return Err(format!(
            "{} already exists; `--force` replaces it",
            target.display()
        ));
    }
    let toml = manifest::render(
        &imported.manifest,
        "Kiln project settings (ADR 0027), imported from herd.yml.",
    );
    // Only write what Kiln itself will read back.
    manifest::parse(&toml)
        .map_err(|e| format!("{}: can't be carried over: {e}", source.display()))?;
    std::fs::write(&target, &toml)
        .map_err(|e| format!("cannot write {}: {e}", target.display()))?;

    let m = &imported.manifest;
    println!("Wrote {} from {}:", target.display(), source.display());
    if let Some(name) = &m.name {
        println!("  name:    {name}");
    }
    if let Some(php) = &m.php {
        println!("  PHP:     {php}");
    }
    if let Some(secure) = m.secure {
        println!("  HTTPS:   {}", if secure { "yes" } else { "no" });
    }
    if !m.aliases.is_empty() {
        println!("  aliases: {}", m.aliases.join(", "));
    }
    for note in &imported.notes {
        println!("Note: {note}");
    }

    let store = crate::web::open_store()?;
    let sites = kiln_runtime::sites(&store).map_err(|e| e.to_string())?;
    let served = sites
        .sites
        .iter()
        .find(|s| s.alias_of.is_none() && s.project == folder)
        .map(|s| s.host.to_string());
    if let Some(host) = served {
        println!("{host} already serves this folder; the settings apply from now on.");
        crate::web::apply_site_change(&store)?;
    } else {
        drop(store);
        crate::web::link(None, Some(folder), false)?;
    }
    if !m.aliases.is_empty() {
        println!("Run `kiln hosts sync` so the new names resolve.");
    }
    Ok(())
}
