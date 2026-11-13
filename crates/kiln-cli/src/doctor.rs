//! `kiln doctor` ([ADR 0017]): gathers facts from this machine (read-only) and prints the
//! findings of `kiln-doctor`'s rules.
//!
//! [ADR 0017]: ../../../docs/adr/0017-kiln-doctor.md

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use kiln_doctor::{
    CertFacts, Facts, OtherToolsFacts, Owner, PhpFacts, PortFacts, ResolverFacts, SharedName,
    ShimFacts, SiteDns, SiteFacts, StateFacts, Status, WebFacts,
};
use kiln_ops::{Journal, OpState};
use kiln_state::StateStore;
use kiln_sys::background::ProcStatus;

use crate::coexist::{self, Listeners};
use crate::web;

/// Prints the report (with --json, it is the result's data); returns whether nothing failed.
pub fn run() -> bool {
    let facts = gather();
    let report = kiln_doctor::diagnose(&facts);
    if crate::out::json() {
        crate::out::data(serde_json::from_str(&report.to_json()).unwrap_or_default());
    } else {
        print!("{}", report.render());
    }
    report.worst() < Status::Fail
}

fn gather() -> Facts {
    let mut facts = Facts {
        ancestors: ancestors(),
        resolver: resolver(),
        ..Facts::default()
    };
    let listeners = Listeners::now();
    let system = coexist::system_services();
    let php_on_path = crate::apply::which_on(&std::env::var("PATH").unwrap_or_default(), "php");
    let store = match open_read_only() {
        Ok(store) => store,
        Err(e) => {
            facts.state.open_error = Some(e);
            // Other tools are worth knowing about before Kiln is first used.
            facts.others = others(None, &listeners, php_on_path.as_ref(), &system);
            facts.services = crate::service::doctor_facts(None, &listeners, &system);
            return facts;
        }
    };
    facts.state = state(&store);
    let resolved = web::resolved(&store).unwrap_or_default();
    facts.sites = sites(&resolved);
    let secure = resolved.sites.iter().any(|s| s.secure);
    facts.ports = ports(&store, secure, &listeners);
    facts.web = web_facts(&store);
    facts.php = php(&store, &resolved);
    if secure {
        facts.certificates = Some(certificates());
    }
    facts.shims = shims(php_on_path.clone());
    facts.services = crate::service::doctor_facts(Some(&store), &listeners, &system);
    facts.others = others(
        Some((&store, &resolved)),
        &listeners,
        php_on_path.as_ref(),
        &system,
    );
    facts
}

/// The database as it is: no migration, no journal import (the doctor changes nothing).
/// `kiln setup` ([ADR 0057] §1): what first-run onboarding asks about, from the same facts
/// `kiln doctor` gathers. Reads only.
///
/// [ADR 0057]: ../../../docs/adr/0057-first-run-onboarding.md
pub fn setup() -> Result<(), String> {
    let store = open_read_only()?;
    let web = web_facts(&store);
    let php: Vec<String> = store
        .php_versions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| kiln_php::is_installed(&r.dir))
        .map(|r| r.version)
        .collect();
    let default_php = store
        .setting(crate::pool::DEFAULT_KEY)
        .map_err(|e| e.to_string())?;
    let local = kiln_state::default_local_dir();
    let composer = local
        .as_ref()
        .is_some_and(|l| l.join(kiln_runtime::shim::COMPOSER_PHAR).is_file());
    let node = local
        .as_ref()
        .map(|l| kiln_node::installed(&l.join(kiln_runtime::node::NODE_DIR)).len())
        .unwrap_or_default();
    let ca = certificates();
    let mut hosts: Vec<String> = web::resolved(&store)
        .map(|all| all.sites.iter().map(|s| s.host.to_string()).collect())
        .unwrap_or_default();
    hosts.extend(
        web::proxy_specs(&store)
            .unwrap_or_default()
            .iter()
            .map(|p| p.host.to_string()),
    );
    let listed = web::hosts_names();
    let missing: Vec<&String> = hosts.iter().filter(|h| !listed.contains(h)).collect();
    let applied: Vec<String> = kiln_ops::Journal::entries(&store)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.state == OpState::Applied)
        .map(|e| e.kind)
        .collect();
    let shims_on_path = applied.iter().any(|k| {
        k == kiln_platform::user_path::USER_PATH_KIND
            || k == kiln_platform::shell_profile::SHELL_PROFILE_KIND
    });
    let login_item = crate::app::login_item_kind();
    let ready = web.installed && !php.is_empty();
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "ready": ready,
            "web": {"installed": web.installed, "running": web.running_pid.is_some()},
            "php": {"versions": php, "default": default_php},
            "composer": composer,
            "node": node,
            "ca": {"exists": ca.root_present, "trusted": ca.trusted},
            "hosts": {"sites": hosts.len(), "missing": missing},
            "shims_on_path": shims_on_path,
            "login_item": login_item,
        }));
        return Ok(());
    }
    let mark = |done: bool| if done { "done" } else { "not yet" };
    say!("Web server:        {}", mark(web.installed));
    say!(
        "PHP:               {}",
        if php.is_empty() {
            "not yet".to_owned()
        } else {
            php.join(", ")
        }
    );
    say!("Composer:          {}", mark(composer));
    say!(
        "HTTPS (Kiln's CA): {}",
        match (ca.root_present, ca.trusted) {
            (false, _) => "not yet",
            (true, Some(false)) => "made, not trusted yet",
            (true, _) => "done",
        }
    );
    say!(
        "Hosts file:        {}",
        if missing.is_empty() {
            "every site is in it".to_owned()
        } else {
            format!("{} site(s) missing (`kiln hosts sync`)", missing.len())
        }
    );
    say!("Shims on PATH:     {}", mark(shims_on_path));
    say!("Starts at sign-in: {login_item}");
    Ok(())
}

fn open_read_only() -> Result<StateStore, String> {
    let dir = kiln_state::default_state_dir().ok_or("cannot determine the state directory")?;
    let db = dir.join(kiln_state::DB_FILE);
    if !db.is_file() {
        return Err(format!(
            "{} doesn't exist yet (Kiln hasn't been used)",
            db.display()
        ));
    }
    StateStore::open(&db).map_err(|e| e.to_string())
}

fn state(store: &StateStore) -> StateFacts {
    let unfinished = store
        .entries()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| matches!(e.state, OpState::Prepared | OpState::Stuck))
        .map(|e| {
            (
                e.id,
                e.kind.clone(),
                format!("{:?}", e.state).to_lowercase(),
            )
        })
        .collect();
    StateFacts {
        open_error: None,
        schema: store.schema_version().unwrap_or(0),
        supported: kiln_state::SCHEMA_VERSION,
        unfinished,
    }
}

#[cfg(windows)]
fn ancestors() -> Vec<PathBuf> {
    let Ok(parents) = kiln_sys::win::process_parents() else {
        return Vec::new();
    };
    let mut images = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..16 {
        let Some(&parent) = parents.get(&pid) else {
            break;
        };
        if parent == 0 || parent == pid {
            break;
        }
        if let Ok(Some(process)) = kiln_sys::win::Process::open_for_query(parent)
            && let Ok(image) = process.image_path()
        {
            images.push(image);
        }
        pid = parent;
    }
    images
}

#[cfg(not(windows))]
fn ancestors() -> Vec<PathBuf> {
    Vec::new()
}

fn ports(store: &StateStore, secure: bool, listeners: &Listeners) -> Vec<PortFacts> {
    let caddy = web::server().ok().map(|s| s.exe().to_path_buf());
    let mut wanted = vec![&coexist::HTTP];
    if secure {
        wanted.push(&coexist::HTTPS);
    }
    let ipv6 = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok();
    let mut out = Vec::new();
    for kind in wanted {
        let port = coexist::planned_port(store, kind).unwrap_or(kind.preferred[0]);
        let auto = coexist::is_auto(store, kind).unwrap_or(false);
        let mut ips: Vec<IpAddr> = vec![Ipv4Addr::LOCALHOST.into()];
        if ipv6 {
            ips.push(Ipv6Addr::LOCALHOST.into());
        }
        for ip in ips {
            let addr = SocketAddr::new(ip, port);
            // A listener on the wildcard address of the same family holds the port too.
            let holder = listeners.all().iter().find(|(l, _)| {
                l.port() == port
                    && l.is_ipv4() == ip.is_ipv4()
                    && (l.ip() == ip || l.ip().is_unspecified())
            });
            out.push(PortFacts {
                addr,
                purpose: kind.purpose,
                owner: holder.map(|(_, pid)| owner(*pid, caddy.as_deref())),
                auto,
            });
        }
    }
    out
}

fn owner(pid: u32, caddy: Option<&Path>) -> Owner {
    Owner {
        pid,
        who: coexist::holder(pid).describe(),
        is_kiln: caddy.is_some_and(|exe| kiln_sys::background::is_ours(pid, exe)),
    }
}

/// Other local PHP tools ([ADR 0019] §5); `kiln` is `None` before Kiln has been used.
///
/// [ADR 0019]: ../../../docs/adr/0019-coexistence.md
fn others(
    kiln: Option<(&StateStore, &kiln_sites::Resolved)>,
    listeners: &Listeners,
    php_on_path: Option<&PathBuf>,
    system: &[kiln_coexist::SystemService],
) -> OtherToolsFacts {
    let installed = kiln_coexist::installed_here();
    let herd_installed = installed
        .iter()
        .any(|(tool, _)| *tool == kiln_coexist::Tool::Herd);

    let caddy = web::server().ok().map(|s| s.exe().to_path_buf());
    let web_ports: BTreeSet<u16> = kiln_coexist::HTTP_PORTS
        .iter()
        .chain(&kiln_coexist::HTTPS_PORTS)
        .copied()
        .collect();
    let service_ports: BTreeSet<u16> = kiln_coexist::SERVICE_PORTS.iter().map(|p| p.port).collect();
    let web_listeners = grouped(listeners, &web_ports, |pid| {
        caddy
            .as_deref()
            .is_some_and(|exe| kiln_sys::background::is_ours(pid, exe))
    });
    // Kiln's own instances on these ports are reported under Services instead.
    let service_listeners = grouped(listeners, &service_ports, crate::service::is_ours_pid);

    let shared_names = kiln
        .map(|(store, resolved)| {
            let names: Vec<String> = resolved.sites.iter().map(|s| s.host.to_string()).collect();
            let urls = web::site_urls(store);
            coexist::shared_names(&names, &coexist::hosts_claims())
                .into_iter()
                .map(|(host, claim)| SharedName {
                    kiln_url: urls.get(&host).cloned().unwrap_or_default(),
                    owner: claim.owner().to_owned(),
                    host,
                })
                .collect()
        })
        .unwrap_or_default();

    OtherToolsFacts {
        installed: installed
            .into_iter()
            .map(|(tool, dir)| (tool.name().to_owned(), dir))
            .collect(),
        listeners: web_listeners,
        shared_names,
        php_on_path: coexist::php_from_tool(php_on_path)
            .map(|(tool, php)| (tool.name().to_owned(), php)),
        herd_installed,
        service_listeners,
        system_services: system
            .iter()
            .map(|s| (s.name.clone(), s.kind.to_owned(), s.running))
            .collect(),
    }
}

/// Programs listening on `ports` (loopback or all addresses), one line per program, skipping
/// those `skip` says are Kiln's: nginx's workers share their master's sockets, so one server
/// can show up as many processes.
fn grouped(
    listeners: &Listeners,
    ports: &BTreeSet<u16>,
    skip: impl Fn(u32) -> bool,
) -> Vec<(String, Vec<SocketAddr>)> {
    let mut by_pid: BTreeMap<u32, Vec<SocketAddr>> = BTreeMap::new();
    for (addr, pid) in listeners.all() {
        let local = addr.ip().is_loopback() || addr.ip().is_unspecified();
        if local && ports.contains(&addr.port()) {
            by_pid.entry(*pid).or_default().push(*addr);
        }
    }
    let mut by_program: BTreeMap<String, (Vec<u32>, BTreeSet<SocketAddr>)> = BTreeMap::new();
    for (pid, addrs) in by_pid {
        if skip(pid) {
            continue;
        }
        let entry = by_program
            .entry(coexist::holder(pid).describe())
            .or_default();
        entry.0.push(pid);
        entry.1.extend(addrs);
    }
    by_program
        .into_iter()
        .map(|(who, (pids, addrs))| {
            let pids = match pids.as_slice() {
                [pid] => format!("PID {pid}"),
                many => format!("{} processes", many.len()),
            };
            (format!("{who}, {pids}"), addrs.into_iter().collect())
        })
        .collect()
}

fn web_facts(store: &StateStore) -> WebFacts {
    let Ok(server) = web::server() else {
        return WebFacts::default();
    };
    let running_pid = match server.status() {
        Ok(ProcStatus::Running { pid }) => Some(pid),
        _ => None,
    };
    WebFacts {
        installed: server.exe().is_file(),
        version: store.setting(web::CADDY_VERSION_KEY).ok().flatten(),
        running_pid,
        admin_answers: running_pid.map(|_| {
            kiln_web::AdminClient::new(server.admin_socket())
                .config()
                .is_ok()
        }),
    }
}

fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

fn php(store: &StateStore, resolved: &kiln_sites::Resolved) -> PhpFacts {
    let records = store.php_versions().unwrap_or_default();
    let installed: Vec<(String, bool)> = records
        .iter()
        .map(|r| (r.version.clone(), kiln_php::is_installed(&r.dir)))
        .collect();
    let complete = kiln_runtime::installed(store).unwrap_or_default();
    let default = kiln_runtime::default_php(store, &complete)
        .ok()
        .flatten()
        .map(|p| p.version);
    let fallbacks = kiln_runtime::site_versions(store, &resolved.sites, &complete)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, v)| v.fell_back)
        .map(|(host, v)| (host, v.pinned.unwrap_or_default(), v.php.map(|p| p.version)))
        .collect();
    let supervisor = crate::pool::supervisor().ok();
    let supervisor_pid = supervisor.as_ref().and_then(|s| match s.status() {
        Ok(ProcStatus::Running { pid }) => Some(pid),
        _ => None,
    });
    let pools = supervisor
        .as_ref()
        .and_then(|s| s.pools().ok().flatten())
        .map(|pools| {
            pools
                .pools
                .iter()
                .map(|p| {
                    let workers = p
                        .workers
                        .iter()
                        .map(|w| (w.port, listening(w.port)))
                        .collect();
                    (p.id.clone(), workers)
                })
                .collect()
        })
        .unwrap_or_default();
    let recent_errors = supervisor
        .map(|s| last_lines(&s.errors_path(), 3))
        .unwrap_or_default();
    PhpFacts {
        installed,
        default,
        fallbacks,
        supervisor_pid,
        pools,
        recent_errors,
    }
}

fn last_lines(path: &Path, n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

fn sites(resolved: &kiln_sites::Resolved) -> SiteFacts {
    let in_hosts = web::hosts_names();
    let sites = resolved
        .sites
        .iter()
        .map(|site| SiteDns {
            host: site.host.to_string(),
            in_hosts_block: in_hosts.iter().any(|n| n == site.host.as_str()),
            resolves_to: (site.host.as_str(), 80)
                .to_socket_addrs()
                .map(|addrs| addrs.map(|a| a.ip()).collect())
                .unwrap_or_default(),
        })
        .collect();
    let current: BTreeSet<&str> = resolved.sites.iter().map(|s| s.host.as_str()).collect();
    SiteFacts {
        sites,
        skipped: resolved
            .skipped
            .iter()
            .map(|s| (s.folder.clone(), s.reason.clone()))
            .collect(),
        project_warnings: resolved.warnings.clone(),
        stale_hosts: in_hosts
            .into_iter()
            .filter(|n| !current.contains(n.as_str()))
            .collect(),
    }
}

fn certificates() -> CertFacts {
    let Ok(ca) = crate::apply::ca_store() else {
        return CertFacts::default();
    };
    let root = ca.root().ok();
    let intermediate_days_left = ca
        .intermediate()
        .ok()
        .flatten()
        .map(|i| (i.not_after - time::OffsetDateTime::now_utc()).whole_days());
    let mut facts = CertFacts {
        root_present: root.is_some(),
        trusted: root.as_ref().and_then(trusted),
        intermediate_days_left,
        firefox_profiles: firefox_profiles(),
        key_storage: ca.root_key_description().map(str::to_owned),
        stale_site_certificates: stale_site_certificates(&ca),
        ..CertFacts::default()
    };
    if let Some(root) = &root {
        linux_trust(&mut facts, root);
    }
    facts
}

/// Whether Caddy's stored site certificates came from another intermediate than the current
/// one (I-57). Read-only.
fn stale_site_certificates(ca: &kiln_pki::CaStore) -> bool {
    let (Ok(Some(intermediate)), Ok(server)) = (ca.intermediate(), web::server()) else {
        return false;
    };
    let storage = server
        .config_path()
        .parent()
        .map(|dir| dir.join("storage"))
        .unwrap_or_default();
    !kiln_web::issued::issued_from(&storage, &intermediate.sha256)
}

/// Each browser NSS database and the system store ([ADR 0021] §8). Read-only: `certutil -L`
/// and a file check.
///
/// [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md
#[cfg(target_os = "linux")]
fn linux_trust(facts: &mut CertFacts, root: &kiln_pki::CertInfo) {
    use kiln_platform::trust_nss::{Certutil, NssTool, nickname_for, nss_databases};
    use kiln_platform::trust_system::{anchor_file_name, detect_anchors};
    let certutil = Certutil::find();
    let nickname = nickname_for(&root.sha256);
    facts.certutil = Some(certutil.is_some());
    facts.nss = std::env::var_os("HOME")
        .map(|home| nss_databases(Path::new(&home), false))
        .unwrap_or_default()
        .into_iter()
        .map(|db| {
            let trusted = certutil
                .as_ref()
                .and_then(|c| c.contains(&db.path, &nickname).ok());
            (db.label, trusted)
        })
        .collect();
    facts.system_trusted = detect_anchors(Path::new("/"))
        .map(|anchors| anchors.dir.join(anchor_file_name(&root.sha256)).is_file());
}

#[cfg(not(target_os = "linux"))]
fn linux_trust(_facts: &mut CertFacts, _root: &kiln_pki::CertInfo) {}

#[cfg(windows)]
fn trusted(root: &kiln_pki::CertInfo) -> Option<bool> {
    kiln_sys::wincrypt::CertStore::open_current_user("Root")
        .and_then(|store| store.contains(&root.sha1))
        .ok()
}

#[cfg(not(windows))]
fn trusted(_root: &kiln_pki::CertInfo) -> Option<bool> {
    None
}

fn firefox_profiles() -> usize {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return 0;
    };
    std::fs::read_dir(PathBuf::from(appdata).join(r"Mozilla\Firefox\Profiles")).map_or(
        0,
        |entries| {
            entries
                .filter_map(Result::ok)
                .filter(|e| e.path().join("prefs.js").is_file())
                .count()
        },
    )
}

fn shims(php_on_path: Option<PathBuf>) -> ShimFacts {
    use kiln_runtime::shim::Tool;
    let Some(local) = kiln_state::default_local_dir() else {
        return ShimFacts::default();
    };
    let shims_dir = local.join(kiln_runtime::shim::SHIMS_DIR);
    ShimFacts {
        installed: shims_dir.join(Tool::Php.file_name()).is_file(),
        php_on_path,
        kiln_node: !kiln_node::installed(&local.join(kiln_runtime::node::NODE_DIR)).is_empty(),
        node_shim: shims_dir.join(Tool::Node.file_name()).is_file(),
        node_on_path: crate::apply::which_on(&std::env::var("PATH").unwrap_or_default(), "node"),
        shims_dir,
    }
}

/// How this machine resolves names ([ADR 0020] §2). Linux only: Windows has no nsswitch.
///
/// [ADR 0020]: ../../../docs/adr/0020-linux-dns-and-privilege.md
#[cfg(target_os = "linux")]
#[allow(clippy::unnecessary_wraps)] // same signature as elsewhere, where it is `None`
fn resolver() -> Option<ResolverFacts> {
    let nsswitch_hosts = std::fs::read_to_string("/etc/nsswitch.conf")
        .ok()
        .and_then(|text| {
            text.lines()
                .map(|line| line.split('#').next().unwrap_or_default().trim())
                .find_map(|line| line.strip_prefix("hosts:").map(|s| s.trim().to_owned()))
        });
    let link = std::fs::read_link("/etc/resolv.conf").unwrap_or_default();
    let text = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    let resolv_conf_manager = if link.to_string_lossy().contains("systemd/resolve")
        || text.contains("nameserver 127.0.0.53")
    {
        Some("systemd-resolved".to_owned())
    } else if text.contains("Generated by NetworkManager") {
        Some("NetworkManager".to_owned())
    } else {
        None
    };
    Some(ResolverFacts {
        nsswitch_hosts,
        resolv_conf_manager,
    })
}

#[cfg(not(target_os = "linux"))]
fn resolver() -> Option<ResolverFacts> {
    None
}
