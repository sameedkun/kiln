//! `kiln domain add|remove|list` ([ADR 0083]): extra names for a site, kept in Kiln's state
//! (never in the project), served as aliases are. Like groups (ADR 0074), nothing of the
//! project's or the system's changes, so it isn't journaled: `remove` is the way back.
//!
//! [ADR 0083]: ../../../docs/adr/0083-extra-site-names.md

use std::collections::BTreeMap;

use kiln_core::HostName;
use kiln_runtime::DOMAINS_KEY;
use kiln_state::StateStore;

/// The main site `site` names: served by Kiln, not an alias and not a proxy.
fn main_site(store: &StateStore, site: &str) -> Result<HostName, String> {
    let tld = kiln_runtime::tld(store).map_err(|e| e.to_string())?;
    let host = crate::web::host_name_under(site, &tld)?;
    if crate::web::proxy_specs(store)?
        .iter()
        .any(|p| p.host == host)
    {
        return Err(format!(
            "{host} is a proxy; extra names are for sites Kiln serves from a folder"
        ));
    }
    let resolved = crate::web::resolved(store)?;
    match resolved.sites.iter().find(|s| s.host == host) {
        Some(s) if s.alias_of.is_some() => Err(format!(
            "{host} is another name for {}: add names to that site",
            s.main_host()
        )),
        Some(_) => Ok(host),
        None => Err(format!("Kiln doesn't serve {host}")),
    }
}

fn save(store: &mut StateStore, all: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    let text = serde_json::to_string(all).map_err(|e| e.to_string())?;
    store
        .set_setting(DOMAINS_KEY, &text)
        .map_err(|e| e.to_string())
}

/// Forgets a site's names when the site itself is gone (`kiln unlink`, ADR 0083 §3), so a
/// later site of that name doesn't get them back unasked. Returns the names forgotten.
pub(crate) fn forget(store: &mut StateStore, host: &str) -> Result<Vec<String>, String> {
    let mut all = kiln_runtime::domains(store);
    let Some(names) = all.remove(host) else {
        return Ok(Vec::new());
    };
    save(store, &all)?;
    Ok(names)
}

/// `kiln domain add <site> <name>...`.
pub fn add(site: &str, names: &[String]) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let main = main_site(&store, site)?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let resolved = crate::web::resolved(&store)?;
    let proxies = crate::web::proxy_specs(&store)?;
    let mut all = kiln_runtime::domains(&store);
    let mut added = Vec::new();
    for name in names {
        let host = kiln_sites::full_name(name, &tld)?;
        if let Some(s) = resolved.sites.iter().find(|s| s.host == host) {
            if s.alias_of.as_ref() == Some(&main) {
                say!("{host} is already a name of {main}.");
                continue;
            }
            return Err(format!(
                "{host} is already a site ({})",
                s.project.display()
            ));
        }
        if proxies.iter().any(|p| p.host == host) {
            return Err(format!("{host} is already a proxy"));
        }
        let list = all.entry(main.to_string()).or_default();
        if !list.contains(&host.to_string()) {
            list.push(host.to_string());
            added.push(host);
        }
    }
    if added.is_empty() {
        return Ok(());
    }
    save(&mut store, &all)?;
    for host in &added {
        say!("{host} is another name for {main} now.");
        crate::web::hosts_hint(host.as_str());
    }
    let hosts: Vec<String> = added.iter().map(ToString::to_string).collect();
    crate::coexist::note_shared_names(&store, &hosts);
    crate::web::apply_site_change(&store)
}

/// `kiln domain remove <site> <name>...`.
pub fn remove(site: &str, names: &[String]) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    // The main site may be gone (unlinked): its names can still be removed.
    let main = crate::web::host_name_under(site, &tld)?.to_string();
    let mut all = kiln_runtime::domains(&store);
    let Some(list) = all.get_mut(&main) else {
        return Err(format!("{main} has no extra names of Kiln's"));
    };
    for name in names {
        let host = kiln_sites::full_name(name, &tld)?.to_string();
        let Some(at) = list.iter().position(|h| *h == host) else {
            return Err(format!(
                "{host} isn't one of {main}'s names in Kiln{}",
                if list.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", list.join(", "))
                }
            ));
        };
        list.remove(at);
        say!("{host} is no longer a name of {main}.");
    }
    if list.is_empty() {
        all.remove(&main);
    }
    save(&mut store, &all)?;
    say!("`kiln hosts sync` takes the names out of the hosts file.");
    crate::web::apply_site_change(&store)
}

/// `kiln domain list [<site>]`: each site's extra names, Kiln's and its `kiln.toml`'s.
pub fn list(site: Option<&str>) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let only = site
        .map(|s| crate::web::host_name_under(s, &tld))
        .transpose()?;
    let resolved = crate::web::resolved(&store)?;
    let mut by_main: BTreeMap<String, Vec<(String, &str)>> = BTreeMap::new();
    for s in &resolved.sites {
        if let Some(main) = &s.alias_of {
            let from = if s.domain { "kiln" } else { "kiln.toml" };
            by_main
                .entry(main.to_string())
                .or_default()
                .push((s.host.to_string(), from));
        }
    }
    if let Some(host) = &only {
        by_main.retain(|main, _| main == host.as_str());
    }
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "sites": by_main.iter().map(|(main, names)| serde_json::json!({
                "host": main,
                "names": names.iter().map(|(h, from)| serde_json::json!({"host": h, "from": from})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if by_main.is_empty() {
        say!(
            "{} no other names. `kiln domain add <site> <name>` adds one.",
            only.map_or_else(|| "No site has".to_owned(), |h| format!("{h} has"))
        );
    }
    for (main, names) in &by_main {
        say!("{main}");
        for (host, from) in names {
            say!(
                "  {host}  {}",
                if *from == "kiln" {
                    "(Kiln)"
                } else {
                    "(kiln.toml)"
                }
            );
        }
    }
    Ok(())
}
