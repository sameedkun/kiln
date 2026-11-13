//! `pools.json`: the desired set of php-cgi workers ([ADR 0014] §2, §5).
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::collections::BTreeMap;
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
    /// Any other program, such as a database or Mailpit ([ADR 0036] §1): run with its own
    /// arguments, its output going to its own log, stopped gracefully.
    ///
    /// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
    Program,
}

/// How a [`PoolKind::Program`] runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramSpec {
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Its working folder (default: the program's own folder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Where its stdout and stderr go (appended).
    pub log: PathBuf,
    /// Run first when it has to stop, e.g. `mysqladmin shutdown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_command: Option<Vec<String>>,
    /// Ports it opens besides the worker's, all of which must accept connections before it
    /// counts as started.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_listens: Vec<u16>,
}

/// The workers for one PHP build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSpec {
    /// The PHP version, e.g. `8.4.27`, or a service instance's name.
    pub id: String,
    #[serde(default)]
    pub kind: PoolKind,
    /// `php-cgi.exe` for [`PoolKind::Cgi`], `php-fpm` for [`PoolKind::Fpm`], the executable
    /// for [`PoolKind::Program`].
    #[serde(alias = "php_cgi")]
    pub program: PathBuf,
    /// The `php.ini` to load (`-c`); empty for a [`PoolKind::Program`].
    #[serde(default)]
    pub ini: PathBuf,
    /// The PHP-FPM configuration (`--fpm-config`), for [`PoolKind::Fpm`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fpm_config: Option<PathBuf>,
    /// How to run it, for [`PoolKind::Program`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_spec: Option<ProgramSpec>,
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

    /// Every address that must accept connections before the pools count as started: the
    /// workers', plus the other ports programs open. A program on port 0 listens on none
    /// (a queue worker, [`PoolSpec::program`]).
    #[must_use]
    pub fn ready_addresses(&self) -> Vec<SocketAddr> {
        let also = self
            .pools
            .iter()
            .filter_map(|pool| pool.program_spec.as_ref())
            .flat_map(|spec| &spec.also_listens)
            .map(|&port| SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        self.upstreams()
            .into_iter()
            .filter(|a| a.port() != 0)
            .chain(also)
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
            program_spec: None,
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
            program_spec: None,
            workers: vec![WorkerSpec { port, max_requests }],
        }
    }

    /// A program, such as a service instance, listening on `port` (and the ports in
    /// `spec.also_listens`); port 0 for one that listens on none, such as a queue worker
    /// ([ADR 0060]). It is never recycled.
    ///
    /// [ADR 0060]: ../../../docs/adr/0060-dev-processes.md
    #[must_use]
    pub fn program(id: String, program: PathBuf, port: u16, spec: ProgramSpec) -> Self {
        Self {
            id,
            kind: PoolKind::Program,
            program,
            ini: PathBuf::new(),
            fpm_config: None,
            program_spec: Some(spec),
            workers: vec![WorkerSpec {
                port,
                max_requests: 0,
            }],
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
    fn programs_round_trip_and_wait_for_all_their_ports() {
        let spec = ProgramSpec {
            args: vec!["--smtp".into(), "127.0.0.1:1025".into()],
            log: "/k/services/mail/mail.log".into(),
            stop_command: Some(vec!["stop-it".into()]),
            also_listens: vec![8025],
            ..ProgramSpec::default()
        };
        let pools = Pools {
            pools: vec![PoolSpec::program(
                "mailpit-1".into(),
                "/k/services/bin/mailpit/1.31.6/kiln-mailpit".into(),
                1025,
                spec,
            )],
        };
        let json = serde_json::to_string(&pools).unwrap();
        assert_eq!(serde_json::from_str::<Pools>(&json).unwrap(), pools);
        assert_eq!(pools.pools[0].kind, PoolKind::Program);
        assert_eq!(
            pools.upstreams(),
            [SocketAddr::from((Ipv4Addr::LOCALHOST, 1025))]
        );
        assert_eq!(
            pools.ready_addresses(),
            [
                SocketAddr::from((Ipv4Addr::LOCALHOST, 1025)),
                SocketAddr::from((Ipv4Addr::LOCALHOST, 8025))
            ]
        );
    }

    #[test]
    fn a_program_on_port_0_has_nothing_to_wait_for() {
        let pools = Pools {
            pools: vec![PoolSpec::program(
                "queue-shop".into(),
                "/k/php/8.4/php".into(),
                0,
                ProgramSpec::default(),
            )],
        };
        assert_eq!(pools.ready_addresses(), []);
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
