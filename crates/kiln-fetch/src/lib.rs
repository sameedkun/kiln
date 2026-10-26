//! Verified downloads ([ADR 0013]).
//!
//! - **The expected digest comes first.** [`Fetcher::fetch`] takes a SHA-256 or SHA-512 and
//!   hashes the body while streaming it to `<name>.partial`. Only a matching file is moved into
//!   the cache; anything else is deleted.
//! - **HTTPS only**, redirects included. Server certificates are checked against the operating
//!   system's trust store, and proxies come from the usual environment variables.
//! - **Cache:** verified files live in one folder and are reused only after being hashed again.
//!
//! [`Fetcher::get_text`] fetches small documents such as a release index, whose TLS connection
//! *is* the trust anchor.
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use sha2::{Digest as _, Sha256, Sha512};

/// Largest single download Kiln accepts.
pub const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

/// Longest a single body may take to arrive.
pub const BODY_TIMEOUT: Duration = Duration::from_hours(1);

/// Longest a download may go without receiving any data before it is abandoned (I-48).
pub const STALL_TIMEOUT: Duration = Duration::from_mins(1);

/// A digest the downloaded bytes must match, as lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    Sha256(String),
    Sha512(String),
}

impl Expected {
    /// Parses hex of the right length for the algorithm (any case).
    pub fn sha256(hex: &str) -> Result<Self, FetchError> {
        check_hex(hex, 64).map(Self::Sha256)
    }

    pub fn sha512(hex: &str) -> Result<Self, FetchError> {
        check_hex(hex, 128).map(Self::Sha512)
    }

    #[must_use]
    pub fn hex(&self) -> &str {
        match self {
            Self::Sha256(h) | Self::Sha512(h) => h,
        }
    }

    #[must_use]
    pub fn algorithm(&self) -> &'static str {
        match self {
            Self::Sha256(_) => "SHA-256",
            Self::Sha512(_) => "SHA-512",
        }
    }

    fn hasher(&self) -> Hasher {
        match self {
            Self::Sha256(_) => Hasher::Sha256(Sha256::new()),
            Self::Sha512(_) => Hasher::Sha512(Sha512::new()),
        }
    }
}

fn check_hex(hex: &str, len: usize) -> Result<String, FetchError> {
    let hex = hex.trim().to_ascii_lowercase();
    if hex.len() == len && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(hex)
    } else {
        Err(FetchError::BadDigest(hex))
    }
}

enum Hasher {
    Sha256(Sha256),
    Sha512(Sha512),
}

impl Hasher {
    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(h) => h.update(data),
            Self::Sha512(h) => h.update(data),
        }
    }

    fn finish_hex(self) -> String {
        let bytes: Vec<u8> = match self {
            Self::Sha256(h) => h.finalize().to_vec(),
            Self::Sha512(h) => h.finalize().to_vec(),
        };
        bytes.iter().fold(String::new(), |mut hex, b| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{b:02x}");
            hex
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("{0:?} isn't a valid hex digest")]
    BadDigest(String),
    #[error("{0:?} isn't a plain file name")]
    BadFileName(String),
    #[error("refusing {0}: Kiln only downloads over HTTPS")]
    NotHttps(String),
    #[error("download of {url} failed: {detail}")]
    Http { url: String, detail: String },
    #[error(
        "download of {url} stalled: no data arrived for {} s; check the connection and try again",
        .waited.as_secs()
    )]
    Stalled { url: String, waited: Duration },
    #[error("{url} is larger than the {limit} bytes allowed")]
    TooLarge { url: String, limit: u64 },
    #[error(
        "{url} doesn't match its expected {algorithm} and was discarded\n  expected: {expected}\n  got:      {actual}"
    )]
    Mismatch {
        url: String,
        algorithm: &'static str,
        expected: String,
        actual: String,
    },
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> FetchError {
    let context = context.into();
    move |source| FetchError::Io { context, source }
}

/// Downloads into a cache folder.
#[derive(Debug, Clone)]
pub struct Fetcher {
    agent: ureq::Agent,
    cache_dir: PathBuf,
    https_only: bool,
    stall_timeout: Duration,
}

impl Fetcher {
    /// A fetcher for real downloads: HTTPS only, OS trust store, proxies from the environment.
    #[must_use]
    pub fn new(cache_dir: PathBuf) -> Self {
        Self::build(cache_dir, true)
    }

    /// Plain HTTP allowed: only for tests against a local server.
    #[doc(hidden)]
    #[must_use]
    pub fn insecure_for_tests(cache_dir: PathBuf) -> Self {
        Self::build(cache_dir, false)
    }

    fn build(cache_dir: PathBuf, https_only: bool) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .build();
        let agent = ureq::Agent::config_builder()
            .https_only(https_only)
            .tls_config(tls)
            .proxy(ureq::Proxy::try_from_env())
            .max_redirects(10)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            // The whole body, not idle time, so generous enough for MAX_DOWNLOAD on a slow
            // line (at ~110 KB/s, PHP's 34 MB take about 5 minutes). `fetch` catches stalls
            // itself (STALL_TIMEOUT).
            .timeout_recv_body(Some(BODY_TIMEOUT))
            .user_agent(concat!("kiln/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self {
            agent,
            cache_dir,
            https_only,
            stall_timeout: STALL_TIMEOUT,
        }
    }

    /// A shorter stall timeout: only for tests.
    #[doc(hidden)]
    #[must_use]
    pub fn with_stall_timeout(mut self, timeout: Duration) -> Self {
        self.stall_timeout = timeout;
        self
    }

    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    fn check_url(&self, url: &str) -> Result<(), FetchError> {
        if self.https_only && !url.starts_with("https://") {
            return Err(FetchError::NotHttps(url.to_owned()));
        }
        Ok(())
    }

    fn get(&self, url: &str) -> Result<ureq::http::Response<ureq::Body>, FetchError> {
        self.check_url(url)?;
        self.agent.get(url).call().map_err(|e| FetchError::Http {
            url: url.to_owned(),
            detail: match e {
                ureq::Error::RequireHttpsOnly(_) => {
                    "a redirect pointed to a non-HTTPS address".to_owned()
                }
                other => other.to_string(),
            },
        })
    }

    /// A small text document (up to `limit` bytes), e.g. a release index.
    pub fn get_text(&self, url: &str, limit: u64) -> Result<String, FetchError> {
        let mut response = self.get(url)?;
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_string()
            .map_err(|e| match e {
                ureq::Error::BodyExceedsLimit(_) => FetchError::TooLarge {
                    url: url.to_owned(),
                    limit,
                },
                other => FetchError::Http {
                    url: url.to_owned(),
                    detail: other.to_string(),
                },
            })
    }

    /// The cached path for `file_name`, if it is there and matches `expected`.
    pub fn cached(
        &self,
        file_name: &str,
        expected: &Expected,
    ) -> Result<Option<PathBuf>, FetchError> {
        let path = self.cache_dir.join(plain_name(file_name)?);
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err(format!("cannot read {}", path.display()))(e)),
        };
        let mut hasher = expected.hasher();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(io_err(format!("cannot read {}", path.display())))?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok((hasher.finish_hex() == expected.hex()).then_some(path))
    }

    /// Downloads `url` as `file_name` into the cache, unless a verified copy is already there.
    /// `progress(bytes_so_far, total)` is called as data arrives.
    pub fn fetch(
        &self,
        url: &str,
        file_name: &str,
        expected: &Expected,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<PathBuf, FetchError> {
        if let Some(path) = self.cached(file_name, expected)? {
            return Ok(path);
        }
        let name = plain_name(file_name)?;
        fs::create_dir_all(&self.cache_dir).map_err(io_err(format!(
            "cannot create {}",
            self.cache_dir.display()
        )))?;
        let dest = self.cache_dir.join(name);
        let partial = self.cache_dir.join(format!("{name}.partial"));

        let response = self.get(url)?;
        let total = response.body().content_length();
        if total.is_some_and(|n| n > MAX_DOWNLOAD) {
            return Err(FetchError::TooLarge {
                url: url.to_owned(),
                limit: MAX_DOWNLOAD,
            });
        }
        let chunks = read_in_background(response);
        let result = (|| {
            let mut out = File::create(&partial)
                .map_err(io_err(format!("cannot create {}", partial.display())))?;
            let mut hasher = expected.hasher();
            let mut done = 0u64;
            loop {
                let chunk = match chunks.recv_timeout(self.stall_timeout) {
                    Ok(chunk) => chunk.map_err(|e| FetchError::Http {
                        url: url.to_owned(),
                        detail: e.to_string(),
                    })?,
                    Err(RecvTimeoutError::Timeout) => {
                        return Err(FetchError::Stalled {
                            url: url.to_owned(),
                            waited: self.stall_timeout,
                        });
                    }
                    // The reader only hangs up after sending the end or an error.
                    Err(RecvTimeoutError::Disconnected) => Vec::new(),
                };
                if chunk.is_empty() {
                    break;
                }
                hasher.update(&chunk);
                out.write_all(&chunk)
                    .map_err(io_err(format!("cannot write {}", partial.display())))?;
                done += chunk.len() as u64;
                progress(done, total);
            }
            out.sync_all()
                .map_err(io_err(format!("cannot write {}", partial.display())))?;
            drop(out);
            let actual = hasher.finish_hex();
            if actual != expected.hex() {
                return Err(FetchError::Mismatch {
                    url: url.to_owned(),
                    algorithm: expected.algorithm(),
                    expected: expected.hex().to_owned(),
                    actual,
                });
            }
            kiln_sys::rename_replace(&partial, &dest)
                .map_err(io_err(format!("cannot move {} into place", dest.display())))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result.map(|()| dest)
    }
}

/// Reads `response`'s body (up to [`MAX_DOWNLOAD`]) on its own thread and hands over each
/// chunk; an empty chunk is the end. ureq has no idle timeout, so the caller enforces one by
/// how long it waits for the next chunk (I-48). A thread blocked on a dead connection is left
/// behind and ends with the connection or at [`BODY_TIMEOUT`]; it holds no file.
fn read_in_background(
    response: ureq::http::Response<ureq::Body>,
) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::sync_channel(4);
    std::thread::spawn(move || {
        let mut reader = response
            .into_body()
            .into_with_config()
            .limit(MAX_DOWNLOAD)
            .reader();
        loop {
            let mut buffer = vec![0u8; 64 * 1024];
            let chunk = reader.read(&mut buffer).map(|n| {
                buffer.truncate(n);
                buffer
            });
            let last = !matches!(&chunk, Ok(c) if !c.is_empty());
            if sender.send(chunk).is_err() || last {
                return;
            }
        }
    });
    receiver
}

/// `name` if it is a plain file name: no separators, no `..`, nothing hidden.
pub fn plain_name(name: &str) -> Result<&str, FetchError> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && name.len() <= 255;
    if ok {
        Ok(name)
    } else {
        Err(FetchError::BadFileName(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Serves each scripted response to one connection, in order, on a local port.
    fn serve(responses: Vec<Vec<u8>>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(&response);
            }
        });
        (base, handle)
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn sha256_hex(data: &[u8]) -> String {
        let mut h = Expected::Sha256(String::new()).hasher();
        h.update(data);
        h.finish_hex()
    }

    #[test]
    fn digests_are_validated() {
        assert!(Expected::sha256(&"A".repeat(64)).is_ok());
        assert_eq!(
            Expected::sha256(&"A".repeat(64)).unwrap().hex(),
            "a".repeat(64)
        );
        assert!(Expected::sha256("abc").is_err());
        assert!(Expected::sha512(&"g".repeat(128)).is_err());
    }

    #[test]
    fn plain_names_only() {
        assert!(plain_name("php-8.4.27-nts-Win32-vs17-x64.zip").is_ok());
        for bad in ["", "../x.zip", "a/b.zip", r"a\b.zip", "C:x.zip", ".hidden"] {
            assert!(plain_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn plain_http_is_refused_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let fetcher = Fetcher::new(dir.path().into());
        let err = fetcher
            .fetch(
                "http://example.test/x.zip",
                "x.zip",
                &Expected::sha256(&"0".repeat(64)).unwrap(),
                &mut |_, _| {},
            )
            .unwrap_err();
        assert!(matches!(err, FetchError::NotHttps(_)), "{err}");
        assert!(matches!(
            fetcher.get_text("http://example.test/i.json", 10),
            Err(FetchError::NotHttps(_))
        ));
    }

    #[test]
    fn a_matching_download_is_cached_and_reused_offline() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"pretend this is a zip".to_vec();
        let expected = Expected::sha256(&sha256_hex(&body)).unwrap();
        let (base, server) = serve(vec![ok(&body)]);
        let fetcher = Fetcher::insecure_for_tests(dir.path().join("cache"));
        let mut seen = Vec::new();
        let path = fetcher
            .fetch(
                &format!("{base}/a.zip"),
                "a.zip",
                &expected,
                &mut |done, total| {
                    seen.push((done, total));
                },
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), body);
        assert_eq!(
            seen.last(),
            Some(&(body.len() as u64, Some(body.len() as u64)))
        );
        // The server is gone; the verified copy is reused.
        let again = fetcher
            .fetch(&format!("{base}/a.zip"), "a.zip", &expected, &mut |_, _| {})
            .unwrap();
        assert_eq!(again, path);
    }

    #[test]
    fn a_mismatching_download_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let (base, server) = serve(vec![ok(b"tampered")]);
        let fetcher = Fetcher::insecure_for_tests(dir.path().into());
        let expected = Expected::sha256(&sha256_hex(b"original")).unwrap();
        let err = fetcher
            .fetch(&format!("{base}/a.zip"), "a.zip", &expected, &mut |_, _| {})
            .unwrap_err();
        server.join().unwrap();
        assert!(matches!(err, FetchError::Mismatch { .. }), "{err}");
        assert!(!dir.path().join("a.zip").exists());
        assert!(!dir.path().join("a.zip.partial").exists());
    }

    #[test]
    fn a_tampered_cache_entry_is_downloaded_again() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.zip"), b"tampered").unwrap();
        let (base, server) = serve(vec![ok(b"original")]);
        let fetcher = Fetcher::insecure_for_tests(dir.path().into());
        let expected = Expected::sha256(&sha256_hex(b"original")).unwrap();
        let path = fetcher
            .fetch(&format!("{base}/a.zip"), "a.zip", &expected, &mut |_, _| {})
            .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"original");
    }

    #[test]
    fn sha512_and_redirects_work() {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let redirect = format!(
            "HTTP/1.1 302 Found\r\nLocation: {base}/real.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        let server = std::thread::spawn(move || {
            for response in [redirect, ok(b"caddy")] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(&response);
            }
        });
        let mut h = Expected::Sha512(String::new()).hasher();
        h.update(b"caddy");
        let expected = Expected::sha512(&h.finish_hex()).unwrap();
        let fetcher = Fetcher::insecure_for_tests(dir.path().into());
        let path = fetcher
            .fetch(
                &format!("{base}/start.zip"),
                "caddy.zip",
                &expected,
                &mut |_, _| {},
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"caddy");
    }

    #[test]
    fn a_stalled_download_is_abandoned_and_its_partial_file_removed() {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (release, wait) = mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            // Promise 100 bytes, send 5, then go quiet with the connection open.
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nhello");
            let _ = stream.flush();
            let _ = wait.recv();
        });
        let fetcher = Fetcher::insecure_for_tests(dir.path().into())
            .with_stall_timeout(Duration::from_millis(500));
        let expected = Expected::sha256(&"0".repeat(64)).unwrap();
        let mut seen = 0;
        let err = fetcher
            .fetch(
                &format!("{base}/a.zip"),
                "a.zip",
                &expected,
                &mut |done, _| {
                    seen = done;
                },
            )
            .unwrap_err();
        assert!(matches!(err, FetchError::Stalled { .. }), "{err}");
        assert_eq!(seen, 5, "the bytes that did arrive were counted");
        assert!(!dir.path().join("a.zip.partial").exists());
        release.send(()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn http_errors_and_size_limits_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let not_found =
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
        let (base, server) = serve(vec![not_found, ok(b"0123456789")]);
        let fetcher = Fetcher::insecure_for_tests(dir.path().into());
        let err = fetcher
            .get_text(&format!("{base}/missing"), 100)
            .unwrap_err();
        assert!(matches!(err, FetchError::Http { .. }), "{err}");
        let err = fetcher.get_text(&format!("{base}/big"), 4).unwrap_err();
        assert!(matches!(err, FetchError::TooLarge { .. }), "{err}");
        server.join().unwrap();
    }
}
