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

/// `blog` or `blog.test` → `blog.test`, validated.
pub(crate) fn host_name(name: &str) -> Result<HostName, String> {
    let tld = Tld::default();
    let fqdn = if name.ends_with(&format!(".{tld}")) {
        name.to_ascii_lowercase()
    } else {
        format!("{}.{tld}", name.to_ascii_lowercase())
    };
    HostName::new(&fqdn, &tld).map_err(|e| e.to_string())
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
fn ca_files() -> Result<CaFiles, String> {
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
            .issue_intermediate(&Tld::default())
            .map_err(|e| e.to_string())?;
        println!(
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

fn web_config(store: &StateStore) -> Result<WebConfig, String> {
    let run = server()?;
    let run_dir = run
        .config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let site_pools = crate::pool::needs(store)?.site_pools;
    let sites: Vec<SiteSpec> = resolved(store)?
        .sites
        .iter()
        .map(|site| spec(site, &site_pools))
        .collect();
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

/// The web server's view of a site; `site_pools` maps PHP sites to their pool (ADR 0016 §6).
fn spec(site: &kiln_sites::Site, site_pools: &BTreeMap<String, String>) -> SiteSpec {
    SiteSpec {
        host: site.host.clone(),
        root: site.document_root.clone(),
        secure: site.secure,
        php_pool: site_pools.get(site.host.as_str()).cloned(),
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
        println!("The web server isn't running; it takes effect at `kiln web start`.");
        return Ok(());
    }
    let saved =
        |e: String| format!("the change is saved, but the running server didn't take it: {e}");
    let running = crate::pool::running()?;
    let planned = crate::pool::plan(store, running.as_ref())?;
    match (&running, &planned) {
        (Some(old), Some(new)) if old != new => {
            crate::pool::switch(new, reload_with_pools).map_err(saved)?;
            println!("Web server and PHP pools updated without a restart.");
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
    println!("Web server reloaded with the change.");
    Ok(())
}

fn hosts_hint(host: &str) {
    if !hosts_names().iter().any(|n| n == host) {
        println!("Note: {host} isn't in the hosts file yet; run `kiln hosts sync`.");
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
    let name = match name {
        Some(name) => name,
        None => root
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("can't derive a site name from the folder; pass one")?
            .to_owned(),
    };
    let host = host_name(&name)?;
    let mut store = open_store()?;
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
        Some(old) if old.root != root => println!(
            "Re-linked {host}: {} (was {}).",
            root.display(),
            old.root.display()
        ),
        Some(_) => println!("{host} was already linked to {}.", root.display()),
        None => println!("Linked {host} → {}.", root.display()),
    }
    let config = web_config(&store)?;
    if let Some(site) = resolved(&store)?.sites.iter().find(|s| s.host == host) {
        println!("Detected: {}.", describe(site));
        println!("URL: {}", config.site_url(&spec(site, &BTreeMap::new())));
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

/// `kiln park [folder]`: every subfolder becomes `<name>.test` ([ADR 0015] §6).
///
/// [ADR 0015]: ../../../docs/adr/0015-parked-folders-and-drivers.md
pub fn park(path: Option<PathBuf>) -> Result<(), String> {
    let folder = folder_arg(path)?;
    let mut store = open_store()?;
    if store.park(&folder).map_err(|e| e.to_string())? {
        println!(
            "Parked {}: each subfolder is served as <name>.test.",
            folder.display()
        );
    } else {
        println!("{} is already parked.", folder.display());
    }
    let all = resolved(&store)?;
    let here: Vec<&kiln_sites::Site> = all
        .sites
        .iter()
        .filter(|s| s.source == kiln_sites::Source::Parked(folder.clone()))
        .collect();
    for site in &here {
        println!("  {:<24} {}", site.host.as_str(), describe(site));
    }
    if here.is_empty() {
        println!("  (no subfolders yet)");
    }
    for skipped in all
        .skipped
        .iter()
        .filter(|s| s.folder.parent() == Some(folder.as_path()))
    {
        println!("  skipped {}: {}", skipped.folder.display(), skipped.reason);
    }
    let missing = here
        .iter()
        .filter(|s| !hosts_names().iter().any(|n| n == s.host.as_str()))
        .count();
    if missing > 0 {
        println!("{missing} of them aren't in the hosts file yet; run `kiln hosts sync`.");
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
    println!(
        "Unparked {}. Its folders are untouched; `kiln hosts sync` removes their names.",
        folder.display()
    );
    apply_site_change(&store)
}

/// `canonicalize` on Windows returns `\\?\C:\...`; Caddy and humans want `C:\...`.
fn strip_verbatim(path: PathBuf) -> PathBuf {
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
    println!("Unlinked {host}. Its files are untouched.");
    // A secured parked site was pinned as a link (ADR 0015 §8); it falls back to parked.
    if let Some(site) = resolved(&store)?.sites.iter().find(|s| s.host == host) {
        println!(
            "It's still served from its parked folder, over plain HTTP: {}.",
            describe(site)
        );
    } else if hosts_names().iter().any(|n| n == host.as_str()) {
        println!("It's still in the hosts file; `kiln hosts sync` removes it.");
    }
    apply_site_change(&store)
}

pub fn set_secure(name: &str, secure: bool) -> Result<(), String> {
    let host = host_name(name)?;
    let mut store = open_store()?;
    if secure {
        ca_files()?;
    }
    if !store
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
                println!(
                    "{host} comes from a parked folder; pinned it as a link to {} so it can keep HTTPS.",
                    site.project.display()
                );
            }
            Some(_) => {
                println!("{host} is a parked site and already uses plain HTTP.");
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
    if secure {
        println!("{host} now uses HTTPS: {url} (HTTP redirects there).");
        trust_hint()?;
    } else {
        println!("{host} is back to plain HTTP: {url}");
    }
    apply_site_change(&store)
}

#[cfg(windows)]
fn trust_hint() -> Result<(), String> {
    let root = apply::ca_store()?.root().map_err(|e| e.to_string())?;
    let trusted = kiln_sys::wincrypt::CertStore::open_current_user("Root")
        .and_then(|store| store.contains(&root.sha1))
        .unwrap_or(false);
    if !trusted {
        println!("Browsers won't trust it until you run `kiln ca trust`.");
    }
    Ok(())
}

/// Linux: a hint unless every browser NSS database found already trusts the root.
#[cfg(not(windows))]
fn trust_hint() -> Result<(), String> {
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
        println!("Browsers won't trust it until you run `kiln ca trust`.");
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
    resolved(store)
        .map(|all| {
            all.sites
                .iter()
                .map(|site| {
                    (
                        site.host.to_string(),
                        config.site_url(&spec(site, &BTreeMap::new())),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn sites() -> Result<(), String> {
    let store = open_store()?;
    let parked = store.parked().map_err(|e| e.to_string())?;
    for dir in &parked {
        println!("Parked: {}", dir.display());
    }
    let all = resolved(&store)?;
    if all.sites.is_empty() {
        println!(
            "No sites yet. Run `kiln link` in a project folder, or `kiln park` in a folder of projects."
        );
        return Ok(());
    }
    let config = url_config(&store);
    let in_hosts = hosts_names();
    let installed = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
    let versions =
        kiln_runtime::site_versions(&store, &all.sites, &installed).map_err(|e| e.to_string())?;
    for site in &all.sites {
        let url = config.site_url(&spec(site, &BTreeMap::new()));
        // "8.4.27", "8.1.34 (pinned)", "8.4.27 (pin 8.2 missing)", or "-" for static sites.
        let php = match versions.get(site.host.as_str()) {
            _ if !site.driver.uses_php() => "-".to_owned(),
            Some(v) => {
                let version = v.php.as_ref().map_or("none", |p| p.version.as_str());
                match (&v.pinned, v.fell_back) {
                    (Some(pin), true) => format!("{version} (pin {pin} missing)"),
                    (Some(_), false) => format!("{version} (pinned)"),
                    (None, _) => version.to_owned(),
                }
            }
            None => "none".to_owned(),
        };
        let dns = if in_hosts.iter().any(|n| n == site.host.as_str()) {
            ""
        } else {
            "  (not in hosts file)"
        };
        let source = match &site.source {
            kiln_sites::Source::Link => "link",
            kiln_sites::Source::Parked(_) => "parked",
        };
        println!(
            "{url:<34} {:<17} {php:<17} {source:<6} {}{dns}",
            site.driver.name(),
            site.document_root.display()
        );
    }
    for skipped in &all.skipped {
        println!("skipped {}: {}", skipped.folder.display(), skipped.reason);
    }
    Ok(())
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
    println!(
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
    let pid = match server.start(&config) {
        Ok(pid) => pid,
        Err(e) => {
            let _ = supervisor.stop();
            return Err(e.to_string());
        }
    };
    println!("Web server started (PID {pid}).");
    print_listeners(&config);
    print_php(pools.as_ref());
    for site in &config.sites {
        println!("  {}", config.site_url(site));
    }
    if config.sites.is_empty() {
        println!("No sites linked yet. Run `kiln link` in a project folder.");
    }
    Ok(())
}

fn print_php(pools: Option<&Pools>) {
    match pools.filter(|p| !p.pools.is_empty()) {
        Some(pools) => {
            for pool in &pools.pools {
                let ports: Vec<String> = pool.workers.iter().map(|w| w.port.to_string()).collect();
                println!(
                    "PHP {}: {} php-cgi workers on 127.0.0.1:{}.",
                    pool.id,
                    pool.workers.len(),
                    ports.join(", ")
                );
            }
        }
        None if cfg!(windows) => println!(
            "No PHP installed, so sites are served as static files. Add PHP with `kiln php install 8.4`."
        ),
        None => {
            println!("PHP for sites comes to this platform with P1-04; sites are static for now.");
        }
    }
}

fn print_listeners(config: &WebConfig) {
    let listeners: Vec<String> = config.listeners().iter().map(ToString::to_string).collect();
    println!(
        "Listening on {} (this computer only).",
        listeners.join(", ")
    );
}

pub fn stop() -> Result<(), String> {
    match server()?.stop().map_err(|e| e.to_string())? {
        Some(pid) => println!("Web server stopped (PID {pid})."),
        None => println!("The web server wasn't running."),
    }
    if let Some(pid) = crate::pool::supervisor()?
        .stop()
        .map_err(|e| format!("cannot stop the PHP supervisor: {e}"))?
    {
        println!("PHP workers stopped (supervisor PID {pid}).");
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
    let config = web_config(&store)?;
    server()?.reload(&config).map_err(|e| e.to_string())?;
    println!("Web server reloaded.");
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
        (true, Some(version)) => println!("Caddy {version}: {}", server.exe().display()),
        (true, None) => println!("Caddy: {}", server.exe().display()),
        (false, _) => println!("Caddy isn't installed; run `kiln web install`."),
    }
    match server.status().map_err(|e| e.to_string())? {
        Status::Running { pid } => {
            println!("Running (PID {pid}).");
            if let Ok(config) = web_config(&store) {
                print_listeners(&config);
            }
        }
        Status::Stale { pid } => println!(
            "Not running (PID {pid} from a previous start has exited). Start it with `kiln web start`."
        ),
        Status::Stopped => println!("Not running. Start it with `kiln web start`."),
    }
    let supervisor = crate::pool::supervisor()?;
    match supervisor.status().map_err(|e| e.to_string())? {
        kiln_sys::background::ProcStatus::Running { pid } => {
            let pools = supervisor.pools().map_err(|e| e.to_string())?;
            print_php(pools.as_ref());
            println!(
                "PHP supervisor PID {pid}; log: {}",
                supervisor.log_path().display()
            );
        }
        _ => match crate::pool::chosen_php(&store)? {
            Some(php) => println!(
                "PHP {} will serve sites when the server starts.",
                php.version
            ),
            None => print_php(None),
        },
    }
    println!("Config: {}", server.config_path().display());
    Ok(())
}

pub fn print_config() -> Result<(), String> {
    let store = open_store()?;
    let json = web_config(&store)?.render().map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

pub fn hosts_sync(yes: bool) -> Result<(), String> {
    let store = open_store()?;
    // Links and parked sites alike: the hosts file has no wildcards (ADR 0015 §9).
    let names: Vec<String> = resolved(&store)?
        .sites
        .into_iter()
        .map(|s| s.host.to_string())
        .collect();
    drop(store);
    let tld = Tld::default();
    let (tld, entries) = apply::site_entries(&names, tld.as_str())?;
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
