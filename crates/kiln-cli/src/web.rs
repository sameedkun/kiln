//! Sites and the web server ([ADR 0011], [ADR 0012]): `kiln link|unlink|secure|unsecure|sites`,
//! `kiln web install|start|stop|reload|restart|status|config` and `kiln hosts sync`.
//!
//! Sites are served over plain HTTP unless `kiln secure` turns HTTPS on for them.
//!
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md
//! [ADR 0012]: ../../../docs/adr/0012-caddy-admin-channel.md

use std::collections::BTreeMap;
use std::net::{Ipv6Addr, TcpListener};
use std::path::{Path, PathBuf};

use kiln_core::{HostName, Tld};
use kiln_state::{SiteRecord, StateStore};
use kiln_supervisor::Pools;
use kiln_web::{CaFiles, SiteSpec, Status, WebConfig, WebServer};

use crate::apply;

pub(crate) const HTTP_PORT_KEY: &str = "web.http_port";
pub(crate) const HTTPS_PORT_KEY: &str = "web.https_port";
pub(crate) const CADDY_VERSION_KEY: &str = "web.caddy_version";
/// Re-issue the intermediate when it has fewer days left than this (ADR 0011 §7).
const INTERMEDIATE_MIN_DAYS: i64 = 10;

pub(crate) fn open_store() -> Result<StateStore, String> {
    let dir = kiln_state::default_state_dir().ok_or("cannot determine the state directory")?;
    StateStore::open_dir(&dir)
        .map(|(store, _)| store)
        .map_err(|e| e.to_string())
}

fn local_dir() -> Result<PathBuf, String> {
    kiln_state::default_local_dir()
        .ok_or_else(|| "cannot determine Kiln's local data directory".to_owned())
}

pub(crate) fn server() -> Result<WebServer, String> {
    let local = local_dir()?;
    Ok(WebServer::new(
        local.join("bin").join(kiln_web::EXE_NAME),
        local.join("caddy"),
    ))
}

/// The development TLD sites are served under ([ADR 0033]).
///
/// [ADR 0033]: ../../../docs/adr/0033-configurable-tld.md
pub(crate) fn current_tld() -> Result<Tld, String> {
    kiln_runtime::tld(&open_store()?).map_err(|e| e.to_string())
}

/// `blog` or `blog.test` → `blog.test` (under the current TLD), validated.
pub(crate) fn host_name(name: &str) -> Result<HostName, String> {
    host_name_under(name, &current_tld()?)
}

/// `blog` or `blog.<tld>` → `blog.<tld>`, validated.
pub(crate) fn host_name_under(name: &str, tld: &Tld) -> Result<HostName, String> {
    let name = name.to_ascii_lowercase();
    let fqdn = if name.ends_with(&format!(".{tld}")) {
        name
    } else {
        format!("{name}.{tld}")
    };
    HostName::new(&fqdn, tld).map_err(|e| e.to_string())
}

pub(crate) fn port_setting(store: &StateStore, key: &str, default: u16) -> Result<u16, String> {
    match store.setting(key).map_err(|e| e.to_string())? {
        None => Ok(default),
        Some(value) => value
            .parse()
            .map_err(|_| format!("setting {key} holds {value:?}, which isn't a port")),
    }
}

/// Makes sure the CA can back HTTPS sites: the root exists and the intermediate has at least
/// [`INTERMEDIATE_MIN_DAYS`] left (re-issued here if not; Caddy can't renew it).
pub(crate) fn ca_files() -> Result<CaFiles, String> {
    let ca = apply::ca_store()?;
    if !ca.root_exists() {
        return Err(
            "HTTPS sites need Kiln's CA: run `kiln ca init` and `kiln ca trust` first".into(),
        );
    }
    let fresh_enough = ca
        .intermediate()
        .map_err(|e| e.to_string())?
        .is_some_and(|i| {
            (i.not_after - time::OffsetDateTime::now_utc()).whole_days() >= INTERMEDIATE_MIN_DAYS
        });
    if !fresh_enough {
        let issued = ca
            .issue_intermediate(&apply::root_tld(&ca)?)
            .map_err(|e| e.to_string())?;
        say!(
            "Issued a new intermediate certificate for the web server (expires {}).",
            issued.not_after.date()
        );
    }
    let (root_cert, intermediate_cert, intermediate_key) = ca.web_server_files();
    Ok(CaFiles {
        root_cert,
        intermediate_cert,
        intermediate_key,
    })
}

/// `kiln ca renew` ([ADR 0026] §2, P2-03): a new intermediate when fewer than
/// [`INTERMEDIATE_MIN_DAYS`] are left (or `force`), then a running server with HTTPS sites
/// restarts to use it (a reload would keep old site certificates in Caddy's memory, I-57).
/// `kilnd` runs this when the intermediate is due.
///
/// [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md
pub fn ca_renew(force: bool) -> Result<(), String> {
    let ca = apply::ca_store()?;
    if !ca.root_exists() {
        return Err("Kiln has no CA yet; `kiln ca init` creates one".into());
    }
    let days_left = ca
        .intermediate()
        .map_err(|e| e.to_string())?
        .map(|i| (i.not_after - time::OffsetDateTime::now_utc()).whole_days());
    if let Some(days) = days_left.filter(|&d| !force && d >= INTERMEDIATE_MIN_DAYS) {
        say!(
            "The web server's intermediate certificate is valid for {days} more days; it is renewed when fewer than {INTERMEDIATE_MIN_DAYS} are left."
        );
        return Ok(());
    }
    let issued = ca
        .issue_intermediate(&apply::root_tld(&ca)?)
        .map_err(|e| e.to_string())?;
    say!(
        "Issued a new intermediate certificate for the web server (expires {}).",
        issued.not_after.date()
    );
    if is_running()? && restart_if_ca_changed(&open_store()?)? {
        say!("HTTPS sites now use it.");
    }
    Ok(())
}

/// The SHA-256 of the intermediate `config` hands Caddy; `None` without HTTPS sites.
fn intermediate_sha256(config: &WebConfig) -> Result<Option<String>, String> {
    let Some(ca) = &config.ca else {
        return Ok(None);
    };
    let pem = std::fs::read_to_string(&ca.intermediate_cert)
        .map_err(|e| format!("cannot read {}: {e}", ca.intermediate_cert.display()))?;
    let der = kiln_pki::pem_to_der(&pem).map_err(|e| e.to_string())?;
    Ok(Some(kiln_pki::sha256_fingerprint(&der)))
}

/// Records which intermediate Caddy issues from, clearing site certificates from any other
/// (I-57). Done whenever Caddy gets the CA, so the first certificates are recorded too (I-72).
fn record_issuer(config: &WebConfig) -> Result<(), String> {
    match intermediate_sha256(config)? {
        Some(sha256) => kiln_web::issued::forget_other_issuers(&config.storage_dir, &sha256)
            .map_err(|e| format!("cannot clear old site certificates: {e}")),
        None => Ok(()),
    }
}

/// After Kiln's CA changed, a reload would keep serving the old site certificates from
/// Caddy's memory, so a running server restarts instead (I-57). Returns whether it did.
fn restart_if_ca_changed(store: &StateStore) -> Result<bool, String> {
    let config = web_config(store)?; // may re-issue an expiring intermediate first
    let changed = intermediate_sha256(&config)?
        .is_some_and(|sha256| !kiln_web::issued::issued_from(&config.storage_dir, &sha256));
    if !changed {
        record_issuer(&config)?;
        return Ok(false);
    }
    say!(
        "Kiln's CA changed since the web server started; restarting it so sites get certificates from the new one."
    );
    // Only Caddy restarts: the PHP workers don't depend on the CA, and the new config is
    // validated before the old server stops, so sites go unanswered for a moment (ADR 0034).
    record_issuer(&config)?;
    let pid = server()?.restart(&config).map_err(|e| e.to_string())?;
    wait_for_certificates(store)?;
    say!("Web server restarted (PID {pid}); the PHP workers kept running.");
    Ok(true)
}

fn web_config(store: &StateStore) -> Result<WebConfig, String> {
    let run = server()?;
    let run_dir = run
        .config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let site_pools = crate::pool::needs(store)?.site_pools;
    let sites = all_specs(store, &site_pools)?;
    let ca = if sites.iter().any(|s| s.secure) {
        Some(ca_files()?)
    } else {
        None
    };
    Ok(WebConfig {
        http_port: crate::coexist::planned_port(store, &crate::coexist::HTTP)?,
        https_port: crate::coexist::planned_port(store, &crate::coexist::HTTPS)?,
        ipv6: TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok(),
        sites,
        ca,
        storage_dir: run_dir.join("storage"),
        log_file: run.log_file(),
        admin_socket: run.admin_socket(),
        php_pools: crate::pool::running_upstreams()?,
    })
}

/// Every site Kiln serves: links plus parked subfolders, each with its detected driver
/// ([ADR 0015]).
///
/// [ADR 0015]: ../../../docs/adr/0015-parked-folders-and-drivers.md
pub(crate) fn resolved(store: &StateStore) -> Result<kiln_sites::Resolved, String> {
    kiln_runtime::sites(store).map_err(|e| e.to_string())
}

/// Proxy sites as the web server sees them ([ADR 0032]).
///
/// [ADR 0032]: ../../../docs/adr/0032-proxy-sites.md
pub(crate) fn proxy_specs(store: &StateStore) -> Result<Vec<SiteSpec>, String> {
    store
        .proxies()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| {
            Ok(SiteSpec {
                host: host_name(&p.host)?,
                root: PathBuf::new(),
                secure: p.secure,
                php_pool: None,
                proxy: Some(
                    kiln_web::ProxyTarget::parse(&p.target)
                        .map_err(|e| format!("{}: {e}", p.host))?,
                ),
            })
        })
        .collect()
}

/// Every site the server serves: proxies, then the folder sites whose name no proxy has
/// taken (a proxy shadows a parked subfolder, [ADR 0032] §4).
///
/// [ADR 0032]: ../../../docs/adr/0032-proxy-sites.md
fn all_specs(
    store: &StateStore,
    site_pools: &BTreeMap<String, String>,
) -> Result<Vec<SiteSpec>, String> {
    let mut sites = proxy_specs(store)?;
    let folders: Vec<SiteSpec> = resolved(store)?
        .sites
        .iter()
        .filter(|site| !sites.iter().any(|p| p.host == site.host))
        .map(|site| spec(site, site_pools))
        .collect();
    sites.extend(folders);
    Ok(sites)
}

/// The web server's view of a site; `site_pools` maps PHP sites to their pool (ADR 0016 §6).
fn spec(site: &kiln_sites::Site, site_pools: &BTreeMap<String, String>) -> SiteSpec {
    SiteSpec {
        host: site.host.clone(),
        root: site.document_root.clone(),
        secure: site.secure,
        php_pool: site_pools.get(site.host.as_str()).cloned(),
        proxy: None,
    }
}

/// "Detected laravel: serving …\public" when the driver picked a subfolder.
fn describe(site: &kiln_sites::Site) -> String {
    if site.document_root == site.project {
        format!("{} site", site.driver.name())
    } else {
        format!(
            "{} project, serving {}",
            site.driver.name(),
            site.document_root.display()
        )
    }
}

/// After a change to sites, pins or PHP settings: bring a running server in line without
/// restarting it. PHP pools that are still needed keep running untouched; new ones start
/// beside them and old ones retire after Caddy has moved (ADR 0012, ADR 0016 §7).
pub fn apply_site_change(store: &StateStore) -> Result<(), String> {
    if !is_running()? {
        say!("The web server isn't running; it takes effect at `kiln web start`.");
        return Ok(());
    }
    if restart_if_ca_changed(store)? {
        return Ok(());
    }
    let saved =
        |e: String| format!("the change is saved, but the running server didn't take it: {e}");
    let running = crate::pool::running()?;
    let planned = crate::pool::plan(store, running.as_ref())?;
    match (&running, &planned) {
        (Some(old), Some(new)) if old != new => {
            crate::pool::switch(new, reload_with_pools).map_err(saved)?;
            wait_for_certificates(store)?;
            say!("Web server and PHP pools updated without a restart.");
            print_php(Some(new));
            return Ok(());
        }
        (None, Some(new)) => {
            crate::pool::supervisor()?
                .start(new)
                .map_err(|e| saved(e.to_string()))?;
            print_php(Some(new));
        }
        _ => {}
    }
    reload_with_pools(&crate::pool::running_upstreams()?).map_err(saved)?;
    if let (Some(_), None) = (&running, &planned) {
        // No PHP is needed any more.
        crate::pool::supervisor()?
            .stop()
            .map_err(|e| saved(e.to_string()))?;
    }
    wait_for_certificates(store)?;
    say!("Web server reloaded with the change.");
    Ok(())
}

/// Caddy issues a newly secured site's certificate in the background after a reload, and
/// until then refuses the handshake (I-69). Wait a little for it, so the URL Kiln prints
/// next works at once.
fn wait_for_certificates(store: &StateStore) -> Result<(), String> {
    let config = web_config(store)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let missing: Vec<_> = config
            .sites
            .iter()
            .filter(|s| s.secure)
            .map(|s| s.host.as_str())
            .filter(|h| !kiln_web::issued::has_certificate(&config.storage_dir, h))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            say!(
                "Caddy is still issuing the certificate for {}; HTTPS answers once it's done.",
                missing.join(", ")
            );
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Restarts the running workers of PHP `version` so they read its changed `php.ini`, without
/// downtime ([ADR 0030] §5): a new pool starts beside the old one, Caddy switches to it, and
/// the old one retires.
///
/// [ADR 0030]: ../../../docs/adr/0030-php-extension-toggles.md
pub fn restart_php(store: &StateStore, version: &str) -> Result<(), String> {
    let running = if is_running()? {
        crate::pool::running()?
    } else {
        None
    };
    let Some(running) = running.filter(|r| r.pools.iter().any(|p| p.id == version)) else {
        say!("No site is served by PHP {version} right now; its next start uses the change.");
        return Ok(());
    };
    let others = Pools {
        pools: running
            .pools
            .iter()
            // Its debug worker too: its php.ini is the PHP's, written again (ADR 0061).
            .filter(|p| {
                p.id != version && p.id != format!("{version}{}", kiln_web::DEBUG_POOL_SUFFIX)
            })
            .cloned()
            .collect(),
    };
    let Some(planned) = crate::pool::plan(store, Some(&others))? else {
        return Ok(());
    };
    crate::pool::switch(&planned, reload_with_pools).map_err(|e| {
        format!("the change is saved, but PHP {version}'s workers didn't restart: {e}")
    })?;
    say!("PHP {version}'s workers restarted with the change.");
    Ok(())
}

pub(crate) fn hosts_hint(host: &str) {
    if !hosts_names().iter().any(|n| n == host) {
        say!("Note: {host} isn't in the hosts file yet; run `kiln hosts sync`.");
    }
}

/// Names currently in Kiln's hosts-file block (empty if it can't be read).
pub(crate) fn hosts_names() -> Vec<String> {
    std::fs::read(crate::system_hosts_path())
        .ok()
        .and_then(|bytes| kiln_platform::hosts::read_block(&bytes).ok())
        .map(|entries| entries.into_iter().map(|(_, name)| name).collect())
        .unwrap_or_default()
}

pub fn link(name: Option<String>, path: Option<PathBuf>, secure: bool) -> Result<(), String> {
    let root = match path {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let root =
        std::fs::canonicalize(&root).map_err(|e| format!("cannot use {}: {e}", root.display()))?;
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    let root = strip_verbatim(root);
    // A name given here, else `[site] name` in the project's kiln.toml, else the folder's.
    let from_project = kiln_sites::manifest::read(&root)
        .ok()
        .flatten()
        .and_then(|m| m.name);
    let name = match name.or(from_project) {
        Some(name) => name,
        None => root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("can't derive a site name from the folder; pass one")?
            .to_owned(),
    };
    let host = host_name(&name)?;
    let mut store = open_store()?;
    if let Some(proxy) = store.proxy(host.as_str()).map_err(|e| e.to_string())? {
        return Err(format!(
            "{host} is a proxy to {}; `kiln unproxy {name}` first",
            proxy.target
        ));
    }
    if secure {
        ca_files()?;
    }
    let previous = store.site(host.as_str()).map_err(|e| e.to_string())?;
    store
        .put_site(&SiteRecord {
            host: host.to_string(),
            root: root.clone(),
            secure,
        })
        .map_err(|e| e.to_string())?;
    match previous {
        Some(old) if old.root != root => say!(
            "Re-linked {host}: {} (was {}).",
            root.display(),
            old.root.display()
        ),
        Some(_) => say!("{host} was already linked to {}.", root.display()),
        None => say!("Linked {host} → {}.", root.display()),
    }
    let config = web_config(&store)?;
    if let Some(site) = resolved(&store)?.sites.iter().find(|s| s.host == host) {
        say!("Detected: {}.", describe(site));
        say!("URL: {}", config.site_url(&spec(site, &BTreeMap::new())));
    }
    if secure {
        trust_hint()?;
    }
    hosts_hint(host.as_str());
    crate::coexist::note_shared_names(&store, &[host.to_string()]);
    apply_site_change(&store)
}

/// The folder to park or unpark: `path`, or the current folder.
fn folder_arg(path: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = match path {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let full =
        std::fs::canonicalize(&path).map_err(|e| format!("cannot use {}: {e}", path.display()))?;
    if !full.is_dir() {
        return Err(format!("{} is not a folder", full.display()));
    }
    Ok(strip_verbatim(full))
}

/// `kiln park [folder]`: every subfolder becomes `<name>.<tld>` ([ADR 0015] §6).
///
/// [ADR 0015]: ../../../docs/adr/0015-parked-folders-and-drivers.md
pub fn park(path: Option<PathBuf>) -> Result<(), String> {
    let folder = folder_arg(path)?;
    let mut store = open_store()?;
    if store.park(&folder).map_err(|e| e.to_string())? {
        say!(
            "Parked {}: each subfolder is served as <name>.{}.",
            folder.display(),
            kiln_runtime::tld(&store).map_err(|e| e.to_string())?
        );
    } else {
        say!("{} is already parked.", folder.display());
    }
    let all = resolved(&store)?;
    let here: Vec<&kiln_sites::Site> = all
        .sites
        .iter()
        .filter(|s| s.source == kiln_sites::Source::Parked(folder.clone()))
        .collect();
    for site in &here {
        say!("  {:<24} {}", site.host.as_str(), describe(site));
    }
    if here.is_empty() {
        say!("  (no subfolders yet)");
    }
    for skipped in all
        .skipped
        .iter()
        .filter(|s| s.folder.parent() == Some(folder.as_path()))
    {
        say!("  skipped {}: {}", skipped.folder.display(), skipped.reason);
    }
    let missing = here
        .iter()
        .filter(|s| !hosts_names().iter().any(|n| n == s.host.as_str()))
        .count();
    if missing > 0 {
        say!("{missing} of them aren't in the hosts file yet; run `kiln hosts sync`.");
    }
    let names: Vec<String> = here.iter().map(|s| s.host.to_string()).collect();
    crate::coexist::note_shared_names(&store, &names);
    apply_site_change(&store)
}

/// `kiln unpark [folder]`.
pub fn unpark(path: Option<PathBuf>) -> Result<(), String> {
    let folder = folder_arg(path)?;
    let mut store = open_store()?;
    if !store.unpark(&folder).map_err(|e| e.to_string())? {
        return Err(format!(
            "{} isn't parked (see `kiln sites`)",
            folder.display()
        ));
    }
    say!(
        "Unparked {}. Its folders are untouched; `kiln hosts sync` removes their names.",
        folder.display()
    );
    apply_site_change(&store)
}

/// `canonicalize` on Windows returns `\\?\C:\...`; Caddy and humans want `C:\...`.
pub(crate) fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

pub fn unlink(name: &str) -> Result<(), String> {
    let host = host_name(name)?;
    let mut store = open_store()?;
    if !store
        .remove_site(host.as_str())
        .map_err(|e| e.to_string())?
    {
        if let Some(site) = resolved(&store)?.sites.iter().find(|s| s.host == host)
            && let kiln_sites::Source::Parked(dir) = &site.source
        {
            return Err(format!(
                "{host} comes from the parked folder {}; unpark it or rename the subfolder",
                dir.display()
            ));
        }
        return Err(format!("{host} isn't linked"));
    }
    say!("Unlinked {host}. Its files are untouched.");
    // A secured parked site was pinned as a link (ADR 0015 §8); it falls back to parked.
    if let Some(site) = resolved(&store)?.sites.iter().find(|s| s.host == host) {
        say!(
            "It's still served from its parked folder, over plain HTTP: {}.",
            describe(site)
        );
    } else if hosts_names().iter().any(|n| n == host.as_str()) {
        say!("It's still in the hosts file; `kiln hosts sync` removes it.");
    }
    apply_site_change(&store)
}

pub fn set_secure(name: &str, secure: bool) -> Result<(), String> {
    let host = host_name(name)?;
    let mut store = open_store()?;
    if secure {
        ca_files()?;
    }
    let is_proxy = store
        .set_proxy_secure(host.as_str(), secure)
        .map_err(|e| e.to_string())?;
    if !is_proxy
        && !store
            .set_site_secure(host.as_str(), secure)
            .map_err(|e| e.to_string())?
    {
        // A parked site has no link row to carry the flag: securing pins it as one
        // (ADR 0015 §8); parked sites are already plain HTTP.
        let parked = resolved(&store)?
            .sites
            .into_iter()
            .find(|s| s.host == host && matches!(s.source, kiln_sites::Source::Parked(_)));
        match parked {
            Some(site) if secure => {
                store
                    .put_site(&SiteRecord {
                        host: host.to_string(),
                        root: site.project.clone(),
                        secure: true,
                    })
                    .map_err(|e| e.to_string())?;
                say!(
                    "{host} comes from a parked folder; pinned it as a link to {} so it can keep HTTPS.",
                    site.project.display()
                );
            }
            Some(_) => {
                say!("{host} is a parked site and already uses plain HTTP.");
                return Ok(());
            }
            None => {
                return Err(format!(
                    "{host} isn't a site; run `kiln link` or `kiln park` first"
                ));
            }
        }
    }
    let config = web_config(&store)?;
    let url = config
        .sites
        .iter()
        .find(|s| s.host == host)
        .map(|s| config.site_url(s))
        .unwrap_or_default();
    let still_secure = config.sites.iter().any(|s| s.host == host && s.secure);
    if secure {
        say!("{host} now uses HTTPS: {url} (HTTP redirects there).");
        trust_hint()?;
    } else if still_secure {
        say!(
            "{host} keeps HTTPS: its kiln.toml has `secure = true` (ADR 0027). Change it there to serve plain HTTP."
        );
    } else {
        say!("{host} is back to plain HTTP: {url}");
    }
    apply_site_change(&store)
}

#[cfg(windows)]
pub(crate) fn trust_hint() -> Result<(), String> {
    let root = apply::ca_store()?.root().map_err(|e| e.to_string())?;
    let trusted = kiln_sys::wincrypt::CertStore::open_current_user("Root")
        .and_then(|store| store.contains(&root.sha1))
        .unwrap_or(false);
    if !trusted {
        say!("Browsers won't trust it until you run `kiln ca trust`.");
    }
    Ok(())
}

/// Linux: a hint unless every browser NSS database found already trusts the root.
#[cfg(not(windows))]
pub(crate) fn trust_hint() -> Result<(), String> {
    use kiln_platform::trust_nss::{Certutil, NssTool, nss_databases};
    let root = apply::ca_store()?.root().map_err(|e| e.to_string())?;
    let nickname = kiln_platform::trust_nss::nickname_for(&root.sha256);
    let databases = std::env::var_os("HOME")
        .map(|home| nss_databases(std::path::Path::new(&home), false))
        .unwrap_or_default();
    let trusted = Certutil::find().is_some_and(|certutil| {
        !databases.is_empty()
            && databases
                .iter()
                .all(|db| certutil.contains(&db.path, &nickname).unwrap_or(false))
    });
    if !trusted {
        say!("Browsers won't trust it until you run `kiln ca trust`.");
    }
    Ok(())
}

/// Enough of the config to print site URLs: only the ports matter. Reads settings only (never
/// the CA, which `web_config` may renew), so `kiln doctor` can use it.
fn url_config(store: &StateStore) -> WebConfig {
    WebConfig {
        http_port: crate::coexist::planned_port(store, &crate::coexist::HTTP).unwrap_or(80),
        https_port: crate::coexist::planned_port(store, &crate::coexist::HTTPS).unwrap_or(443),
        ipv6: false,
        sites: Vec::new(),
        ca: None,
        storage_dir: PathBuf::new(),
        log_file: PathBuf::new(),
        admin_socket: PathBuf::new(),
        php_pools: BTreeMap::new(),
    }
}

/// Every site's URL, by host name.
pub(crate) fn site_urls(store: &StateStore) -> BTreeMap<String, String> {
    let config = url_config(store);
    all_specs(store, &BTreeMap::new())
        .map(|all| {
            all.iter()
                .map(|site| (site.host.to_string(), config.site_url(site)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn sites() -> Result<(), String> {
    let store = open_store()?;
    let parked = store.parked().map_err(|e| e.to_string())?;
    let all = resolved(&store)?;
    let proxies = proxy_specs(&store)?;
    if crate::out::json() {
        crate::out::data(sites_data(&store, &parked, &all, &proxies)?);
        return Ok(());
    }
    for dir in &parked {
        say!("Parked: {}", dir.display());
    }
    if all.sites.is_empty() && proxies.is_empty() {
        say!(
            "No sites yet. Run `kiln link` in a project folder, or `kiln park` in a folder of projects."
        );
        return Ok(());
    }
    let config = url_config(&store);
    let in_hosts = hosts_names();
    let dns_note = |host: &HostName| {
        if in_hosts.iter().any(|n| n == host.as_str()) {
            ""
        } else {
            "  (not in hosts file)"
        }
    };
    for proxy in &proxies {
        let target = proxy
            .proxy
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        say!(
            "{:<34} {:<17} {:<17} {:<6} {target}{}",
            config.site_url(proxy),
            "dev server",
            "-",
            "proxy",
            dns_note(&proxy.host)
        );
    }
    let installed = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
    let versions =
        kiln_runtime::site_versions(&store, &all.sites, &installed).map_err(|e| e.to_string())?;
    for site in all
        .sites
        .iter()
        .filter(|s| !proxies.iter().any(|p| p.host == s.host))
    {
        let url = config.site_url(&spec(site, &BTreeMap::new()));
        // "8.4.27", "8.1.34 (pinned)", "8.3.36 (kiln.toml)", "8.4.27 (pin 8.2 missing)",
        // or "-" for static sites.
        let php = match versions.get(site.host.as_str()) {
            _ if !site.driver.uses_php() => "-".to_owned(),
            Some(v) => {
                let version = v.php.as_ref().map_or("none", |p| p.version.as_str());
                let what = if v.from_project { "kiln.toml" } else { "pin" };
                match (&v.pinned, v.fell_back) {
                    (Some(pin), true) => format!("{version} ({what} {pin} missing)"),
                    (Some(_), false) if v.from_project => format!("{version} (kiln.toml)"),
                    (Some(_), false) => format!("{version} (pinned)"),
                    (None, _) => version.to_owned(),
                }
            }
            None => "none".to_owned(),
        };
        let dns = dns_note(&site.host);
        let source = match (&site.alias_of, &site.source) {
            (Some(_), _) => "alias",
            (None, kiln_sites::Source::Link) => "link",
            (None, kiln_sites::Source::Parked(_)) => "parked",
        };
        say!(
            "{url:<34} {:<17} {php:<17} {source:<6} {}{dns}",
            site.driver.name(),
            site.document_root.display()
        );
    }
    for skipped in &all.skipped {
        say!("skipped {}: {}", skipped.folder.display(), skipped.reason);
    }
    for warning in &all.warnings {
        say!("warning: {warning}");
    }
    Ok(())
}

/// The Node that runs in a project, and why (ADR 0055 §1).
fn node_json(here: &kiln_runtime::node::DirNode) -> serde_json::Value {
    serde_json::json!({
        "version": here.node.as_ref().map(|n| n.version.clone()),
        "asked_by": here.wanted.as_ref().map(|w| w.file.clone()),
        "asks_for": here.wanted.as_ref().map(|w| w.text.clone()),
        "fell_back": here.fell_back,
    })
}

/// `kiln settings` ([ADR 0058] §2): what the app's Settings page shows and changes.
///
/// [ADR 0058]: ../../../docs/adr/0058-history-doctor-settings.md
pub fn settings() -> Result<(), String> {
    use crate::coexist::{HTTP, HTTPS, is_auto, planned_port};
    let store = open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let workers = crate::pool::workers(&store)?;
    let port = |kind| -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({
            "port": planned_port(&store, kind)?,
            "auto": is_auto(&store, kind)?,
        }))
    };
    let (http, https) = (port(&HTTP)?, port(&HTTPS)?);
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "tld": tld.as_str(),
            "http": http,
            "https": https,
            "php_workers": workers,
            "mcp_allow_changes": crate::mcp::changes_allowed(),
            "check_kiln_updates": crate::updates::check_kiln(&store),
        }));
        return Ok(());
    }
    let who = |v: &serde_json::Value| {
        if v["auto"] == true {
            "picked by Kiln"
        } else {
            "your choice"
        }
    };
    say!("Development TLD: .{}", tld.as_str());
    say!("HTTP port:       {} ({})", http["port"], who(&http));
    say!("HTTPS port:      {} ({})", https["port"], who(&https));
    say!("PHP workers:     {workers} per PHP version");
    Ok(())
}

/// The framework's version in a project (ADR 0055 §1): its package in `composer.lock`, or
/// `WordPress`' own version file.
fn framework_version(driver: kiln_sites::Driver, project: &Path) -> Option<String> {
    use kiln_sites::Driver;
    let package = match driver {
        Driver::Laravel => "laravel/framework",
        Driver::Symfony => "symfony/framework-bundle",
        Driver::Drupal => "drupal/core",
        Driver::Craft => "craftcms/cms",
        Driver::WordPressBedrock => "roots/wordpress",
        Driver::WordPress => return wordpress_version(project),
        Driver::PhpPublic | Driver::PhpWeb | Driver::Php | Driver::Static => return None,
    };
    let lock = std::fs::read_to_string(project.join("composer.lock")).ok()?;
    let lock: serde_json::Value = serde_json::from_str(&lock).ok()?;
    lock["packages"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == package)?["version"]
        .as_str()
        .map(|v| v.trim_start_matches('v').to_owned())
}

/// `$wp_version = '6.6.2';` in `wp-includes/version.php`.
fn wordpress_version(project: &Path) -> Option<String> {
    let text = std::fs::read_to_string(project.join("wp-includes").join("version.php")).ok()?;
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("$wp_version"))?;
    let start = line.find(['\'', '"'])? + 1;
    let rest = &line[start..];
    Some(rest[..rest.find(['\'', '"'])?].to_owned())
}

/// `kiln --json sites`' data (ADR 0053 §3): the same values the table is made of.
fn sites_data(
    store: &StateStore,
    parked: &[PathBuf],
    all: &kiln_sites::Resolved,
    proxies: &[SiteSpec],
) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let config = url_config(store);
    let in_hosts = hosts_names();
    let listed = |host: &str| in_hosts.iter().any(|n| n == host);
    let installed = kiln_runtime::installed(store).map_err(|e| e.to_string())?;
    let versions =
        kiln_runtime::site_versions(store, &all.sites, &installed).map_err(|e| e.to_string())?;
    let nodes = kiln_state::default_local_dir()
        .map(|local| kiln_node::installed(&local.join(kiln_runtime::node::NODE_DIR)))
        .unwrap_or_default();
    let default_node = store
        .setting(kiln_runtime::node::DEFAULT_KEY)
        .map_err(|e| e.to_string())?;
    let sites: Vec<_> = all
        .sites
        .iter()
        .filter(|s| !proxies.iter().any(|p| p.host == s.host))
        .map(|site| {
            let host = site.host.as_str();
            let php = versions
                .get(host)
                .filter(|_| site.driver.uses_php())
                .map(|v| {
                    json!({
                        "version": v.php.as_ref().map(|p| p.version.clone()),
                        "pinned": v.pinned,
                        "from_project": v.from_project,
                        "fell_back": v.fell_back,
                    })
                });
            let (source, folder) = match (&site.alias_of, &site.source) {
                (Some(_), _) => ("alias", None),
                (None, kiln_sites::Source::Link) => ("link", None),
                (None, kiln_sites::Source::Parked(dir)) => ("parked", Some(dir)),
            };
            json!({
                "host": host,
                "url": config.site_url(&spec(site, &BTreeMap::new())),
                "driver": site.driver.name(),
                "document_root": site.document_root,
                "secure": site.secure,
                "source": source,
                "parked_in": folder,
                "alias_of": site.alias_of.as_ref().map(HostName::as_str),
                "php": php,
                "in_hosts": listed(host),
                "project": site.project,
                "framework_version": framework_version(site.driver, &site.project),
                "node": node_json(&kiln_runtime::node::node_for_dir(&nodes, default_node.as_deref(), &site.project)),
            })
        })
        .collect();
    let proxies: Vec<_> = proxies
        .iter()
        .map(|proxy| {
            json!({
                "host": proxy.host.as_str(),
                "url": config.site_url(proxy),
                "target": proxy.proxy.as_ref().map(ToString::to_string),
                "in_hosts": listed(proxy.host.as_str()),
            })
        })
        .collect();
    Ok(json!({
        "parked": parked,
        "sites": sites,
        "proxies": proxies,
        "skipped": all.skipped.iter().map(|s| json!({"folder": s.folder, "reason": s.reason.clone()})).collect::<Vec<_>>(),
        "warnings": all.warnings.iter().map(ToString::to_string).collect::<Vec<_>>(),
    }))
}

pub fn install(archive: Option<PathBuf>) -> Result<(), String> {
    let release = kiln_web::release::release_for_this_platform().ok_or_else(|| {
        format!(
            "no pinned Caddy release for {}/{} yet",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let server = server()?;
    if matches!(
        server.status().map_err(|e| e.to_string())?,
        Status::Running { .. }
    ) {
        return Err("stop the web server first (`kiln web stop`)".into());
    }
    let archive = if let Some(archive) = archive {
        archive
    } else {
        // Download the pinned archive; the fetcher refuses anything but its pinned SHA-512.
        let expected = kiln_fetch::Expected::sha512(release.sha512).map_err(|e| e.to_string())?;
        let label = format!("Downloading {}", release.archive);
        crate::php::fetcher()?
            .fetch(
                &release.download_url(),
                release.archive,
                &expected,
                &mut crate::php::progress_printer(&label),
            )
            .map_err(|e| e.to_string())?
    };
    let installed =
        kiln_web::release::install(&archive, server.exe()).map_err(|e| e.to_string())?;
    let mut store = open_store()?;
    store
        .set_setting(CADDY_VERSION_KEY, installed.version)
        .map_err(|e| e.to_string())?;
    say!(
        "Verified {} (SHA-512 matches the pinned release) and installed Caddy {} as {}.",
        installed.archive,
        installed.version,
        server.exe().display()
    );
    Ok(())
}

/// `kiln web start`: ports the user chose (`--http-port`, `--https-port`) are kept; the others
/// are picked automatically ([ADR 0019] §3).
///
/// [ADR 0019]: ../../../docs/adr/0019-coexistence.md
#[allow(clippy::similar_names)] // HTTP and HTTPS ports are the clearest names
pub fn start(
    http_port: Option<u16>,
    https_port: Option<u16>,
    auto_ports: bool,
) -> Result<(), String> {
    let mut store = open_store()?;
    let server = server()?;
    if let Status::Running { pid } = server.status().map_err(|e| e.to_string())? {
        return Err(format!("Kiln's web server is already running (PID {pid})"));
    }
    let secure = resolved(&store)?.sites.iter().any(|s| s.secure);
    let ports = crate::coexist::Listeners::now();
    for (kind, chosen, needed) in [
        (&crate::coexist::HTTP, http_port, true),
        (&crate::coexist::HTTPS, https_port, secure),
    ] {
        crate::coexist::choose_port(&mut store, kind, chosen, auto_ports, needed, &ports)?;
    }
    // PHP workers first, so Caddy never points at ports nobody listens on.
    let supervisor = crate::pool::supervisor()?;
    let pools = crate::pool::plan(&store, None)?;
    if let Some(pools) = &pools {
        supervisor.start(pools).map_err(|e| e.to_string())?;
    }
    let config = match web_config(&store) {
        Ok(config) => config,
        Err(e) => {
            let _ = supervisor.stop();
            return Err(e);
        }
    };
    // Site certificates from an earlier intermediate would be served on (I-57).
    if let Err(e) = record_issuer(&config) {
        let _ = supervisor.stop();
        return Err(e);
    }
    let pid = match server.start(&config) {
        Ok(pid) => pid,
        Err(e) => {
            let _ = supervisor.stop();
            return Err(e.to_string());
        }
    };
    // The URLs below should work at once (I-69).
    wait_for_certificates(&store)?;
    say!("Web server started (PID {pid}).");
    print_listeners(&config);
    print_php(pools.as_ref());
    for site in &config.sites {
        say!("  {}", config.site_url(site));
    }
    if config.sites.is_empty() {
        say!("No sites linked yet. Run `kiln link` in a project folder.");
    }
    Ok(())
}

fn print_php(pools: Option<&Pools>) {
    match pools.filter(|p| !p.pools.is_empty()) {
        Some(pools) => {
            for pool in &pools.pools {
                let ports: Vec<String> = pool.workers.iter().map(|w| w.port.to_string()).collect();
                match pool.kind {
                    kiln_supervisor::pools::PoolKind::Cgi => say!(
                        "PHP {}: {} php-cgi workers on 127.0.0.1:{}.",
                        pool.id,
                        pool.workers.len(),
                        ports.join(", ")
                    ),
                    kiln_supervisor::pools::PoolKind::Fpm => say!(
                        "PHP {}: PHP-FPM on 127.0.0.1:{}.",
                        pool.id,
                        ports.join(", ")
                    ),
                    // Services run under their own supervisor, never in the PHP one.
                    kiln_supervisor::pools::PoolKind::Program => {}
                }
            }
        }
        None => say!(
            "No PHP installed, so sites are served as static files. Add PHP with `kiln php install 8.4`."
        ),
    }
}

fn print_listeners(config: &WebConfig) {
    let listeners: Vec<String> = config.listeners().iter().map(ToString::to_string).collect();
    say!(
        "Listening on {} (this computer only).",
        listeners.join(", ")
    );
}

pub fn stop() -> Result<(), String> {
    match server()?.stop().map_err(|e| e.to_string())? {
        Some(pid) => say!("Web server stopped (PID {pid})."),
        None => say!("The web server wasn't running."),
    }
    if let Some(pid) = crate::pool::supervisor()?
        .stop()
        .map_err(|e| format!("cannot stop the PHP supervisor: {e}"))?
    {
        say!("PHP workers stopped (supervisor PID {pid}).");
    }
    Ok(())
}

pub fn restart() -> Result<(), String> {
    stop()?;
    start(None, None, false)
}

/// Re-applies the current sites and settings to the running server without restarting it.
pub fn reload() -> Result<(), String> {
    let store = open_store()?;
    if is_running()? && restart_if_ca_changed(&store)? {
        return Ok(());
    }
    let config = web_config(&store)?;
    server()?.reload(&config).map_err(|e| e.to_string())?;
    say!("Web server reloaded.");
    print_listeners(&config);
    Ok(())
}

pub fn status() -> Result<(), String> {
    let server = server()?;
    let store = open_store()?;
    let installed = store
        .setting(CADDY_VERSION_KEY)
        .map_err(|e| e.to_string())?;
    match (server.exe().is_file(), installed) {
        (true, Some(version)) => say!("Caddy {version}: {}", server.exe().display()),
        (true, None) => say!("Caddy: {}", server.exe().display()),
        (false, _) => say!("Caddy isn't installed; run `kiln web install`."),
    }
    match server.status().map_err(|e| e.to_string())? {
        Status::Running { pid } => {
            say!("Running (PID {pid}).");
            if let Ok(config) = web_config(&store) {
                print_listeners(&config);
            }
        }
        Status::Stale { pid } => say!(
            "Not running (PID {pid} from a previous start has exited). Start it with `kiln web start`."
        ),
        Status::Stopped => say!("Not running. Start it with `kiln web start`."),
    }
    let supervisor = crate::pool::supervisor()?;
    match supervisor.status().map_err(|e| e.to_string())? {
        kiln_sys::background::ProcStatus::Running { pid } => {
            let pools = supervisor.pools().map_err(|e| e.to_string())?;
            print_php(pools.as_ref());
            say!(
                "PHP supervisor PID {pid}; log: {}",
                supervisor.log_path().display()
            );
        }
        _ => match crate::pool::chosen_php(&store)? {
            Some(php) => say!(
                "PHP {} will serve sites when the server starts.",
                php.version
            ),
            None => print_php(None),
        },
    }
    say!("Config: {}", server.config_path().display());
    Ok(())
}

pub fn print_config() -> Result<(), String> {
    let store = open_store()?;
    let json = web_config(&store)?.render().map_err(|e| e.to_string())?;
    say!("{json}");
    Ok(())
}

pub fn hosts_sync(yes: bool) -> Result<(), String> {
    let store = open_store()?;
    // Links, parked sites and proxies alike: the hosts file has no wildcards (ADR 0015 §9).
    let names: Vec<String> = all_specs(&store, &BTreeMap::new())?
        .into_iter()
        .map(|s| s.host.to_string())
        .collect();
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    drop(store);
    let (tld, entries) = apply::site_entries(&names, tld)?;
    apply::hosts_set(tld, entries, yes)?;
    // New parked folders are served from now on, without a separate reload.
    if is_running()? {
        apply_site_change(&open_store()?)?;
    }
    Ok(())
}

/// Reloads the running server with these PHP pools instead of the running ones, for a pool
/// switch ([ADR 0014] §11, [ADR 0016] §7).
///
/// [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md
/// [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md
pub fn reload_with_pools(
    pools: &BTreeMap<String, Vec<std::net::SocketAddr>>,
) -> Result<(), String> {
    let store = open_store()?;
    let mut config = web_config(&store)?;
    config.php_pools = pools.clone();
    server()?.reload(&config).map_err(|e| e.to_string())
}

/// Whether Caddy is running.
pub fn is_running() -> Result<bool, String> {
    Ok(matches!(
        server()?.status().map_err(|e| e.to_string())?,
        Status::Running { .. }
    ))
}

#[cfg(test)]
mod framework_tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn the_frameworks_version_comes_from_composer_lock() {
        let dir = project(&[(
            "composer.lock",
            r#"{"packages":[{"name":"laravel/reverb","version":"v1.6.0"},{"name":"laravel/framework","version":"v12.38.1"}]}"#,
        )]);
        assert_eq!(
            framework_version(kiln_sites::Driver::Laravel, dir.path()).as_deref(),
            Some("12.38.1")
        );
        assert_eq!(
            framework_version(kiln_sites::Driver::Symfony, dir.path()),
            None
        );
        assert_eq!(
            framework_version(kiln_sites::Driver::Static, dir.path()),
            None
        );
    }

    #[test]
    fn wordpress_says_its_version_in_its_own_file() {
        let dir = project(&[(
            "wp-includes/version.php",
            "<?php\n/** The WordPress version string. */\n$wp_version = '6.8.3';\n",
        )]);
        assert_eq!(
            framework_version(kiln_sites::Driver::WordPress, dir.path()).as_deref(),
            Some("6.8.3")
        );
    }
}
