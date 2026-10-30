//! `kiln proxy|unproxy` ([ADR 0032]): a `.test` name served from a local dev server.
//!
//! [ADR 0032]: ../../../docs/adr/0032-proxy-sites.md

use kiln_state::ProxyRecord;
use kiln_web::ProxyTarget;

use crate::web::{apply_site_change, host_name, open_store, resolved};

/// `kiln proxy <name> <target> [--secure]`.
pub fn proxy(name: &str, target: &str, secure: bool) -> Result<(), String> {
    let host = host_name(name)?;
    let target = ProxyTarget::parse(target).map_err(|e| e.to_string())?;
    let mut store = open_store()?;
    if let Some(site) = store.site(host.as_str()).map_err(|e| e.to_string())? {
        return Err(format!(
            "{host} is linked to {}; `kiln unlink {name}` first, or pick another name",
            site.root.display()
        ));
    }
    if secure {
        crate::web::ca_files()?;
    }
    let previous = store.proxy(host.as_str()).map_err(|e| e.to_string())?;
    store
        .put_proxy(&ProxyRecord {
            host: host.to_string(),
            target: target.to_string(),
            // Run again without --secure, an HTTPS proxy stays HTTPS (`kiln unsecure` ends it).
            secure: secure || previous.as_ref().is_some_and(|p| p.secure),
        })
        .map_err(|e| e.to_string())?;
    match previous {
        Some(old) if old.target != target.to_string() => {
            println!("{host} now proxies to {target} (was {}).", old.target);
        }
        Some(_) => println!("{host} already proxies to {target}."),
        None => println!("{host} proxies to {target}."),
    }
    if let Some(site) = resolved(&store)?
        .sites
        .iter()
        .find(|s| s.host == host && s.alias_of.is_none())
    {
        println!(
            "It takes the name from the parked folder {}, which is served again after `kiln unproxy {name}`.",
            site.project.display()
        );
    }
    if let Some(url) = crate::web::site_urls(&store).get(host.as_str()) {
        println!("URL: {url}");
    }
    if secure {
        crate::web::trust_hint()?;
    }
    crate::web::hosts_hint(host.as_str());
    crate::coexist::note_shared_names(&store, &[host.to_string()]);
    apply_site_change(&store)
}

/// `kiln unproxy <name>`.
pub fn unproxy(name: &str) -> Result<(), String> {
    let host = host_name(name)?;
    let mut store = open_store()?;
    let Some(old) = store.proxy(host.as_str()).map_err(|e| e.to_string())? else {
        return Err(format!("{host} isn't a proxy (see `kiln sites`)"));
    };
    store
        .remove_proxy(host.as_str())
        .map_err(|e| e.to_string())?;
    println!("{host} no longer proxies to {}.", old.target);
    if let Some(site) = resolved(&store)?.sites.iter().find(|s| s.host == host) {
        println!(
            "It's served from its parked folder again: {}.",
            site.project.display()
        );
    } else if crate::web::hosts_names().iter().any(|n| n == host.as_str()) {
        println!("It's still in the hosts file; `kiln hosts sync` removes it.");
    }
    apply_site_change(&store)
}
