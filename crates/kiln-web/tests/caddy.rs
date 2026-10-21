//! End-to-end check against a real Caddy: plain HTTP sites, an HTTPS site with a certificate
//! from Kiln's CA, the redirect, the 404 for unknown hosts, and stop.
//!
//! Needs a Caddy binary: set `KILN_TEST_CADDY` to its path (for example the one `kiln web
//! install` placed under `%LOCALAPPDATA%\Kiln\bin`). Without it the test only prints a note,
//! because CI doesn't download Caddy yet (P1-15).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kiln_core::{HostName, Tld};
use kiln_pki::{CaStore, default_protector};
use kiln_web::{CaFiles, SiteSpec, Status, WebConfig, WebServer};

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
    }
}

/// Status code, headers and body of `GET /` with `Host: host`, over `stream`.
fn get(mut stream: impl Read + Write, host: &str) -> (u16, String) {
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
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

#[test]
#[allow(clippy::similar_names)] // HTTP and HTTPS ports are the clearest names
fn serves_http_sites_and_https_only_where_enabled() {
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

    let (http_port, https_port) = (free_port(), free_port());
    let mut config = WebConfig {
        http_port,
        https_port,
        ipv6: false,
        sites: vec![
            site(dir.path(), "blog", false),
            site(dir.path(), "shop", true),
        ],
        ca: Some(CaFiles {
            root_cert,
            intermediate_cert,
            intermediate_key,
        }),
        storage_dir: dir.path().join("run").join("storage"),
        log_file: dir.path().join("run").join("caddy.log"),
    };
    let server = WebServer::new(exe, dir.path().join("run"));

    let pid = server.start(&config).unwrap();
    let result = std::panic::catch_unwind(|| {
        assert_eq!(server.status().unwrap(), Status::Running { pid });

        // Plain site over HTTP.
        let (code, body) = http_get(http_port, "blog.test");
        assert_eq!(code, 200, "{body}");
        assert!(body.ends_with("blog ok"), "{body}");

        // Unknown host: 404, not some other site.
        let (code, body) = http_get(http_port, "nobody.test");
        assert_eq!(code, 404, "{body}");
        assert!(body.contains("kiln link"), "{body}");

        // Secure site: HTTP redirects to HTTPS on the configured port.
        let (code, body) = http_get(http_port, "shop.test");
        assert_eq!(code, 308, "{body}");
        assert!(
            body.contains(&format!("Location: https://shop.test:{https_port}/")),
            "{body}"
        );

        // HTTPS with a certificate that chains to Kiln's root. Caddy issues it in the
        // background right after start-up, so allow it a moment.
        let deadline = Instant::now() + Duration::from_secs(15);
        let (code, body) = loop {
            match https_get(https_port, "shop.test", &root.der) {
                Ok(response) => break response,
                Err(e) if Instant::now() < deadline => {
                    eprintln!("waiting for the certificate: {e}");
                    std::thread::sleep(Duration::from_millis(250));
                }
                Err(e) => panic!("HTTPS to shop.test failed: {e}\n{}", server.recent_log()),
            }
        };
        assert_eq!(code, 200, "{body}");
        assert!(body.ends_with("shop ok"), "{body}");

        // A plain site has no certificate, so HTTPS to it fails instead of serving it.
        assert!(https_get(https_port, "blog.test", &root.der).is_err());
    });

    // Restart with no secure sites: the HTTPS port is released.
    server.stop().unwrap();
    assert_eq!(server.status().unwrap(), Status::Stopped);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    config.sites.retain(|s| !s.secure);
    server.start(&config).unwrap();
    let https_free = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, https_port)));
    let (code, _) = http_get(http_port, "blog.test");
    server.stop().unwrap();
    assert_eq!(code, 200);
    assert!(
        https_free.is_ok(),
        "HTTPS port not bound without secure sites"
    );
    assert!(
        TcpListener::bind((Ipv4Addr::LOCALHOST, http_port)).is_ok(),
        "port released on stop"
    );
}
