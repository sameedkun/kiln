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

/// The workers for one PHP build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSpec {
    /// The PHP version, e.g. `8.4.27`.
    pub id: String,
    pub php_cgi: PathBuf,
    /// The `php.ini` to load (`-c`).
    pub ini: PathBuf,
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
    /// A pool with one worker per port and staggered recycle limits.
    #[must_use]
    pub fn new(id: String, php_cgi: PathBuf, ini: PathBuf, ports: &[u16], base: u32) -> Self {
        let workers = ports
            .iter()
            .zip(stagger(base, ports.len()))
            .map(|(&port, max_requests)| WorkerSpec { port, max_requests })
            .collect();
        Self {
            id,
            php_cgi,
            ini,
            workers,
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
    fn free_ports_are_distinct() {
        let ports = free_ports(8).unwrap();
        let mut unique = ports.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 8);
    }
}
