//! Client for Caddy's admin API over its Unix socket ([ADR 0012]).
//!
//! A minimal HTTP/1.1 exchange, one request per connection. Requests that fail before a
//! response arrives are retried: on Windows Caddy replaces its admin listener on every config
//! load, and the next request after that gets an EOF once ([I-46]). Every request Kiln sends
//! is safe to repeat (`/load` with the same config is a no-op).
//!
//! [ADR 0012]: ../../../docs/adr/0012-caddy-admin-channel.md
//! [I-46]: ../../../docs/issues.md#i-46

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const ATTEMPTS: u32 = 6;
const RETRY_DELAY: Duration = Duration::from_millis(50);
const IO_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("cannot reach the web server's admin socket {}: {source}", .socket.display())]
    Unreachable { socket: PathBuf, source: io::Error },
    #[error("the web server rejected the request ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("unreadable response from the web server: {0}")]
    BadResponse(String),
}

/// Caddy's admin API at `socket`.
#[derive(Debug, Clone)]
pub struct AdminClient {
    socket: PathBuf,
}

impl AdminClient {
    #[must_use]
    pub fn new(socket: PathBuf) -> Self {
        Self { socket }
    }

    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Replaces the running config (`POST /load`). Caddy validates it first and keeps the old
    /// config if it is rejected.
    pub fn load(&self, config_json: &str) -> Result<(), AdminError> {
        self.call("POST", "/load", Some(config_json.as_bytes()))
            .map(drop)
    }

    /// Asks Caddy to shut down gracefully (`POST /stop`).
    pub fn stop(&self) -> Result<(), AdminError> {
        self.call("POST", "/stop", None).map(drop)
    }

    /// The running config as JSON (`GET /config/`).
    pub fn config(&self) -> Result<String, AdminError> {
        self.call("GET", "/config/", None)
    }

    /// Sends a request, retrying while no response arrives; returns the body of a 2xx reply.
    fn call(&self, method: &str, path: &str, body: Option<&[u8]>) -> Result<String, AdminError> {
        let request = encode_request(method, path, body);
        let mut last = None;
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(RETRY_DELAY);
            }
            match self.exchange(&request) {
                Ok(raw) if raw.is_empty() => {
                    last = Some(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "connection closed before a response",
                    ));
                }
                Ok(raw) => {
                    let (status, body) = decode_response(&raw)?;
                    return if (200..300).contains(&status) {
                        Ok(body)
                    } else {
                        Err(AdminError::Rejected { status, body })
                    };
                }
                Err(e) => last = Some(e),
            }
        }
        Err(AdminError::Unreachable {
            socket: self.socket.clone(),
            source: last.unwrap_or_else(|| io::Error::other("no attempt made")),
        })
    }

    fn exchange(&self, request: &[u8]) -> io::Result<Vec<u8>> {
        let mut stream = connect(&self.socket)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        stream.write_all(request)?;
        let mut raw = Vec::new();
        match stream.read_to_end(&mut raw) {
            Ok(_) => Ok(raw),
            // A reset after a complete reply still counts as the reply.
            Err(_) if !raw.is_empty() => Ok(raw),
            Err(e) => Err(e),
        }
    }
}

#[cfg(windows)]
fn connect(socket: &Path) -> io::Result<std::net::TcpStream> {
    kiln_sys::win::connect_unix(socket)
}

#[cfg(unix)]
fn connect(socket: &Path) -> io::Result<std::os::unix::net::UnixStream> {
    std::os::unix::net::UnixStream::connect(socket)
}

fn encode_request(method: &str, path: &str, body: Option<&[u8]>) -> Vec<u8> {
    let body = body.unwrap_or_default();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    request
}

/// Status code and body of a complete HTTP/1.1 response (`Content-Length`, chunked, or read to
/// end of stream).
fn decode_response(raw: &[u8]) -> Result<(u16, String), AdminError> {
    let bad = |what: &str| AdminError::BadResponse(what.to_owned());
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| bad("no end of headers"))?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| bad("headers aren't UTF-8"))?;
    let rest = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| bad("no status line"))?;
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked") {
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            length = value.parse::<usize>().ok();
        }
    }
    let body = if chunked {
        dechunk(rest).ok_or_else(|| bad("malformed chunked body"))?
    } else {
        match length {
            Some(n) => rest
                .get(..n)
                .ok_or_else(|| bad("body shorter than Content-Length"))?,
            None => rest,
        }
        .to_vec()
    };
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&data[..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(data.get(..size)?);
        data = data.get(size + 2..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_carry_length_and_close() {
        let request = String::from_utf8(encode_request("POST", "/load", Some(b"{}"))).unwrap();
        assert!(request.starts_with("POST /load HTTP/1.1\r\nHost: localhost\r\n"));
        assert!(request.contains("Content-Length: 2\r\n"));
        assert!(request.contains("Connection: close\r\n"));
        assert!(request.ends_with("\r\n\r\n{}"));
        let get = String::from_utf8(encode_request("GET", "/config/", None)).unwrap();
        assert!(get.contains("Content-Length: 0\r\n"));
    }

    #[test]
    fn decodes_length_delimited_and_open_ended_bodies() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nnull trailing";
        assert_eq!(decode_response(raw).unwrap(), (200, "null".into()));
        let raw = b"HTTP/1.1 400 Bad Request\r\n\r\n{\"error\":\"bad\"}";
        assert_eq!(
            decode_response(raw).unwrap(),
            (400, "{\"error\":\"bad\"}".into())
        );
    }

    #[test]
    fn decodes_chunked_bodies() {
        let raw =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;x=y\r\n world\r\n0\r\n\r\n";
        assert_eq!(decode_response(raw).unwrap(), (200, "hello world".into()));
    }

    #[test]
    fn malformed_responses_are_errors() {
        assert!(decode_response(b"garbage").is_err());
        assert!(decode_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort").is_err());
        assert!(
            decode_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n")
                .is_err()
        );
    }

    #[test]
    fn an_absent_socket_is_unreachable_after_retries() {
        let dir = tempfile::tempdir().unwrap();
        let client = AdminClient::new(dir.path().join("admin.sock"));
        let started = std::time::Instant::now();
        let err = client.config().unwrap_err();
        assert!(matches!(err, AdminError::Unreachable { .. }), "{err}");
        assert!(started.elapsed() >= RETRY_DELAY * (ATTEMPTS - 1));
    }
}
