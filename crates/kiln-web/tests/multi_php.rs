//! Two PHP versions side by side behind one Caddy ([ADR 0016] §5–6): each site reaches the
//! pool of its own version.
//!
//! Needs `KILN_TEST_CADDY`, `KILN_TEST_PHP` and `KILN_TEST_PHP2` (two PHP folders of different
//! versions). Without them the test only prints a note.
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kiln_core::{HostName, Tld};
use kiln_supervisor::launcher::PhpCgiLauncher;
use kiln_supervisor::pools::free_ports;
use kiln_supervisor::{PoolSpec, Pools};
use kiln_web::{SiteSpec, WebConfig, WebServer};

fn env_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    assert!(path.exists(), "{name}={} doesn't exist", path.display());
    Some(path)
}

fn body(port: u16, host: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    let text = String::from_utf8_lossy(&response).into_owned();
    text.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned()
}

#[test]
fn each_site_runs_on_its_own_php_version() {
    let (Some(caddy), Some(newer), Some(older)) = (
        env_path("KILN_TEST_CADDY"),
        env_path("KILN_TEST_PHP"),
        env_path("KILN_TEST_PHP2"),
    ) else {
        eprintln!("skipped: set KILN_TEST_CADDY, KILN_TEST_PHP and KILN_TEST_PHP2");
        return;
    };
    let version = |p: &PathBuf| p.file_name().unwrap().to_string_lossy().into_owned();
    let dir = tempfile::tempdir().unwrap();
    for name in ["modern", "legacy"] {
        let root = dir.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.php"), "<?php echo PHP_VERSION;").unwrap();
    }

    let pool = |php: &PathBuf| {
        PoolSpec::new(
            version(php),
            php.join("php-cgi.exe"),
            php.join("php.ini"),
            &free_ports(2).unwrap(),
            500,
        )
    };
    let pools = Pools {
        pools: vec![pool(&newer), pool(&older)],
    };
    let run_dir = dir.path().join("supervisor");
    std::fs::create_dir_all(&run_dir).unwrap();
    pools.write(&run_dir.join("pools.json")).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let supervisor = {
        let (stop, run_dir) = (Arc::clone(&stop), run_dir.clone());
        std::thread::spawn(move || {
            let launcher = PhpCgiLauncher::new(&run_dir.join("php-errors.log")).unwrap();
            kiln_supervisor::run::run(
                &run_dir.join("pools.json"),
                &run_dir.join("supervisor.log"),
                launcher,
                &stop,
            )
            .unwrap();
        })
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    while !pools
        .upstreams()
        .iter()
        .all(|a| TcpStream::connect_timeout(a, Duration::from_millis(200)).is_ok())
    {
        assert!(Instant::now() < deadline, "workers didn't start");
        std::thread::sleep(Duration::from_millis(100));
    }

    let exe = dir.path().join("bin").join(kiln_web::EXE_NAME);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::copy(&caddy, &exe).unwrap();
    let server = WebServer::new(exe, dir.path().join("caddy"));
    let http_port = free_ports(1).unwrap()[0];
    let site = |name: &str, php: &PathBuf| SiteSpec {
        host: HostName::new(&format!("{name}.test"), &Tld::default()).unwrap(),
        root: dir.path().join(name),
        secure: false,
        php_pool: Some(version(php)),
        proxy: None,
    };
    let config = WebConfig {
        http_port,
        https_port: free_ports(1).unwrap()[0],
        ipv6: false,
        sites: vec![site("modern", &newer), site("legacy", &older)],
        ca: None,
        storage_dir: dir.path().join("caddy").join("storage"),
        log_file: server.log_file(),
        admin_socket: server.admin_socket(),
        php_pools: pools
            .pools
            .iter()
            .map(|p| (p.id.clone(), p.upstreams()))
            .collect::<BTreeMap<_, _>>(),
    };
    server.start(&config).unwrap();
    let modern = body(http_port, "modern.test");
    let legacy = body(http_port, "legacy.test");
    server.stop().unwrap();
    stop.store(true, Ordering::Relaxed);
    supervisor.join().unwrap();

    assert_eq!(modern, version(&newer));
    assert_eq!(legacy, version(&older));
}
