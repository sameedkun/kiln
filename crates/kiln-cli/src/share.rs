//! `kiln share <site> [--stop]` ([ADR 0076]): a site reachable from anywhere
//! through a Cloudflare quick tunnel while it's shared, and not after.
//!
//! [ADR 0076]: ../../../docs/adr/0076-sharing-sites.md

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;

/// How long `share` waits for the tunnel's public address.
const WAIT: Duration = Duration::from_secs(45);

/// A served site's host and folder, by its name.
fn site(name: &str) -> Result<(String, PathBuf), String> {
    let store = crate::web::open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let host = crate::web::host_name_under(name, &tld)?.as_str().to_owned();
    crate::web::resolved(&store)?
        .sites
        .iter()
        .find(|s| s.host.as_str() == host && s.alias_of.is_none())
        .map(|s| (host.clone(), s.project.clone()))
        .ok_or_else(|| format!("Kiln doesn't serve {host}"))
}

/// The tunnel instance sharing `project`, if there is one.
fn tunnel_of(project: &std::path::Path) -> Result<Option<kiln_state::ServiceRecord>, String> {
    Ok(crate::web::open_store()?
        .services()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|r| r.kind == "tunnel" && r.project.as_deref() == Some(project)))
}

/// The public address in an instance's log so far, if its tunnel is up.
pub(crate) fn url_of(record: &kiln_state::ServiceRecord) -> Option<String> {
    let dirs = crate::service::dirs_of(record).ok()?;
    let text = std::fs::read(&dirs.log).ok()?;
    let tail = &text[text.len().saturating_sub(65_536)..];
    kiln_services::tunnel::shared_url(&String::from_utf8_lossy(tail))
}

/// `kiln share <site>`: starts its tunnel (or finds it running) and says its public address.
pub fn share(name: &str) -> Result<(), String> {
    let (host, project) = site(name)?;
    let record = if let Some(record) = tunnel_of(&project)? {
        record
    } else {
        crate::service::add("tunnel", None, None, &[], Some(project.clone()))?;
        tunnel_of(&project)?.ok_or("the tunnel wasn't added")?
    };
    let deadline = Instant::now() + WAIT;
    let url = loop {
        if let Some(url) = url_of(&record) {
            break url;
        }
        if Instant::now() > deadline {
            return Err(format!(
                "{host}'s tunnel started but gave no address within {} s: see `kiln service logs {}`",
                WAIT.as_secs(),
                record.name
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    if crate::out::json() {
        crate::out::data(json!({"host": host, "url": url, "instance": record.name}));
    }
    say!("{host} is shared at {url}");
    say!(
        "Anyone with that address can open it while it's shared; `kiln share {name} --stop` stops it."
    );
    Ok(())
}

/// `kiln share <site> [--stop]`.
pub fn run(site: &str, stop: bool) -> Result<(), String> {
    if stop { unshare(site) } else { share(site) }
}

/// `kiln share <site> --stop`: stops and removes its tunnel; the address stops working.
pub fn unshare(name: &str) -> Result<(), String> {
    let (host, project) = site(name)?;
    let Some(record) = tunnel_of(&project)? else {
        say!("{host} isn't shared.");
        return Ok(());
    };
    // Its data is only Kiln's own empty config and the log.
    crate::service::remove(&record.name, true)?;
    say!("{host} isn't shared any more.");
    Ok(())
}
