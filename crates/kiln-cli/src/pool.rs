//! The PHP worker pool behind the web server ([ADR 0014]): which PHP serves sites, how many
//! workers, starting the supervisor, and switching versions without downtime.
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::net::SocketAddr;

use kiln_state::{PhpRecord, StateStore};
use kiln_supervisor::pools::{DEFAULT_MAX_REQUESTS, DEFAULT_WORKERS, MAX_WORKERS, free_ports};
use kiln_supervisor::{PoolSpec, Pools, SupervisorHandle};
use kiln_sys::background::ProcStatus;

pub const DEFAULT_KEY: &str = "php.default";
pub const WORKERS_KEY: &str = "php.workers";

pub fn supervisor() -> Result<SupervisorHandle, String> {
    let local =
        kiln_state::default_local_dir().ok_or("cannot determine Kiln's local data directory")?;
    let exe = kiln_supervisor::handle::default_exe().map_err(|e| e.to_string())?;
    Ok(SupervisorHandle::new(exe, local.join("supervisor")))
}

/// The PHP that serves sites: `php.default` if it is installed, else the newest installed.
pub fn chosen_php(store: &StateStore) -> Result<Option<PhpRecord>, String> {
    let installed: Vec<PhpRecord> = store
        .php_versions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| kiln_php::is_installed(&r.dir))
        .collect();
    let wanted = store.setting(DEFAULT_KEY).map_err(|e| e.to_string())?;
    Ok(match wanted {
        Some(version) => installed
            .iter()
            .find(|r| r.version == version)
            .or(installed.last())
            .cloned(),
        None => installed.last().cloned(),
    })
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

/// The pools to start, or `None` when no PHP is installed (sites stay static).
pub fn plan(store: &StateStore) -> Result<Option<Pools>, String> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let Some(php) = chosen_php(store)? else {
        return Ok(None);
    };
    Ok(Some(Pools {
        pools: vec![pool_for(&php, workers(store)?)?],
    }))
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

/// Upstreams of the running pool, for Caddy.
pub fn running_upstreams() -> Result<Vec<SocketAddr>, String> {
    Ok(running()?.map(|p| p.upstreams()).unwrap_or_default())
}

/// Moves a running server to `new` without downtime ([ADR 0014] §11): run old and new
/// workers side by side, point Caddy at the new ones, let Caddy's previous config drain, then
/// retire the old ones.
pub fn switch(
    new: &Pools,
    reload_caddy: impl FnOnce(&[SocketAddr]) -> Result<(), String>,
) -> Result<(), String> {
    let supervisor = supervisor()?;
    let old = running()?.unwrap_or_default();
    let both = Pools {
        pools: old.pools.iter().chain(&new.pools).cloned().collect(),
    };
    supervisor.update(&both).map_err(|e| e.to_string())?;
    if let Err(e) = reload_caddy(&new.upstreams()) {
        // Caddy kept the old upstreams; keep the old workers and retire the new ones.
        let _ = supervisor.update(&old);
        return Err(e);
    }
    // Let Caddy's previous config finish the requests it still sends to the old workers.
    std::thread::sleep(kiln_supervisor::pools::RETIRE_GRACE);
    supervisor.update(new).map_err(|e| e.to_string())
}
