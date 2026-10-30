//! A proxy site against a real Caddy ([ADR 0032]): a tiny dev server on loopback, reached as
//! `app.test` by its port alone (so through `localhost`), with its own `Host`, the
//! forwarded headers, a WebSocket upgrade as live reload uses, and 502 once it's gone.
//!
//! Needs a Caddy binary in `KILN_TEST_CADDY`, as `caddy.rs` does.
//!
//! [ADR 0032]: ../../../docs/adr/0032-proxy-sites.md

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kiln_core::{HostName, Tld};
use kiln_web::{ProxyTarget, SiteSpec, WebConfig, WebServer};

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

/// A dev server: answers each request with the `Host` and `X-Forwarded-*` headers it got,
/// and turns an `Upgrade: websocket` request into a 101 followed by an echo.
fn dev_server(listener: TcpListener) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            std::thread::spawn(move || serve(stream));
        }
    });
}

fn serve(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        headers.push(line.trim_end().to_owned());
    }
    let header = |name: &str| {
        headers
            .iter()
            .find_map(|h| {
                let (key, value) = h.split_once(':')?;
                key.eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_owned())
            })
            .unwrap_or_default()
    };
    if header("Upgrade").eq_ignore_ascii_case("websocket") {
        let _ = write!(
            stream,
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n"
        );
        let mut buf = [0u8; 64];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || stream.write_all(&buf[..n]).is_err() {
                break;
            }
        }
        return;
    }
    let body = format!(
        "host={} xfh={} xfp={}",
        header("Host"),
        header("X-Forwarded-Host"),
        header("X-Forwarded-Proto")
    );
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// The response to `request`, read until the server closes or `until` appears.
fn exchange(port: u16, request: &str, until: Option<&str>) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let mut buf = [0u8; 1024];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&response);
        if until.is_some_and(|u| text.contains(u)) {
            break;
        }
        if text.starts_with("HTTP/1.1 101") && text.contains("\r\n\r\n") && until.is_some() {
            // Upgraded: say something, and wait for it to come back.
            if !text.contains("ping") {
                stream.write_all(b"ping").unwrap();
            }
        }
    }
    String::from_utf8_lossy(&response).into_owned()
}

fn get(port: u16) -> String {
    exchange(
        port,
        "GET /src/main.js HTTP/1.1\r\nHost: app.test\r\nConnection: close\r\n\r\n",
        None,
    )
}

#[test]
fn a_proxy_site_reaches_its_dev_server_with_live_reload() {
    let Some(caddy) = caddy() else {
        eprintln!("skipped: set KILN_TEST_CADDY to a Caddy binary to run this test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("bin").join(kiln_web::EXE_NAME);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::copy(&caddy, &exe).unwrap();

    let dev = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let dev_port = dev.local_addr().unwrap().port();
    dev_server(dev);

    let server = WebServer::new(exe, dir.path().join("run"));
    let http_port = free_port();
    let config = |target: &str| WebConfig {
        http_port,
        https_port: free_port(),
        ipv6: false,
        sites: vec![SiteSpec {
            host: HostName::new("app.test", &Tld::default()).unwrap(),
            root: PathBuf::new(),
            secure: false,
            php_pool: None,
            proxy: Some(ProxyTarget::parse(target).unwrap()),
        }],
        ca: None,
        storage_dir: dir.path().join("run").join("storage"),
        log_file: server.log_file(),
        admin_socket: server.admin_socket(),
        php_pools: std::collections::BTreeMap::new(),
    };

    // The port alone means localhost: Caddy finds the server on 127.0.0.1 even if it tries
    // ::1 first.
    server.start(&config(&dev_port.to_string())).unwrap();
    let result = std::panic::catch_unwind(|| {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut page = get(http_port);
        while !page.starts_with("HTTP/1.1 200") && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
            page = get(http_port);
        }
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        assert!(
            page.contains(&format!("host=localhost:{dev_port} ")),
            "the dev server sees its own host: {page}"
        );
        assert!(page.contains("xfh=app.test xfp=http"), "{page}");

        // Live reload: the WebSocket upgrade reaches the dev server, and data flows both ways.
        let ws = exchange(
            http_port,
            "GET /__vite_hmr HTTP/1.1\r\nHost: app.test\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
            Some("\r\n\r\nping"),
        );
        assert!(ws.starts_with("HTTP/1.1 101"), "{ws}");
        assert!(ws.ends_with("ping"), "echoed through Caddy: {ws}");

        // The dev server stops: 502 until it's back.
        server.reload(&config(&free_port().to_string())).unwrap();
        let page = get(http_port);
        assert!(page.starts_with("HTTP/1.1 502"), "{page}");
    });
    server.stop().unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
