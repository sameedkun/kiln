//! End-to-end check against a real Caddy: HTTP by default, HTTPS turned on and off by live
//! reloads (with requests running), the redirect, the 404, and a graceful stop.
//!
//! Needs a Caddy binary: set `KILN_TEST_CADDY` to its path (for example the one `kiln web
//! install` placed under `%LOCALAPPDATA%\Kiln\bin`). Without it the test only prints a note.
//! CI downloads Caddy that way and runs it on Windows and Linux.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kiln_core::{HostName, Tld};
use kiln_pki::{CaStore, default_protector};
use kiln_web::{CaFiles, SiteSpec, Status, WebConfig, WebError, WebServer};

fn caddy() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("KILN_TEST_CADDY")?);
    assert!(
        path.is_file(),
        "KILN_TEST_CADDY={} is not a file",
        path.display()
    );
    Some(path)
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn site(dir: &Path, name: &str, secure: bool) -> SiteSpec {
    let root = dir.join("sites").join(name);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("index.html"), format!("{name} ok")).unwrap();
    SiteSpec {
        host: HostName::new(&format!("{name}.test"), &Tld::default()).unwrap(),
        root,
        secure,
        php_pool: None,
        proxy: None,
    }
}

/// Status code, headers and body of `GET /` with `Host: host`, over `stream`.
fn get(mut stream: impl Read + Write, host: &str) -> (u16, String) {
    // A connection reset before the request is sent comes back as an empty response, which
    // the callers' assertions report.
    let _ = write!(
        stream,
        "GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    );
    let mut response = Vec::new();
    // TLS peers may close without close_notify; what was read so far is what matters.
    let _ = stream.read_to_end(&mut response);
    let text = String::from_utf8_lossy(&response).into_owned();
    let code = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, text)
}

fn http_get(port: u16, host: &str) -> (u16, String) {
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    get(stream, host)
}

fn https_get(port: u16, host: &str, root_der: &[u8]) -> Result<(u16, String), rustls::Error> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls_pki_types::CertificateDer::from(root_der.to_vec()))
        .unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = rustls_pki_types::ServerName::try_from(host.to_owned()).unwrap();
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name)?;
    let mut sock = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // Complete the handshake first, so certificate errors surface as errors.
    while conn.is_handshaking() {
        conn.complete_io(&mut sock).map_err(|e| {
            e.into_inner()
                .and_then(|inner| inner.downcast::<rustls::Error>().ok())
                .map_or(rustls::Error::General("handshake I/O failed".into()), |e| {
                    *e
                })
        })?;
    }
    Ok(get(rustls::StreamOwned::new(conn, sock), host))
}

/// HTTPS `GET /` to `host`, waiting up to 15 s for Caddy to issue the certificate (it does
/// that in the background after start-up or a reload).
fn https_get_eventually(server: &WebServer, port: u16, host: &str, root: &[u8]) -> (u16, String) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match https_get(port, host, root) {
            Ok(response) => return response,
            Err(e) if Instant::now() < deadline => {
                eprintln!("waiting for the certificate: {e}");
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => panic!("HTTPS to {host} failed: {e}\n{}", server.recent_log()),
        }
    }
}

fn port_is_free(port: u16) -> bool {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).is_ok()
}

#[test]
#[allow(clippy::similar_names, clippy::too_many_lines)] // one end-to-end story
fn serves_http_by_default_reloads_live_and_stops_gracefully() {
    let Some(caddy) = caddy() else {
        eprintln!("skipped: set KILN_TEST_CADDY to a Caddy binary to run this test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("bin").join(kiln_web::EXE_NAME);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::copy(&caddy, &exe).unwrap();

    let ca = CaStore::new(dir.path().join("ca"), default_protector());
    let root = ca.create_root(&Tld::default(), "test@kiln").unwrap();
    ca.issue_intermediate(&Tld::default()).unwrap();
    let (root_cert, intermediate_cert, intermediate_key) = ca.web_server_files();

    let server = WebServer::new(exe, dir.path().join("run"));
    let (http_port, https_port) = (free_port(), free_port());
    let blog = site(dir.path(), "blog", false);
    let shop = site(dir.path(), "shop", false);
    let with = |secure_shop: bool| WebConfig {
        http_port,
        https_port,
        ipv6: false,
        sites: vec![
            blog.clone(),
            SiteSpec {
                secure: secure_shop,
                ..shop.clone()
            },
        ],
        ca: Some(CaFiles {
            root_cert: root_cert.clone(),
            intermediate_cert: intermediate_cert.clone(),
            intermediate_key: intermediate_key.clone(),
        }),
        storage_dir: dir.path().join("run").join("storage"),
        log_file: server.log_file(),
        admin_socket: server.admin_socket(),
        php_pools: std::collections::BTreeMap::new(),
    };

    // HTTP by default: no secure site, so no HTTPS listener at all.
    let pid = server.start(&with(false)).unwrap();
    let result = std::panic::catch_unwind(|| {
        assert_eq!(server.status().unwrap(), Status::Running { pid });
        assert!(server.admin_socket().exists(), "admin socket created");
        assert!(
            port_is_free(https_port),
            "no HTTPS listener without a secure site"
        );
        let (code, body) = http_get(http_port, "shop.test");
        assert_eq!(code, 200, "{body}");
        assert!(body.ends_with("shop ok"), "{body}");
        // Each request is a JSON line in the access log, with its host and duration
        // (ADR 0065), and not in Caddy's own log.
        let access = server.access_log_file();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let line = loop {
            let text = std::fs::read_to_string(&access).unwrap_or_default();
            if let Some(line) = text.lines().find(|l| l.contains("shop.test")) {
                break line.to_owned();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no request in {}",
                access.display()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let entry: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert!(
            entry["request"]["host"]
                .as_str()
                .unwrap()
                .starts_with("shop.test")
        );
        assert_eq!(entry["status"], 200);
        assert!(entry["duration"].as_f64().is_some(), "{line}");
        let own = std::fs::read_to_string(server.log_file()).unwrap_or_default();
        assert!(
            !own.contains("handled request"),
            "access lines in caddy.log"
        );

        // Keep requesting a site while the config is reloaded underneath it.
        let stop_load = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let load = {
            let stop_load = Arc::clone(&stop_load);
            std::thread::spawn(move || {
                let (mut ok, mut failed) = (0u32, Vec::new());
                while !stop_load.load(std::sync::atomic::Ordering::Relaxed) {
                    // On Linux a connection queued on the old listener at a handoff can be
                    // reset before it completes: the same event as an empty close (I-47).
                    let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, http_port)) else {
                        failed.push((0, String::new()));
                        continue;
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(10)))
                        .unwrap();
                    match get(stream, "blog.test") {
                        (200, body) if body.ends_with("blog ok") => ok += 1,
                        other => failed.push(other),
                    }
                }
                (ok, failed)
            })
        };

        // Turn HTTPS on for shop: same process, new listener, certificate from Kiln's CA.
        server.reload(&with(true)).unwrap();
        assert_eq!(
            server.status().unwrap(),
            Status::Running { pid },
            "no restart"
        );
        let (code, body) = http_get(http_port, "shop.test");
        assert_eq!(code, 308, "{body}");
        assert!(
            body.contains(&format!("Location: https://shop.test:{https_port}/")),
            "{body}"
        );
        let (code, body) = https_get_eventually(&server, https_port, "shop.test", &root.der);
        assert_eq!(code, 200, "{body}");
        assert!(body.ends_with("shop ok"), "{body}");
        // Where `kiln` looks to know the certificate is there (I-69).
        assert!(kiln_web::issued::has_certificate(
            &dir.path().join("run").join("storage"),
            "shop.test"
        ));
        assert!(
            https_get(https_port, "blog.test", &root.der).is_err(),
            "plain site has no TLS"
        );
        let (code, body) = http_get(http_port, "nobody.test");
        assert_eq!(code, 404, "{body}");

        // And off again: the HTTPS listener goes away.
        server.reload(&with(false)).unwrap();
        assert_eq!(http_get(http_port, "shop.test").0, 200);
        assert!(
            port_is_free(https_port),
            "HTTPS listener closed after the last secure site"
        );

        // More reloads under load.
        for round in 0..6 {
            server.reload(&with(round % 2 == 0)).unwrap();
        }
        server.reload(&with(false)).unwrap();
        stop_load.store(true, std::sync::atomic::Ordering::Relaxed);
        let (ok, failed) = load.join().unwrap();
        eprintln!("requests during reloads: {ok} ok, {} failed", failed.len());
        assert!(ok > 0);
        // The server never goes down, and nothing is ever served wrong. Caddy on Windows may
        // close a brand-new connection without a response at a listener handoff: measured at
        // about one per 3–7 reloads under this saturating load, none without reloads (I-47).
        let reloads = 9;
        assert!(
            failed
                .iter()
                .all(|(code, body)| *code == 0 && body.is_empty()),
            "only empty closes are tolerated: {failed:?}"
        );
        assert!(
            failed.len() <= reloads,
            "{} failures in {reloads} reloads: {failed:?}",
            failed.len()
        );

        // A reload that needs a port someone else holds is refused up front; the running
        // config stays.
        let squatter = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let taken = squatter.local_addr().unwrap().port();
        let mut blocked = with(true);
        blocked.https_port = taken;
        let err = server.reload(&blocked).unwrap_err();
        assert!(matches!(err, WebError::PortInUse(_)), "{err}");
        assert_eq!(
            http_get(http_port, "shop.test").0,
            200,
            "old config still serving"
        );
        assert_eq!(server.status().unwrap(), Status::Running { pid });
    });

    // Graceful stop through the admin API; everything is released.
    let stopped = server.stop().unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    assert_eq!(stopped, Some(pid));
    assert_eq!(server.status().unwrap(), Status::Stopped);
    assert!(!server.admin_socket().exists(), "socket removed");
    assert!(port_is_free(http_port), "HTTP port released on stop");
    assert!(matches!(
        server.reload(&with(false)),
        Err(WebError::NotRunning)
    ));
}

#[test]
#[allow(clippy::similar_names)] // http_port and https_port
fn restarts_on_a_validated_config_and_keeps_serving_when_caddy_rejects_one() {
    let Some(caddy) = caddy() else {
        eprintln!("skipped: set KILN_TEST_CADDY to a Caddy binary to run this test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("bin").join(kiln_web::EXE_NAME);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::copy(&caddy, &exe).unwrap();

    let ca = CaStore::new(dir.path().join("ca"), default_protector());
    let root = ca.create_root(&Tld::default(), "test@kiln").unwrap();
    ca.issue_intermediate(&Tld::default()).unwrap();
    let (root_cert, intermediate_cert, intermediate_key) = ca.web_server_files();

    let server = WebServer::new(exe, dir.path().join("run"));
    let (http_port, https_port) = (free_port(), free_port());
    let blog = site(dir.path(), "blog", false);
    let shop = site(dir.path(), "shop", true);
    let with = |sites: Vec<SiteSpec>, ca: Option<CaFiles>| WebConfig {
        http_port,
        https_port,
        ipv6: false,
        sites,
        ca,
        storage_dir: dir.path().join("run").join("storage"),
        log_file: server.log_file(),
        admin_socket: server.admin_socket(),
        php_pools: std::collections::BTreeMap::new(),
    };
    let kiln_ca = CaFiles {
        root_cert,
        intermediate_cert,
        intermediate_key,
    };

    assert!(matches!(
        server.restart(&with(vec![blog.clone()], None)),
        Err(WebError::NotRunning)
    ));
    let first = server.start(&with(vec![blog.clone()], None)).unwrap();
    let result = std::panic::catch_unwind(|| {
        // A new process, on the new config, with the new HTTPS listener.
        let started = std::time::Instant::now();
        let second = server
            .restart(&with(
                vec![blog.clone(), shop.clone()],
                Some(kiln_ca.clone()),
            ))
            .unwrap();
        eprintln!("restart took {:?}", started.elapsed());
        assert_ne!(second, first);
        assert_eq!(server.status().unwrap(), Status::Running { pid: second });
        assert_eq!(http_get(http_port, "blog.test").0, 200);
        let (code, body) = https_get_eventually(&server, https_port, "shop.test", &root.der);
        assert_eq!(code, 200, "{body}");

        // A config Caddy rejects (the intermediate's files are missing) stops nothing.
        let missing = CaFiles {
            intermediate_cert: dir.path().join("missing.crt"),
            intermediate_key: dir.path().join("missing.key"),
            ..kiln_ca.clone()
        };
        let err = server
            .restart(&with(vec![blog.clone(), shop.clone()], Some(missing)))
            .unwrap_err();
        assert!(matches!(err, WebError::Validate(_)), "{err}");
        assert_eq!(server.status().unwrap(), Status::Running { pid: second });
        assert_eq!(http_get(http_port, "blog.test").0, 200, "still serving");
        assert!(
            !dir.path().join("run").join("caddy.json.next").exists(),
            "the rejected config is not left behind"
        );
    });
    server.stop().unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
