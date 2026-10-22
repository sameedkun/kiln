//! The PHP worker pools behind the web server ([ADR 0014], [ADR 0016]): one pool per PHP
//! version that sites use, the supervisor running them, and changing them without downtime.
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md

use std::collections::BTreeMap;
use std::net::SocketAddr;

use kiln_state::{PhpRecord, StateStore};
use kiln_supervisor::pools::{DEFAULT_MAX_REQUESTS, DEFAULT_WORKERS, MAX_WORKERS, free_ports};
use kiln_supervisor::{PoolSpec, Pools, SupervisorHandle};
use kiln_sys::background::ProcStatus;

pub use kiln_runtime::DEFAULT_KEY;
pub const WORKERS_KEY: &str = "php.workers";

pub fn supervisor() -> Result<SupervisorHandle, String> {
    let local =
        kiln_state::default_local_dir().ok_or("cannot determine Kiln's local data directory")?;
    let exe = kiln_supervisor::handle::default_exe().map_err(|e| e.to_string())?;
    Ok(SupervisorHandle::new(exe, local.join("supervisor")))
}

/// The default PHP: `php.default` if it is installed, else the newest installed.
pub fn chosen_php(store: &StateStore) -> Result<Option<PhpRecord>, String> {
    let installed = kiln_runtime::installed(store).map_err(|e| e.to_string())?;
    kiln_runtime::default_php(store, &installed).map_err(|e| e.to_string())
}

pub fn workers(store: &StateStore) -> Result<usize, String> {
    match store.setting(WORKERS_KEY).map_err(|e| e.to_string())? {
        None => Ok(DEFAULT_WORKERS),
        Some(text) => text
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=MAX_WORKERS).contains(n))
            .ok_or_else(|| {
                format!("setting {WORKERS_KEY} holds {text:?}; expected 1 to {MAX_WORKERS}")
            }),
    }
}

/// What PHP needs to run: the build per pool id (its version), and each PHP site's pool.
#[derive(Debug, Default)]
pub struct Needs {
    pub versions: BTreeMap<String, PhpRecord>,
    pub site_pools: BTreeMap<String, String>,
}

/// One pool per version that a PHP site uses ([ADR 0016] §5). With no PHP sites at all, the
/// default version, so new sites work at once.
pub fn needs(store: &StateStore) -> Result<Needs, String> {
    let resolved = kiln_runtime::sites(store).map_err(|e| e.to_string())?;
    let installed = kiln_runtime::installed(store).map_err(|e| e.to_string())?;
    let versions = kiln_runtime::site_versions(store, &resolved.sites, &installed)
        .map_err(|e| e.to_string())?;
    let mut needs = Needs::default();
    for site in resolved.sites.iter().filter(|s| s.driver.uses_php()) {
        if let Some(php) = versions.get(site.host.as_str()).and_then(|v| v.php.clone()) {
            needs
                .site_pools
                .insert(site.host.to_string(), php.version.clone());
            needs.versions.insert(php.version.clone(), php);
        }
    }
    if needs.versions.is_empty()
        && let Some(php) =
            kiln_runtime::default_php(store, &installed).map_err(|e| e.to_string())?
    {
        needs.versions.insert(php.version.clone(), php);
    }
    Ok(needs)
}

/// A fresh pool for `php` on free ports.
pub fn pool_for(php: &PhpRecord, workers: usize) -> Result<PoolSpec, String> {
    let ports = free_ports(workers).map_err(|e| format!("cannot find free ports: {e}"))?;
    Ok(PoolSpec::new(
        php.version.clone(),
        php.dir.join(kiln_php::PHP_CGI_EXE),
        php.dir.join("php.ini"),
        &ports,
        DEFAULT_MAX_REQUESTS,
    ))
}

/// The pools to run, or `None` when there's no PHP (sites stay static). A pool already in
/// `running` with the right build and worker count is kept as is (same ports), so its sites
/// aren't touched ([ADR 0016] §7).
pub fn plan(store: &StateStore, running: Option<&Pools>) -> Result<Option<Pools>, String> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let needs = needs(store)?;
    if needs.versions.is_empty() {
        return Ok(None);
    }
    let workers = workers(store)?;
    let mut pools = Vec::new();
    for php in needs.versions.values() {
        let reusable = running.and_then(|r| {
            r.pools.iter().find(|p| {
                p.id == php.version
                    && p.php_cgi == php.dir.join(kiln_php::PHP_CGI_EXE)
                    && p.workers.len() == workers
            })
        });
        pools.push(match reusable {
            Some(pool) => pool.clone(),
            None => pool_for(php, workers)?,
        });
    }
    Ok(Some(Pools { pools }))
}

/// The pools a running supervisor serves (`None` if it isn't running).
pub fn running() -> Result<Option<Pools>, String> {
    let supervisor = supervisor()?;
    if !matches!(
        supervisor.status().map_err(|e| e.to_string())?,
        ProcStatus::Running { .. }
    ) {
        return Ok(None);
    }
    supervisor.pools().map_err(|e| e.to_string())
}

/// Pool id → upstreams, for Caddy.
#[must_use]
pub fn upstreams(pools: &Pools) -> BTreeMap<String, Vec<SocketAddr>> {
    pools
        .pools
        .iter()
        .map(|p| (p.id.clone(), p.upstreams()))
        .collect()
}

/// Upstreams of the running pools.
pub fn running_upstreams() -> Result<BTreeMap<String, Vec<SocketAddr>>, String> {
    Ok(running()?.map(|p| upstreams(&p)).unwrap_or_default())
}

/// Moves a running server to `new` without downtime ([ADR 0014] §11, [ADR 0016] §7): start
/// the new pools beside the old ones, point Caddy at `new`, let Caddy's previous config drain,
/// then retire pools that are no longer used. Pools present in both are left alone.
pub fn switch(
    new: &Pools,
    reload_caddy: impl FnOnce(&BTreeMap<String, Vec<SocketAddr>>) -> Result<(), String>,
) -> Result<(), String> {
    let supervisor = supervisor()?;
    let old = running()?.unwrap_or_default();
    let mut both = old.clone();
    for pool in &new.pools {
        if !both.pools.contains(pool) {
            both.pools.push(pool.clone());
        }
    }
    supervisor.update(&both).map_err(|e| e.to_string())?;
    if let Err(e) = reload_caddy(&upstreams(new)) {
        // Caddy kept the old upstreams; keep the old pools and retire the new ones.
        let _ = supervisor.update(&old);
        return Err(e);
    }
    if old.pools.iter().any(|p| !new.pools.contains(p)) {
        // Let Caddy's previous config finish the requests it still sends to retiring pools.
        std::thread::sleep(kiln_supervisor::pools::RETIRE_GRACE);
    }
    supervisor.update(new).map_err(|e| e.to_string())
}
