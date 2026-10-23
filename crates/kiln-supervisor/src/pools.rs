//! `pools.json`: the desired set of php-cgi workers ([ADR 0014] §2, §5).
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Requests before the first worker of a pool recycles; later workers get more ([`stagger`]).
pub const DEFAULT_MAX_REQUESTS: u32 = 500;
pub const DEFAULT_WORKERS: usize = 4;
pub const MAX_WORKERS: usize = 32;

/// How long the old pool keeps running after Caddy was switched to a new one ([ADR 0014]
/// §11). Caddy's previous config drains requests already in flight, still sending them to
/// the old workers and retrying among them for up to 2 s; retiring those workers sooner
/// turned such requests into 502s.
///
/// [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md
pub const RETIRE_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Every pool Kiln wants running.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pools {
    pub pools: Vec<PoolSpec>,
}

/// How a pool's PHP serves `FastCGI`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolKind {
    /// php-cgi processes, one per port, each serving one request at a time (Windows,
    /// [ADR 0014]).
    ///
    /// [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md
    #[default]
    Cgi,
    /// One PHP-FPM master on one port, managing its own children (Linux, [ADR 0022]).
    ///
    /// [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
    Fpm,
}

/// The workers for one PHP build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSpec {
    /// The PHP version, e.g. `8.4.27`.
    pub id: String,
    #[serde(default)]
    pub kind: PoolKind,
    /// `php-cgi.exe` for [`PoolKind::Cgi`], `php-fpm` for [`PoolKind::Fpm`].
    #[serde(alias = "php_cgi")]
    pub program: PathBuf,
    /// The `php.ini` to load (`-c`).
    pub ini: PathBuf,
    /// The PHP-FPM configuration (`--fpm-config`), for [`PoolKind::Fpm`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fpm_config: Option<PathBuf>,
    pub workers: Vec<WorkerSpec>,
}

/// One php-cgi process: its loopback port and its recycle limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSpec {
    pub port: u16,
    /// `PHP_FCGI_MAX_REQUESTS`: the worker exits (and is restarted) after this many requests.
    pub max_requests: u32,
}

impl Pools {
    /// Reads `path`; `Ok(None)` if it doesn't exist.
    pub fn read(path: &Path) -> io::Result<Option<Self>> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Writes `path` atomically, so the supervisor never reads half a file.
    pub fn write(&self, path: &Path) -> io::Result<()> {
        let temp = path.with_extension("tmp");
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&temp, json)?;
        kiln_sys::rename_replace(&temp, path)
    }

    /// Every worker's address, for Caddy's upstream list.
    #[must_use]
    pub fn upstreams(&self) -> Vec<SocketAddr> {
        self.pools
            .iter()
            .flat_map(|pool| &pool.workers)
            .map(|worker| SocketAddr::from((Ipv4Addr::LOCALHOST, worker.port)))
            .collect()
    }
}

impl PoolSpec {
    /// A php-cgi pool with one worker per port and staggered recycle limits.
    #[must_use]
    pub fn new(id: String, php_cgi: PathBuf, ini: PathBuf, ports: &[u16], base: u32) -> Self {
        let workers = ports
            .iter()
            .zip(stagger(base, ports.len()))
            .map(|(&port, max_requests)| WorkerSpec { port, max_requests })
            .collect();
        Self {
            id,
            kind: PoolKind::Cgi,
            program: php_cgi,
            ini,
            fpm_config: None,
            workers,
        }
    }

    /// A PHP-FPM pool: one master listening on `port`, configured by `fpm_config` (which
    /// sets its children and their recycle limit, `max_requests`).
    #[must_use]
    pub fn fpm(
        id: String,
        php_fpm: PathBuf,
        ini: PathBuf,
        fpm_config: PathBuf,
        port: u16,
        max_requests: u32,
    ) -> Self {
        Self {
            id,
            kind: PoolKind::Fpm,
            program: php_fpm,
            ini,
            fpm_config: Some(fpm_config),
            workers: vec![WorkerSpec { port, max_requests }],
        }
    }

    #[must_use]
    pub fn upstreams(&self) -> Vec<SocketAddr> {
        self.workers
            .iter()
            .map(|w| SocketAddr::from((Ipv4Addr::LOCALHOST, w.port)))
            .collect()
    }
}

/// The `php-fpm.conf` of a Kiln FPM pool ([ADR 0022] §5): one master on `port`, up to
/// `children` children recycled after `max_requests` requests, logging to `error_log`. It
/// runs as the user, so it sets no `user`/`group`.
///
/// [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
#[must_use]
pub fn fpm_config(port: u16, children: usize, max_requests: u32, error_log: &Path) -> String {
    let children = children.clamp(1, MAX_WORKERS);
    format!(
        "; Written by Kiln; rewritten when the pool changes.\n\
         [global]\n\
         error_log = {log}\n\
         daemonize = no\n\
         \n\
         [kiln]\n\
         listen = 127.0.0.1:{port}\n\
         pm = dynamic\n\
         pm.max_children = {children}\n\
         pm.start_servers = {start}\n\
         pm.min_spare_servers = 1\n\
         pm.max_spare_servers = {children}\n\
         pm.max_requests = {max_requests}\n\
         clear_env = no\n\
         catch_workers_output = yes\n",
        log = error_log.display(),
        start = children.min(2),
    )
}

/// Recycle limits for `n` workers: `base + i × base / n`, so they never all exit together
/// ([I-14]). `base` 0 means "never recycle" for all.
///
/// [I-14]: ../../../docs/issues.md#i-14
#[must_use]
pub fn stagger(base: u32, n: usize) -> Vec<u32> {
    let n32 = u32::try_from(n.max(1)).unwrap_or(u32::MAX);
    (0..n32)
        .map(|i| {
            if base == 0 {
                0
            } else {
                base.saturating_add(i.saturating_mul(base) / n32)
            }
        })
        .collect()
}

/// `n` distinct loopback ports that are free right now.
pub fn free_ports(n: usize) -> io::Result<Vec<u16>> {
    // Hold every listener until all are chosen, so no port is picked twice.
    let listeners = (0..n)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
        .collect::<io::Result<Vec<_>>>()?;
    listeners
        .iter()
        .map(|l| l.local_addr().map(|a| a.port()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_staggered_and_distinct() {
        assert_eq!(stagger(500, 4), [500, 625, 750, 875]);
        assert_eq!(stagger(500, 1), [500]);
        assert_eq!(stagger(0, 3), [0, 0, 0]);
        let many = stagger(500, 32);
        let mut sorted = many.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), 32, "no two workers share a limit");
    }

    #[test]
    fn pools_round_trip_and_list_upstreams() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pools.json");
        assert_eq!(Pools::read(&path).unwrap(), None);
        let pools = Pools {
            pools: vec![PoolSpec::new(
                "8.4.27".into(),
                "C:/php/php-cgi.exe".into(),
                "C:/php/php.ini".into(),
                &[9101, 9102],
                500,
            )],
        };
        pools.write(&path).unwrap();
        assert_eq!(Pools::read(&path).unwrap(), Some(pools.clone()));
        assert_eq!(
            pools.upstreams(),
            [
                SocketAddr::from((Ipv4Addr::LOCALHOST, 9101)),
                SocketAddr::from((Ipv4Addr::LOCALHOST, 9102))
            ]
        );
        assert_eq!(pools.pools[0].workers[1].max_requests, 750);
    }

    #[test]
    fn fpm_pools_listen_once_and_recycle_their_children() {
        let conf = fpm_config(9200, 4, 500, Path::new("/k/supervisor/php-errors.log"));
        assert!(conf.contains("listen = 127.0.0.1:9200\n"));
        assert!(conf.contains("pm.max_children = 4\n"));
        assert!(conf.contains("pm.max_requests = 500\n"));
        assert!(conf.contains("daemonize = no\n"));
        assert!(!conf.contains("user ="), "runs as the user");
        assert!(fpm_config(9200, 0, 500, Path::new("l")).contains("pm.max_children = 1\n"));

        let pool = PoolSpec::fpm(
            "8.4.23".into(),
            "/k/php/8.4.23/php-fpm".into(),
            "/k/php/8.4.23/php.ini".into(),
            "/k/supervisor/php-fpm-8.4.23.conf".into(),
            9200,
            500,
        );
        assert_eq!(pool.kind, PoolKind::Fpm);
        assert_eq!(
            pool.upstreams(),
            [SocketAddr::from((Ipv4Addr::LOCALHOST, 9200))],
            "one address per FPM pool"
        );
        let json = serde_json::to_string(&pool).unwrap();
        assert_eq!(serde_json::from_str::<PoolSpec>(&json).unwrap(), pool);
        // pools.json from before kinds existed still reads, as php-cgi pools.
        let old: PoolSpec = serde_json::from_str(
            r#"{"id":"8.4.27","php_cgi":"php-cgi.exe","ini":"php.ini","workers":[]}"#,
        )
        .unwrap();
        assert_eq!((old.kind, old.fpm_config), (PoolKind::Cgi, None));
    }

    #[test]
    fn free_ports_are_distinct() {
        let ports = free_ports(8).unwrap();
        let mut unique = ports.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 8);
    }
}
