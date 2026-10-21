//! End-to-end PHP behind Caddy ([ADR 0014]): a real supervisor with real php-cgi workers that
//! recycle constantly under load, Caddy's PHP routes, and a live switch to a new pool.
//!
//! Needs `KILN_TEST_CADDY` (a Caddy binary) and `KILN_TEST_PHP` (a PHP folder with
//! `php-cgi.exe` and `php.ini`, e.g. one `kiln php install` created). Without them the test
//! only prints a note.
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

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

fn get(port: u16, host: &str, path: &str) -> (u16, String) {
    let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else {
        return (0, "connection refused".into());
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let _ = write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    );
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    let text = String::from_utf8_lossy(&response).into_owned();
    let code = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
    (code, body)
}

fn wait_listening(pools: &Pools) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !pools
        .upstreams()
        .iter()
        .all(|a| TcpStream::connect_timeout(a, Duration::from_millis(200)).is_ok())
    {
        assert!(Instant::now() < deadline, "php-cgi workers didn't start");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[allow(clippy::too_many_lines)] // one end-to-end story
fn serves_php_through_recycling_workers_and_switches_pools_live() {
    let (Some(caddy), Some(php)) = (env_path("KILN_TEST_CADDY"), env_path("KILN_TEST_PHP")) else {
        eprintln!("skipped: set KILN_TEST_CADDY and KILN_TEST_PHP to run this test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let site_root = dir.path().join("site");
    std::fs::create_dir_all(site_root.join("docs")).unwrap();
    std::fs::write(
        site_root.join("index.php"),
        "<?php if ($_SERVER['REQUEST_URI'] === '/pid') { echo getmypid(); exit; }\n\
         echo 'front ', $_SERVER['REQUEST_URI'];",
    )
    .unwrap();
    std::fs::write(
        site_root.join("info.php"),
        "<?php echo 'info ', PHP_VERSION;",
    )
    .unwrap();
    std::fs::write(site_root.join("static.txt"), "static file").unwrap();
    std::fs::write(
        site_root.join("docs").join("index.php"),
        "<?php echo 'docs';",
    )
    .unwrap();

    // The supervisor, in-process: same loop and launcher as `kiln-supervisor`.
    let run_dir = dir.path().join("supervisor");
    std::fs::create_dir_all(&run_dir).unwrap();
    let pools_file = run_dir.join("pools.json");
    // Tiny recycle limits, so workers exit and restart many times during the test.
    let pool = |id: &str| {
        PoolSpec::new(
            id.into(),
            php.join("php-cgi.exe"),
            php.join("php.ini"),
            &free_ports(3).unwrap(),
            10,
        )
    };
    let first = Pools {
        pools: vec![pool("first")],
    };
    first.write(&pools_file).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let supervisor = {
        let (stop, pools_file, run_dir) = (Arc::clone(&stop), pools_file.clone(), run_dir.clone());
        std::thread::spawn(move || {
            let launcher = PhpCgiLauncher::new(&run_dir.join("php-errors.log")).unwrap();
            kiln_supervisor::run::run(
                &pools_file,
                &run_dir.join("supervisor.log"),
                launcher,
                &stop,
            )
            .unwrap();
        })
    };
    wait_listening(&first);

    let exe = dir.path().join("bin").join(kiln_web::EXE_NAME);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::copy(&caddy, &exe).unwrap();
    let server = WebServer::new(exe, dir.path().join("caddy"));
    let http_port = free_ports(1).unwrap()[0];
    let config = |upstreams| WebConfig {
        http_port,
        https_port: free_ports(1).unwrap()[0],
        ipv6: false,
        sites: vec![SiteSpec {
            host: HostName::new("blog.test", &Tld::default()).unwrap(),
            root: site_root.clone(),
            secure: false,
        }],
        ca: None,
        storage_dir: dir.path().join("caddy").join("storage"),
        log_file: server.log_file(),
        admin_socket: server.admin_socket(),
        php_upstreams: upstreams,
    };
    server.start(&config(first.upstreams())).unwrap();

    let result = std::panic::catch_unwind(|| {
        // PHP, the front controller, a directory index, and static files.
        let (code, body) = get(http_port, "blog.test", "/");
        assert_eq!((code, body.as_str()), (200, "front /"));
        let (code, body) = get(http_port, "blog.test", "/posts/42?x=1");
        assert_eq!((code, body.as_str()), (200, "front /posts/42?x=1"));
        let (code, body) = get(http_port, "blog.test", "/info.php");
        assert_eq!(code, 200);
        assert!(body.starts_with("info 8."), "{body}");
        let (code, _) = get(http_port, "blog.test", "/docs");
        assert_eq!(code, 308, "directory with index.php gets its slash");
        assert_eq!(get(http_port, "blog.test", "/docs/").1, "docs");
        let (code, body) = get(http_port, "blog.test", "/static.txt");
        assert_eq!((code, body.as_str()), (200, "static file"));

        // Hundreds of requests while workers recycle every 10–16 requests: none may fail
        // (I-14, I-15).
        let mut pids = std::collections::BTreeSet::new();
        let mut failures = Vec::new();
        for _ in 0..400 {
            match get(http_port, "blog.test", "/pid") {
                (200, body) => {
                    pids.insert(body);
                }
                other => failures.push(other),
            }
        }
        eprintln!("400 requests: {} distinct php-cgi PIDs", pids.len());
        assert_eq!(
            failures,
            Vec::<(u16, String)>::new(),
            "requests failed during recycling"
        );
        assert!(pids.len() > 10, "workers recycled ({} PIDs)", pids.len());

        // Switch to a new pool without downtime (ADR 0014 §11), with requests running.
        let running = Arc::new(AtomicBool::new(true));
        let load = {
            let running = Arc::clone(&running);
            std::thread::spawn(move || {
                let (mut ok, mut failed) = (0, Vec::new());
                while running.load(Ordering::Relaxed) {
                    match get(http_port, "blog.test", "/pid") {
                        (200, _) => ok += 1,
                        other => failed.push(other),
                    }
                }
                (ok, failed)
            })
        };
        let second = Pools {
            pools: vec![pool("second")],
        };
        let both = Pools {
            pools: first.pools.iter().chain(&second.pools).cloned().collect(),
        };
        both.write(&pools_file).unwrap();
        wait_listening(&second);
        server.reload(&config(second.upstreams())).unwrap();
        std::thread::sleep(kiln_supervisor::pools::RETIRE_GRACE);
        second.write(&pools_file).unwrap();
        std::thread::sleep(Duration::from_millis(800)); // the supervisor retires the old pool
        running.store(false, Ordering::Relaxed);
        let (ok, failed) = load.join().unwrap();
        eprintln!("during the switch: {ok} ok, {} failed", failed.len());
        // The one Caddy reload may cost a request: an empty close (I-47), or a 502 when a
        // request still in Caddy's old config lands on a worker that is recycling at that
        // moment, which the old config can no longer retry (I-50). With recycle limits of
        // 10–16 that collision is likely here; with the real 500 it is rare. Anything else, or
        // more than that, is a bug.
        assert!(
            failed.iter().all(|(code, _)| *code == 0 || *code == 502) && failed.len() <= 2,
            "{failed:?}"
        );
        for addr in first.upstreams() {
            assert!(
                TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_err(),
                "old worker {addr} retired"
            );
        }
        assert_eq!(
            get(http_port, "blog.test", "/").0,
            200,
            "served by the new pool"
        );
    });

    if result.is_err() {
        let log = std::fs::read_to_string(server.log_file()).unwrap_or_default();
        for line in log
            .lines()
            .filter(|l| l.contains("\"error\"") || l.contains("\"warn\""))
        {
            eprintln!("caddy: {line}");
        }
    }
    server.stop().unwrap();
    stop.store(true, Ordering::Relaxed);
    supervisor.join().unwrap();
    if let Err(panic) = result {
        let log = std::fs::read_to_string(run_dir.join("supervisor.log")).unwrap_or_default();
        let errors = std::fs::read_to_string(run_dir.join("php-errors.log")).unwrap_or_default();
        eprintln!("supervisor.log:\n{log}\nphp-errors.log:\n{errors}");
        std::panic::resume_unwind(panic);
    }
    // Stopping the supervisor took every worker with it.
    let all: Vec<_> = Pools::read(&pools_file).unwrap().unwrap().upstreams();
    for addr in all {
        assert!(
            TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_err(),
            "worker {addr} still running after the supervisor stopped"
        );
    }
}
