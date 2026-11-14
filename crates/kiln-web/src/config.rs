//! Caddy JSON config, generated from typed structs ([ADR 0005], [ADR 0011], [ADR 0012]).
//!
//! Only the subset of Caddy's schema that Kiln uses is modelled. The invariants that matter
//! (loopback listeners, a catch-all 404, no automatic HTTPS, the admin endpoint only on a Unix
//! socket, Kiln's CA without trust installation, HTTPS only for sites marked secure) are
//! asserted by the tests at the bottom of this file.
//!
//! [ADR 0005]: ../../../docs/adr/0005-web-server.md
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md
//! [ADR 0012]: ../../../docs/adr/0012-caddy-admin-channel.md

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use kiln_core::HostName;
use serde::Serialize;

/// Name of Kiln's CA inside Caddy's PKI app.
pub const CA_ID: &str = "kiln";
/// Caddy server names, visible in Caddy's logs.
pub const HTTP_SERVER: &str = "kiln_http";
pub const HTTPS_SERVER: &str = "kiln_https";
/// The servers of sites shared on the LAN: `kiln_lan_0`, `kiln_lan_1`, … (ADR 0076 §4).
pub const LAN_SERVER: &str = "kiln_lan_";
/// Longest Unix socket path (`sockaddr_un`), excluding the terminating NUL.
pub const MAX_SOCKET_PATH: usize = 107;

/// Plain-text body for hosts that aren't linked ([I-17]).
///
/// [I-17]: ../../../docs/issues.md#i-17
pub const NOT_LINKED_BODY: &str =
    "No Kiln site is linked under this name.\nRun `kiln link <name>` in the project folder.\n";

/// One site as the web server sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteSpec {
    pub host: HostName,
    pub root: PathBuf,
    /// Serve over HTTPS (and redirect HTTP to it). Off by default (ADR 0011).
    pub secure: bool,
    /// The PHP pool (a version, e.g. `8.3.36`) serving this site (ADR 0016 §6). `None` for
    /// static sites (ADR 0015 §2).
    pub php_pool: Option<String>,
    /// Forward to this local dev server instead of serving `root` (ADR 0032).
    pub proxy: Option<crate::ProxyTarget>,
}

/// The CA files Caddy gets: the root certificate (no key) and the intermediate pair, from
/// `kiln_pki::CaStore::web_server_files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaFiles {
    pub root_cert: PathBuf,
    pub intermediate_cert: PathBuf,
    pub intermediate_key: PathBuf,
}

/// Everything the generated config depends on.
#[derive(Debug, Clone)]
pub struct WebConfig {
    pub http_port: u16,
    pub https_port: u16,
    /// Also listen on `[::1]`. Callers turn this off when the machine has no IPv6 loopback.
    pub ipv6: bool,
    pub sites: Vec<SiteSpec>,
    /// Required as soon as one site is secure.
    pub ca: Option<CaFiles>,
    /// Caddy's storage (issued leaf certificates, locks).
    pub storage_dir: PathBuf,
    /// Caddy's own log file (rolled by Caddy).
    pub log_file: PathBuf,
    /// The admin endpoint's Unix socket (ADR 0012). Protected by its folder's permissions.
    pub admin_socket: PathBuf,
    /// php-cgi pools by id, i.e. version (ADR 0014, 0016). A site whose pool is missing here is
    /// served as static files.
    pub php_pools: BTreeMap<String, Vec<SocketAddr>>,
    /// Sites shared on the LAN ([ADR 0076] §4): the only listeners beyond loopback, one per
    /// site, each on an address of its own. Empty unless the user shares a site.
    ///
    /// [ADR 0076]: ../../../docs/adr/0076-sharing-sites.md
    pub lan: Vec<LanShare>,
}

/// One site shared on the LAN: served at `address` (this PC's network address and a port
/// of its own) to any host name, over plain HTTP (a `.test` certificate can't name an IP).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanShare {
    pub host: HostName,
    pub address: SocketAddr,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "{0} is set to HTTPS, but Kiln's CA isn't available; run `kiln ca init` (or `kiln unsecure {0}`)"
    )]
    SecureWithoutCa(String),
    #[error("{0} is listed more than once")]
    DuplicateSite(String),
    #[error("HTTP and HTTPS can't both use port {0}")]
    SamePort(u16),
    #[error("port 0 isn't a usable port")]
    ZeroPort,
    #[error("{} isn't valid Unicode, which Caddy's config requires", .0.display())]
    NonUnicodePath(PathBuf),
    #[error(
        "the admin socket path {} is longer than {MAX_SOCKET_PATH} bytes, the limit for Unix sockets",
        .0.display()
    )]
    AdminSocketTooLong(PathBuf),
}

impl WebConfig {
    #[must_use]
    pub fn has_secure_sites(&self) -> bool {
        self.sites.iter().any(|s| s.secure)
    }

    fn bind_ips(&self) -> Vec<IpAddr> {
        let mut ips = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
        if self.ipv6 {
            ips.push(IpAddr::V6(Ipv6Addr::LOCALHOST));
        }
        ips
    }

    /// Every address Caddy will listen on: HTTP always, HTTPS only with a secure site.
    #[must_use]
    pub fn listeners(&self) -> Vec<SocketAddr> {
        let mut ports = vec![self.http_port];
        if self.has_secure_sites() {
            ports.push(self.https_port);
        }
        ports
            .into_iter()
            .flat_map(|port| {
                self.bind_ips()
                    .into_iter()
                    .map(move |ip| SocketAddr::new(ip, port))
            })
            .chain(self.lan.iter().map(|share| share.address))
            .collect()
    }

    /// The URL a site is reachable at, e.g. `http://blog.test:8080/`.
    #[must_use]
    pub fn site_url(&self, site: &SiteSpec) -> String {
        if site.secure {
            origin("https", site.host.as_str(), self.https_port, 443) + "/"
        } else {
            origin("http", site.host.as_str(), self.http_port, 80) + "/"
        }
    }

    /// The config as pretty-printed JSON, ready for `caddy run --config`.
    ///
    /// # Panics
    ///
    /// Never: the config structs hold only strings, numbers, booleans and maps with string keys.
    pub fn render(&self) -> Result<String, ConfigError> {
        let caddy = self.build()?;
        Ok(serde_json::to_string_pretty(&caddy).expect("config structs always serialize"))
    }

    fn check(&self) -> Result<(), ConfigError> {
        if self.http_port == 0 || self.https_port == 0 {
            return Err(ConfigError::ZeroPort);
        }
        if self.http_port == self.https_port {
            return Err(ConfigError::SamePort(self.http_port));
        }
        if utf8(&self.admin_socket)?.len() > MAX_SOCKET_PATH {
            return Err(ConfigError::AdminSocketTooLong(self.admin_socket.clone()));
        }
        let mut seen = BTreeSet::new();
        for site in &self.sites {
            if !seen.insert(site.host.as_str()) {
                return Err(ConfigError::DuplicateSite(site.host.to_string()));
            }
            if site.secure && self.ca.is_none() {
                return Err(ConfigError::SecureWithoutCa(site.host.to_string()));
            }
        }
        Ok(())
    }

    fn listen(&self, port: u16) -> Vec<String> {
        self.bind_ips()
            .into_iter()
            .map(|ip| SocketAddr::new(ip, port).to_string())
            .collect()
    }

    /// The dev server for a proxy site; else PHP through the worker pool when there is one,
    /// static files otherwise.
    fn site_handler(&self, site: &SiteSpec) -> Result<Handler, ConfigError> {
        if let Some(target) = &site.proxy {
            return Ok(Handler::proxy(target));
        }
        let upstreams = site
            .php_pool
            .as_ref()
            .and_then(|id| self.php_pools.get(id))
            .filter(|upstreams| !upstreams.is_empty());
        // Requests asking for Xdebug go to the PHP's debug worker, if it runs (ADR 0061).
        let debug = site
            .php_pool
            .as_ref()
            .and_then(|id| self.php_pools.get(&format!("{id}{DEBUG_POOL_SUFFIX}")))
            .filter(|upstreams| !upstreams.is_empty());
        match upstreams {
            Some(upstreams) => Handler::php(&site.root, upstreams, debug.map(Vec::as_slice)),
            None => Handler::files(&site.root),
        }
    }

    /// A site shared on the LAN: its own server on its own address, that site only, to any
    /// host name (phones on the network don't know `.test` names). A share for a site no
    /// longer served listens nowhere.
    fn lan_servers(&self, servers: &mut BTreeMap<String, Server>) -> Result<(), ConfigError> {
        for (n, share) in self.lan.iter().enumerate() {
            let Some(site) = self.sites.iter().find(|s| s.host == share.host) else {
                continue;
            };
            servers.insert(
                format!("{LAN_SERVER}{n}"),
                Server::new(
                    vec![share.address.to_string()],
                    vec![Route::any(self.site_handler(site)?)],
                    false,
                ),
            );
        }
        Ok(())
    }

    fn build(&self) -> Result<Caddy, ConfigError> {
        self.check()?;

        let mut plain_routes = Vec::new();
        let mut tls_routes = Vec::new();
        for site in &self.sites {
            let host = site.host.as_str();
            if site.secure {
                let target = origin("https", host, self.https_port, 443) + "{http.request.uri}";
                plain_routes.push(Route::for_host(host, Handler::redirect(target)));
                tls_routes.push(Route::for_host(host, self.site_handler(site)?));
            } else {
                plain_routes.push(Route::for_host(host, self.site_handler(site)?));
            }
        }
        plain_routes.push(Route::not_linked());

        let mut servers = BTreeMap::new();
        servers.insert(
            HTTP_SERVER.to_owned(),
            Server::new(self.listen(self.http_port), plain_routes, false),
        );
        self.lan_servers(&mut servers)?;

        let secure_hosts: Vec<String> = self
            .sites
            .iter()
            .filter(|s| s.secure)
            .map(|s| s.host.to_string())
            .collect();
        let (tls, pki) = match (&self.ca, secure_hosts.is_empty()) {
            (Some(ca), false) => {
                tls_routes.push(Route::not_linked());
                servers.insert(
                    HTTPS_SERVER.to_owned(),
                    Server::new(self.listen(self.https_port), tls_routes, true),
                );
                (
                    Some(TlsApp::internal(secure_hosts)),
                    Some(PkiApp::kiln(ca)?),
                )
            }
            _ => (None, None),
        };

        Ok(Caddy {
            admin: Admin {
                listen: admin_listen(&self.admin_socket)?,
                config: AdminConfig { persist: false },
            },
            logging: Logging {
                logs: BTreeMap::from([
                    (
                        "default",
                        Log {
                            writer: LogWriter {
                                output: "file",
                                filename: utf8(&self.log_file)?,
                                roll_size_mb: 10,
                                roll_keep: 3,
                            },
                            encoder: None,
                            level: "INFO",
                            include: Vec::new(),
                            exclude: vec![ACCESS_LOG],
                        },
                    ),
                    (
                        "access",
                        Log {
                            writer: LogWriter {
                                output: "file",
                                filename: utf8(
                                    &self
                                        .log_file
                                        .with_file_name(crate::process::ACCESS_LOG_FILE),
                                )?,
                                roll_size_mb: 10,
                                roll_keep: 3,
                            },
                            encoder: Some(LogEncoder { format: "json" }),
                            level: "INFO",
                            include: vec![ACCESS_LOG],
                            exclude: Vec::new(),
                        },
                    ),
                ]),
            },
            storage: Storage {
                module: "file_system",
                root: utf8(&self.storage_dir)?,
            },
            apps: Apps {
                http: HttpApp { servers },
                tls,
                pki,
            },
        })
    }
}

fn origin(scheme: &str, host: &str, port: u16, default_port: u16) -> String {
    if port == default_port {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{port}")
    }
}

/// Caddy's network address for a Unix socket, with forward slashes on Windows.
fn admin_listen(socket: &Path) -> Result<String, ConfigError> {
    Ok(format!("unix/{}", utf8(socket)?.replace('\\', "/")))
}

fn utf8(path: &Path) -> Result<String, ConfigError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| ConfigError::NonUnicodePath(path.to_owned()))
}

// ---------------------------------------------------------------------------------------------
// Caddy's JSON schema (the parts Kiln uses)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct Caddy {
    admin: Admin,
    logging: Logging,
    storage: Storage,
    apps: Apps,
}

#[derive(Debug, Serialize)]
struct Admin {
    /// `unix/<path>`: never a TCP address (ADR 0012).
    listen: String,
    config: AdminConfig,
}

#[derive(Debug, Serialize)]
struct AdminConfig {
    persist: bool,
}

#[derive(Debug, Serialize)]
struct Logging {
    logs: BTreeMap<&'static str, Log>,
}

#[derive(Debug, Serialize)]
struct Log {
    writer: LogWriter,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoder: Option<LogEncoder>,
    level: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    include: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct LogEncoder {
    format: &'static str,
}

/// The logger every server writes requests to ([ADR 0065]).
///
/// [ADR 0065]: ../../../docs/adr/0065-request-timing.md
const ACCESS_LOGGER: &str = "kiln_access";
const ACCESS_LOG: &str = "http.log.access.kiln_access";

#[derive(Debug, Serialize)]
struct ServerLogs {
    default_logger_name: &'static str,
}

#[derive(Debug, Serialize)]
struct LogWriter {
    output: &'static str,
    filename: String,
    roll_size_mb: u32,
    roll_keep: u32,
}

#[derive(Debug, Serialize)]
struct Storage {
    module: &'static str,
    root: String,
}

#[derive(Debug, Serialize)]
struct Apps {
    http: HttpApp,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls: Option<TlsApp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pki: Option<PkiApp>,
}

#[derive(Debug, Serialize)]
struct HttpApp {
    servers: BTreeMap<String, Server>,
}

#[derive(Debug, Serialize)]
struct Server {
    listen: Vec<String>,
    automatic_https: AutomaticHttps,
    /// Present (even as `[{}]`) means "this server speaks TLS".
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_connection_policies: Option<Vec<TlsConnectionPolicy>>,
    /// HTTP/1.1 and HTTP/2 only: no UDP listener for HTTP/3 until it is tested (ADR 0005).
    protocols: Vec<&'static str>,
    routes: Vec<Route>,
    logs: ServerLogs,
}

impl Server {
    fn new(listen: Vec<String>, routes: Vec<Route>, tls: bool) -> Self {
        Self {
            listen,
            automatic_https: AutomaticHttps { disable: true },
            tls_connection_policies: tls.then(|| vec![TlsConnectionPolicy {}]),
            protocols: vec!["h1", "h2"],
            routes,
            logs: ServerLogs {
                default_logger_name: ACCESS_LOGGER,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct AutomaticHttps {
    disable: bool,
}

/// Empty policy: serve whichever managed certificate matches the SNI.
#[derive(Debug, Serialize)]
struct TlsConnectionPolicy {}

#[derive(Debug, Serialize)]
struct Route {
    #[serde(rename = "match", skip_serializing_if = "Vec::is_empty")]
    matchers: Vec<Matcher>,
    handle: Vec<Handler>,
    terminal: bool,
}

impl Route {
    fn for_host(host: &str, handler: Handler) -> Self {
        Self {
            matchers: vec![Matcher {
                host: Some(vec![host.to_owned()]),
                ..Matcher::default()
            }],
            handle: vec![handler],
            terminal: true,
        }
    }

    /// A step inside a subroute matching any of `matchers`, one handler, not terminal.
    fn step_any(matchers: Vec<Matcher>, handler: Handler) -> Self {
        Self {
            matchers,
            handle: vec![handler],
            terminal: false,
        }
    }

    /// A step inside a subroute: optional matcher, one handler, not terminal.
    fn step(matcher: Option<Matcher>, handler: Handler) -> Self {
        Self {
            matchers: matcher.into_iter().collect(),
            handle: vec![handler],
            terminal: false,
        }
    }

    /// Every request, to one handler: a LAN share's only route.
    fn any(handler: Handler) -> Self {
        Self {
            matchers: Vec::new(),
            handle: vec![handler],
            terminal: true,
        }
    }

    fn not_linked() -> Self {
        Self {
            matchers: Vec::new(),
            handle: vec![Handler::StaticResponse {
                status_code: 404,
                headers: BTreeMap::from([(
                    "Content-Type",
                    vec!["text/plain; charset=utf-8".to_owned()],
                )]),
                body: Some(NOT_LINKED_BODY.to_owned()),
            }],
            terminal: true,
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct Matcher {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<FileMatcher>,
    #[serde(skip_serializing_if = "Option::is_none")]
    not: Option<Vec<Matcher>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    header_regexp: Option<BTreeMap<&'static str, RegexpMatcher>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<BTreeMap<&'static str, Vec<&'static str>>>,
}

#[derive(Debug, Serialize)]
struct RegexpMatcher {
    pattern: &'static str,
}

#[derive(Debug, Serialize)]
struct FileMatcher {
    try_files: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    split_path: Option<Vec<&'static str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    try_policy: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "handler", rename_all = "snake_case")]
enum Handler {
    FileServer {
        /// Absent inside a PHP subroute, which sets `vars.root` instead.
        #[serde(skip_serializing_if = "Option::is_none")]
        root: Option<String>,
        index_names: Vec<&'static str>,
    },
    StaticResponse {
        status_code: u16,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<&'static str, Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    Subroute {
        routes: Vec<Route>,
    },
    Vars {
        root: String,
    },
    Rewrite {
        uri: &'static str,
    },
    ReverseProxy {
        load_balancing: LoadBalancing,
        transport: FastCgiTransport,
        upstreams: Vec<Upstream>,
    },
    /// A proxy site's dev server (ADR 0032 §3).
    #[serde(rename = "reverse_proxy")]
    DevServer {
        upstreams: Vec<Upstream>,
        headers: ProxyHeaders,
        #[serde(skip_serializing_if = "Option::is_none")]
        transport: Option<HttpTransport>,
    },
}

/// Header changes on the way to the dev server.
#[derive(Debug, Serialize)]
struct ProxyHeaders {
    request: HeaderOps,
}

#[derive(Debug, Serialize)]
struct HeaderOps {
    set: BTreeMap<&'static str, Vec<String>>,
}

/// HTTP to an HTTPS dev server: its self-signed certificate is accepted, as the connection
/// stays on loopback.
#[derive(Debug, Serialize)]
struct HttpTransport {
    protocol: &'static str,
    tls: InsecureTls,
}

#[derive(Debug, Serialize)]
struct InsecureTls {
    insecure_skip_verify: bool,
}

/// [ADR 0014] §7: no passive health checks, retries bounded by time (I-14, I-15).
#[derive(Debug, Serialize)]
struct LoadBalancing {
    selection_policy: SelectionPolicy,
    /// Nanoseconds.
    try_duration: u64,
    /// Nanoseconds.
    try_interval: u64,
    retry_match: Vec<MethodMatcher>,
}

#[derive(Debug, Serialize)]
struct SelectionPolicy {
    policy: &'static str,
}

#[derive(Debug, Serialize)]
struct MethodMatcher {
    method: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct FastCgiTransport {
    protocol: &'static str,
    split_path: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct Upstream {
    dial: String,
}

/// How long Caddy keeps retrying other workers while one is recycling.
pub const PHP_TRY_DURATION: Duration = Duration::from_secs(2);
pub const PHP_TRY_INTERVAL: Duration = Duration::from_millis(50);

/// A PHP version's debug worker is the pool `<version>+xdebug` ([ADR 0061] §2).
///
/// [ADR 0061]: ../../../docs/adr/0061-xdebug.md
pub const DEBUG_POOL_SUFFIX: &str = "+xdebug";

/// A `Cookie` header carrying an Xdebug trigger: `XDEBUG_SESSION` (browser extensions) or
/// `XDEBUG_TRIGGER`, by name, not inside another cookie's value.
const DEBUG_COOKIE_PATTERN: &str = r"(^|;\s*)XDEBUG_(SESSION|TRIGGER)=";

/// Query parameters that ask Xdebug to debug the request.
const DEBUG_QUERY_KEYS: &[&str] = &["XDEBUG_SESSION_START", "XDEBUG_SESSION", "XDEBUG_TRIGGER"];

impl Handler {
    /// The dev server behind a proxy site. It sees its own `host:port` as `Host`, which dev
    /// servers such as Vite require; `X-Forwarded-Host` keeps the site's name.
    fn proxy(target: &crate::ProxyTarget) -> Self {
        Self::DevServer {
            upstreams: vec![Upstream {
                dial: target.dial(),
            }],
            headers: ProxyHeaders {
                request: HeaderOps {
                    set: BTreeMap::from([(
                        "Host",
                        vec!["{http.reverse_proxy.upstream.hostport}".to_owned()],
                    )]),
                },
            },
            transport: target.https.then_some(HttpTransport {
                protocol: "http",
                tls: InsecureTls {
                    insecure_skip_verify: true,
                },
            }),
        }
    }

    fn files(root: &Path) -> Result<Self, ConfigError> {
        Ok(Self::FileServer {
            root: Some(utf8(root)?),
            index_names: vec!["index.html", "index.htm"],
        })
    }

    /// What Caddy's `php_fastcgi` shorthand expands to (checked against `caddy adapt`):
    /// directory redirect, front-controller rewrite, `*.php` to the workers, static files.
    /// With `debug` upstreams, a request carrying an Xdebug trigger (the `XDEBUG_SESSION` or
    /// `XDEBUG_TRIGGER` cookie, or `XDEBUG_SESSION_START`, `XDEBUG_SESSION` or
    /// `XDEBUG_TRIGGER` in the query) goes to them instead ([ADR 0061] §3).
    ///
    /// [ADR 0061]: ../../../docs/adr/0061-xdebug.md
    fn php(
        root: &Path,
        upstreams: &[SocketAddr],
        debug: Option<&[SocketAddr]>,
    ) -> Result<Self, ConfigError> {
        let mut routes = vec![
            Route::step(None, Self::Vars { root: utf8(root)? }),
            Route::step(
                Some(Matcher {
                    file: Some(FileMatcher {
                        try_files: vec!["{http.request.uri.path}/index.php"],
                        split_path: None,
                        try_policy: None,
                    }),
                    not: Some(vec![Matcher {
                        path: Some(vec!["*/"]),
                        ..Matcher::default()
                    }]),
                    ..Matcher::default()
                }),
                Self::StaticResponse {
                    status_code: 308,
                    headers: BTreeMap::from([(
                        "Location",
                        vec![
                            "{http.request.orig_uri.path}/{http.request.orig_uri.prefixed_query}"
                                .to_owned(),
                        ],
                    )]),
                    body: None,
                },
            ),
            Route::step(
                Some(Matcher {
                    file: Some(FileMatcher {
                        try_files: vec![
                            "{http.request.uri.path}",
                            "{http.request.uri.path}/index.php",
                            "index.php",
                        ],
                        split_path: Some(vec![".php"]),
                        try_policy: Some("first_exist_fallback"),
                    }),
                    ..Matcher::default()
                }),
                Self::Rewrite {
                    uri: "{http.matchers.file.relative}",
                },
            ),
        ];
        let php = || Matcher {
            path: Some(vec!["*.php"]),
            ..Matcher::default()
        };
        if let Some(debug) = debug {
            // One matcher set per trigger: the sets are alternatives, each also needs *.php.
            let cookie = Matcher {
                header_regexp: Some(BTreeMap::from([(
                    "Cookie",
                    RegexpMatcher {
                        pattern: DEBUG_COOKIE_PATTERN,
                    },
                )])),
                ..php()
            };
            let queries = DEBUG_QUERY_KEYS.iter().map(|key| Matcher {
                query: Some(BTreeMap::from([(*key, vec!["*"])])),
                ..php()
            });
            routes.push(Route::step_any(
                std::iter::once(cookie).chain(queries).collect(),
                Self::fastcgi(debug),
            ));
        }
        routes.push(Route::step(Some(php()), Self::fastcgi(upstreams)));
        routes.push(Route::step(
            None,
            Self::FileServer {
                root: None,
                index_names: vec!["index.html", "index.htm"],
            },
        ));
        Ok(Self::Subroute { routes })
    }

    /// PHP through `FastCGI` to `upstreams`, as Caddy's `php_fastcgi` does.
    fn fastcgi(upstreams: &[SocketAddr]) -> Self {
        let nanos = |d: Duration| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
        Self::ReverseProxy {
            load_balancing: LoadBalancing {
                selection_policy: SelectionPolicy {
                    policy: "least_conn",
                },
                try_duration: nanos(PHP_TRY_DURATION),
                try_interval: nanos(PHP_TRY_INTERVAL),
                retry_match: vec![MethodMatcher {
                    method: vec!["GET", "HEAD"],
                }],
            },
            transport: FastCgiTransport {
                protocol: "fastcgi",
                split_path: vec![".php"],
            },
            upstreams: upstreams
                .iter()
                .map(|addr| Upstream {
                    dial: addr.to_string(),
                })
                .collect(),
        }
    }

    fn redirect(location: String) -> Self {
        Self::StaticResponse {
            status_code: 308,
            headers: BTreeMap::from([("Location", vec![location])]),
            body: None,
        }
    }
}

#[derive(Debug, Serialize)]
struct TlsApp {
    certificates: TlsCertificates,
    automation: TlsAutomation,
}

impl TlsApp {
    /// Certificates for exactly `hosts`, issued by Kiln's intermediate.
    fn internal(hosts: Vec<String>) -> Self {
        Self {
            certificates: TlsCertificates {
                automate: hosts.clone(),
            },
            automation: TlsAutomation {
                policies: vec![AutomationPolicy {
                    subjects: hosts,
                    issuers: vec![Issuer {
                        module: "internal",
                        ca: CA_ID,
                        sign_with_root: false,
                    }],
                }],
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct TlsCertificates {
    automate: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TlsAutomation {
    policies: Vec<AutomationPolicy>,
}

#[derive(Debug, Serialize)]
struct AutomationPolicy {
    subjects: Vec<String>,
    issuers: Vec<Issuer>,
}

#[derive(Debug, Serialize)]
struct Issuer {
    module: &'static str,
    ca: &'static str,
    sign_with_root: bool,
}

#[derive(Debug, Serialize)]
struct PkiApp {
    certificate_authorities: BTreeMap<&'static str, PkiCa>,
}

impl PkiApp {
    fn kiln(files: &CaFiles) -> Result<Self, ConfigError> {
        Ok(Self {
            certificate_authorities: BTreeMap::from([
                (
                    CA_ID,
                    PkiCa {
                        name: "Kiln Local Development CA",
                        install_trust: false,
                        root: Some(KeyPair {
                            certificate: utf8(&files.root_cert)?,
                            private_key: None,
                        }),
                        intermediate: Some(KeyPair {
                            certificate: utf8(&files.intermediate_cert)?,
                            private_key: Some(utf8(&files.intermediate_key)?),
                        }),
                    },
                ),
                (
                    CADDY_DEFAULT_CA_ID,
                    PkiCa {
                        name: "Caddy Local Authority (unused by Kiln)",
                        install_trust: false,
                        root: None,
                        intermediate: None,
                    },
                ),
            ]),
        })
    }
}

#[derive(Debug, Serialize)]
struct PkiCa {
    name: &'static str,
    /// Never let Caddy touch OS trust stores; Kiln's `trust.ca.user` operation does (ADR 0010).
    install_trust: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<KeyPair>,
    #[serde(skip_serializing_if = "Option::is_none")]
    intermediate: Option<KeyPair>,
}

/// Caddy's own default CA. Kiln never issues from it, but Caddy sets it up anyway for some
/// names (`.internal` sites did, I-71), and by default installs its root into the system and
/// browser trust stores. Declared here only to turn that off.
const CADDY_DEFAULT_CA_ID: &str = "local";

#[derive(Debug, Serialize)]
struct KeyPair {
    certificate: String,
    /// The root's key is never given to Caddy (ADR 0010 §4).
    #[serde(skip_serializing_if = "Option::is_none")]
    private_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::Tld;
    use serde_json::Value;

    fn site(name: &str, secure: bool) -> SiteSpec {
        SiteSpec {
            host: HostName::new(&format!("{name}.test"), &Tld::default()).unwrap(),
            root: PathBuf::from(format!("/srv/{name}")),
            secure,
            php_pool: Some("8.4.27".into()),
            proxy: None,
        }
    }

    fn config(sites: Vec<SiteSpec>) -> WebConfig {
        WebConfig {
            http_port: 8080,
            https_port: 8443,
            ipv6: true,
            sites,
            ca: Some(CaFiles {
                root_cert: "/kiln/ca/root.crt".into(),
                intermediate_cert: "/kiln/ca/intermediate.crt".into(),
                intermediate_key: "/kiln/ca/intermediate.key".into(),
            }),
            storage_dir: "/kiln/caddy/storage".into(),
            log_file: "/kiln/caddy/caddy.log".into(),
            admin_socket: "/kiln/caddy/admin.sock".into(),
            lan: Vec::new(),
            php_pools: BTreeMap::new(),
        }
    }

    fn json(config: &WebConfig) -> Value {
        serde_json::from_str(&config.render().unwrap()).unwrap()
    }

    fn servers(value: &Value) -> &serde_json::Map<String, Value> {
        value["apps"]["http"]["servers"].as_object().unwrap()
    }

    #[test]
    fn http_only_by_default_no_tls_or_pki() {
        let value = json(&config(vec![site("blog", false), site("shop", false)]));
        let servers = servers(&value);
        assert_eq!(
            servers.keys().collect::<Vec<_>>(),
            vec![HTTP_SERVER],
            "no HTTPS server without a secure site"
        );
        assert!(value["apps"].get("tls").is_none());
        assert!(value["apps"].get("pki").is_none());
        let routes = servers[HTTP_SERVER]["routes"].as_array().unwrap();
        assert_eq!(routes[0]["match"][0]["host"][0], "blog.test");
        assert_eq!(routes[0]["handle"][0]["handler"], "file_server");
        assert_eq!(routes[0]["handle"][0]["root"], "/srv/blog");
        assert_eq!(routes[1]["match"][0]["host"][0], "shop.test");
        assert_eq!(config(vec![]).listeners().len(), 2, "HTTP on v4 + v6 only");
    }

    #[test]
    fn plain_sites_work_without_any_ca() {
        let mut cfg = config(vec![site("blog", false)]);
        cfg.ca = None;
        let value = json(&cfg);
        assert!(value["apps"].get("pki").is_none());
    }

    #[test]
    fn a_secure_site_gets_https_a_redirect_and_a_kiln_issued_certificate() {
        let cfg = config(vec![site("blog", false), site("shop", true)]);
        let value = json(&cfg);
        let servers = servers(&value);

        let http = servers[HTTP_SERVER]["routes"].as_array().unwrap();
        assert_eq!(http[1]["match"][0]["host"][0], "shop.test");
        assert_eq!(http[1]["handle"][0]["status_code"], 308);
        assert_eq!(
            http[1]["handle"][0]["headers"]["Location"][0],
            "https://shop.test:8443{http.request.uri}"
        );

        let https = &servers[HTTPS_SERVER];
        assert_eq!(https["listen"][0], "127.0.0.1:8443");
        assert!(https["tls_connection_policies"].is_array());
        let routes = https["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 2, "only the secure site plus the 404");
        assert_eq!(routes[0]["match"][0]["host"][0], "shop.test");
        assert_eq!(routes[0]["handle"][0]["root"], "/srv/shop");

        let tls = &value["apps"]["tls"];
        assert_eq!(
            tls["certificates"]["automate"],
            serde_json::json!(["shop.test"])
        );
        let policy = &tls["automation"]["policies"][0];
        assert_eq!(policy["subjects"], serde_json::json!(["shop.test"]));
        assert_eq!(policy["issuers"][0]["module"], "internal");
        assert_eq!(policy["issuers"][0]["ca"], CA_ID);

        assert_eq!(cfg.listeners().len(), 4, "HTTP and HTTPS, v4 and v6");
        assert_eq!(cfg.site_url(&cfg.sites[0]), "http://blog.test:8080/");
        assert_eq!(cfg.site_url(&cfg.sites[1]), "https://shop.test:8443/");
    }

    #[test]
    fn caddy_gets_the_intermediate_key_but_never_the_root_key_or_trust_install() {
        let value = json(&config(vec![site("shop", true)]));
        let ca = &value["apps"]["pki"]["certificate_authorities"][CA_ID];
        assert_eq!(ca["install_trust"], false);
        // Nor for Caddy's own default CA, which it may set up by itself (I-71).
        let local = &value["apps"]["pki"]["certificate_authorities"]["local"];
        assert_eq!(local["install_trust"], false);
        assert!(local.get("root").is_none(), "{local}");
        assert_eq!(ca["root"]["certificate"], "/kiln/ca/root.crt");
        assert!(ca["root"].get("private_key").is_none());
        assert_eq!(
            ca["intermediate"]["private_key"],
            "/kiln/ca/intermediate.key"
        );
    }

    #[test]
    fn a_lan_share_is_the_only_listener_beyond_loopback_and_serves_only_its_site() {
        let mut cfg = config(vec![site("blog", false), site("shop", true)]);
        let lan: SocketAddr = "192.168.1.20:8101".parse().unwrap();
        cfg.lan = vec![LanShare {
            host: site("shop", true).host,
            address: lan,
        }];
        let beyond: Vec<_> = cfg
            .listeners()
            .into_iter()
            .filter(|a| !a.ip().is_loopback())
            .collect();
        assert_eq!(beyond, [lan]);
        let value = json(&cfg);
        let all = servers(&value);
        let shared = &all[&format!("{LAN_SERVER}0")];
        assert_eq!(shared["listen"], serde_json::json!(["192.168.1.20:8101"]));
        let routes = shared["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 1, "one route: that site, for any host");
        assert!(routes[0].get("match").is_none());
        assert_eq!(
            routes[0]["handle"],
            json(&config(vec![site("shop", true)]))["apps"]["http"]["servers"][HTTPS_SERVER]["routes"]
                [0]["handle"]
        );
        for (name, server) in all {
            if name.starts_with(LAN_SERVER) {
                continue;
            }
            for listen in server["listen"].as_array().unwrap() {
                let addr: SocketAddr = listen.as_str().unwrap().parse().unwrap();
                assert!(addr.ip().is_loopback(), "{name}: {addr}");
            }
        }
        // A share for a site Kiln no longer serves listens nowhere.
        cfg.lan[0].host = site("gone", false).host;
        assert!(
            cfg.listeners()
                .iter()
                .all(|a| a.ip().is_loopback() || *a == lan)
        );
        assert!(
            !servers(&json(&cfg))
                .keys()
                .any(|n| n.starts_with(LAN_SERVER))
        );
    }

    #[test]
    fn every_listener_is_loopback_and_every_server_ends_in_a_404() {
        for cfg in [
            config(vec![]),
            config(vec![site("blog", false)]),
            config(vec![site("blog", false), site("shop", true)]),
        ] {
            for addr in cfg.listeners() {
                assert!(addr.ip().is_loopback(), "{addr}");
            }
            let value = json(&cfg);
            for (name, server) in servers(&value) {
                for listen in server["listen"].as_array().unwrap() {
                    let addr: SocketAddr = listen.as_str().unwrap().parse().unwrap();
                    assert!(addr.ip().is_loopback(), "{name}: {addr}");
                }
                assert_eq!(server["automatic_https"]["disable"], true, "{name}");
                let last = server["routes"].as_array().unwrap().last().unwrap();
                assert!(
                    last.get("match").is_none(),
                    "{name}: catch-all has no matcher"
                );
                assert_eq!(last["handle"][0]["status_code"], 404, "{name}");
            }
        }
    }

    fn with_php(sites: Vec<SiteSpec>) -> WebConfig {
        let mut cfg = config(sites);
        cfg.php_pools = BTreeMap::from([(
            "8.4.27".to_owned(),
            vec![
                "127.0.0.1:9101".parse().unwrap(),
                "127.0.0.1:9102".parse().unwrap(),
            ],
        )]);
        cfg
    }

    /// The handler list inside a site's subroute, as (matcher, handler) pairs.
    fn php_steps(site_route: &Value) -> &Vec<Value> {
        let handler = &site_route["handle"][0];
        assert_eq!(handler["handler"], "subroute");
        handler["routes"].as_array().unwrap()
    }

    #[test]
    fn a_debug_worker_takes_only_requests_that_ask_for_xdebug() {
        let mut cfg = with_php(vec![site("blog", false)]);
        let value = json(&cfg);
        let plain = php_steps(&servers(&value)[HTTP_SERVER]["routes"][0]).clone();
        assert_eq!(plain.len(), 5, "no debug worker, no debug route");

        cfg.php_pools.insert(
            format!("8.4.27{DEBUG_POOL_SUFFIX}"),
            vec!["127.0.0.1:9199".parse().unwrap()],
        );
        let value = json(&cfg);
        let steps = php_steps(&servers(&value)[HTTP_SERVER]["routes"][0]);
        assert_eq!(steps.len(), 6);
        // After the front-controller rewrite, before the plain workers.
        assert_eq!(steps[2]["handle"][0]["handler"], "rewrite");
        let debug = &steps[3];
        assert_eq!(
            debug["handle"][0]["upstreams"],
            serde_json::json!([{"dial": "127.0.0.1:9199"}])
        );
        assert_eq!(
            steps[4]["handle"][0]["upstreams"][0]["dial"],
            "127.0.0.1:9101"
        );
        let sets = debug["match"].as_array().unwrap();
        assert_eq!(sets.len(), 4, "the cookie, then one set per query key");
        for set in sets {
            assert_eq!(set["path"], serde_json::json!(["*.php"]), "{set}");
        }
        let pattern = sets[0]["header_regexp"]["Cookie"]["pattern"]
            .as_str()
            .unwrap();
        assert_eq!(pattern, r"(^|;\s*)XDEBUG_(SESSION|TRIGGER)=");
        assert_eq!(
            sets[1]["query"],
            serde_json::json!({"XDEBUG_SESSION_START": ["*"]})
        );
    }

    #[test]
    fn with_a_pool_every_site_runs_php_like_caddys_php_fastcgi() {
        let value = json(&with_php(vec![site("blog", false), site("shop", true)]));
        let servers = servers(&value);
        let blog = &servers[HTTP_SERVER]["routes"][0];
        let steps = php_steps(blog);
        let handlers: Vec<&str> = steps
            .iter()
            .map(|s| s["handle"][0]["handler"].as_str().unwrap())
            .collect();
        assert_eq!(
            handlers,
            [
                "vars",
                "static_response",
                "rewrite",
                "reverse_proxy",
                "file_server"
            ]
        );
        assert_eq!(steps[0]["handle"][0]["root"], "/srv/blog");
        assert_eq!(steps[1]["handle"][0]["status_code"], 308, "dir → dir/");
        assert_eq!(
            steps[2]["match"][0]["file"]["try_files"],
            serde_json::json!([
                "{http.request.uri.path}",
                "{http.request.uri.path}/index.php",
                "index.php"
            ]),
            "front controller fallback"
        );
        assert_eq!(steps[3]["match"][0]["path"], serde_json::json!(["*.php"]));
        let proxy = &steps[3]["handle"][0];
        assert_eq!(proxy["transport"]["protocol"], "fastcgi");
        assert_eq!(
            proxy["transport"]["split_path"],
            serde_json::json!([".php"])
        );
        assert_eq!(
            proxy["upstreams"],
            serde_json::json!([{"dial": "127.0.0.1:9101"}, {"dial": "127.0.0.1:9102"}])
        );
        assert!(
            steps[4]["handle"][0].get("root").is_none(),
            "uses vars.root"
        );

        // The secure site runs PHP on HTTPS; its HTTP route stays a redirect.
        let shop_tls = &servers[HTTPS_SERVER]["routes"][0];
        assert_eq!(php_steps(shop_tls)[0]["handle"][0]["root"], "/srv/shop");
        assert_eq!(
            servers[HTTP_SERVER]["routes"][1]["handle"][0]["status_code"],
            308
        );
    }

    #[test]
    fn php_load_balancing_never_quarantines_and_retries_by_time() {
        let value = json(&with_php(vec![site("blog", false)]));
        let proxy = &php_steps(&servers(&value)[HTTP_SERVER]["routes"][0])[3]["handle"][0];
        let lb = &proxy["load_balancing"];
        assert_eq!(lb["selection_policy"]["policy"], "least_conn");
        assert_eq!(
            lb["try_duration"], 2_000_000_000u64,
            "I-15: bounded by time"
        );
        assert_eq!(lb["try_interval"], 50_000_000u64);
        assert!(lb.get("retries").is_none(), "I-15: never lb_retries");
        assert!(
            proxy.get("health_checks").is_none(),
            "I-14: no passive quarantine"
        );
        assert_eq!(
            lb["retry_match"][0]["method"],
            serde_json::json!(["GET", "HEAD"])
        );
    }

    #[test]
    fn each_site_reaches_the_pool_of_its_own_version() {
        let mut legacy = site("legacy", false);
        legacy.php_pool = Some("8.1.34".into());
        let mut gone = site("gone", false);
        gone.php_pool = Some("7.4.33".into()); // no such pool running: static
        let mut cfg = with_php(vec![site("blog", false), legacy, gone]);
        cfg.php_pools
            .insert("8.1.34".into(), vec!["127.0.0.1:9201".parse().unwrap()]);
        let value = json(&cfg);
        let routes = servers(&value)[HTTP_SERVER]["routes"].as_array().unwrap();
        let dial = |i: usize| php_steps(&routes[i])[3]["handle"][0]["upstreams"][0]["dial"].clone();
        assert_eq!(routes[0]["match"][0]["host"][0], "blog.test");
        assert_eq!(dial(0), "127.0.0.1:9101");
        assert_eq!(routes[1]["match"][0]["host"][0], "legacy.test");
        assert_eq!(dial(1), "127.0.0.1:9201");
        assert_eq!(routes[2]["match"][0]["host"][0], "gone.test");
        assert_eq!(routes[2]["handle"][0]["handler"], "file_server");
    }

    #[test]
    fn a_static_site_skips_php_even_with_a_pool() {
        let mut docs = site("docs", false);
        docs.php_pool = None;
        let value = json(&with_php(vec![site("blog", false), docs]));
        let routes = servers(&value)[HTTP_SERVER]["routes"].as_array().unwrap();
        assert_eq!(
            routes[0]["handle"][0]["handler"], "subroute",
            "blog runs PHP"
        );
        assert_eq!(
            routes[1]["handle"][0]["handler"], "file_server",
            "docs is static"
        );
        assert_eq!(routes[1]["handle"][0]["root"], "/srv/docs");
    }

    #[test]
    fn without_a_pool_sites_stay_static() {
        let value = json(&config(vec![site("blog", false)]));
        let route = &servers(&value)[HTTP_SERVER]["routes"][0];
        assert_eq!(route["handle"][0]["handler"], "file_server");
        assert!(!value.to_string().contains("reverse_proxy"));
    }

    #[test]
    fn a_proxy_site_forwards_to_its_dev_server_with_the_targets_host() {
        let mut vite = site("app", true);
        vite.proxy = Some(crate::ProxyTarget::parse("5173").unwrap());
        let mut tls_api = site("api", false);
        tls_api.proxy = Some(crate::ProxyTarget::parse("https://[::1]:8443").unwrap());
        let mut cfg = with_php(vec![vite, tls_api]);
        cfg.ipv6 = true;
        let value = json(&cfg);

        let app = &servers(&value)[HTTPS_SERVER]["routes"][0];
        assert_eq!(app["match"][0]["host"][0], "app.test");
        let handler = &app["handle"][0];
        assert_eq!(handler["handler"], "reverse_proxy");
        assert_eq!(handler["upstreams"][0]["dial"], "localhost:5173");
        assert_eq!(
            handler["headers"]["request"]["set"]["Host"][0],
            "{http.reverse_proxy.upstream.hostport}"
        );
        assert!(
            handler.get("transport").is_none(),
            "plain HTTP to the dev server"
        );
        assert!(handler.get("load_balancing").is_none());
        // HTTP redirects to it, as for any secure site.
        let redirect = &servers(&value)[HTTP_SERVER]["routes"][0]["handle"][0];
        assert_eq!(redirect["status_code"], 308);

        let api = &servers(&value)[HTTP_SERVER]["routes"][1]["handle"][0];
        assert_eq!(api["upstreams"][0]["dial"], "[::1]:8443");
        assert_eq!(api["transport"]["protocol"], "http");
        assert_eq!(api["transport"]["tls"]["insecure_skip_verify"], true);
    }

    #[test]
    fn admin_listens_only_on_the_unix_socket_and_nothing_is_persisted() {
        let value = json(&config(vec![site("blog", false)]));
        assert_eq!(value["admin"]["listen"], "unix//kiln/caddy/admin.sock");
        assert!(value["admin"].get("disabled").is_none());
        assert_eq!(value["admin"]["config"]["persist"], false);
        assert_eq!(value["storage"]["root"], "/kiln/caddy/storage");
        assert_eq!(
            value["logging"]["logs"]["default"]["writer"]["filename"],
            "/kiln/caddy/caddy.log"
        );
    }

    #[test]
    fn windows_socket_paths_use_forward_slashes() {
        let mut cfg = config(vec![]);
        cfg.admin_socket = r"C:\Users\dev\AppData\Local\Kiln\caddy\admin.sock".into();
        assert_eq!(
            json(&cfg)["admin"]["listen"],
            "unix/C:/Users/dev/AppData/Local/Kiln/caddy/admin.sock"
        );
    }

    #[test]
    fn overlong_admin_socket_paths_are_refused() {
        let mut cfg = config(vec![]);
        cfg.admin_socket = format!("/{}/admin.sock", "d".repeat(100)).into();
        assert_eq!(
            cfg.render(),
            Err(ConfigError::AdminSocketTooLong(cfg.admin_socket.clone()))
        );
    }

    #[test]
    fn default_ports_are_left_out_of_urls() {
        let mut cfg = config(vec![site("blog", false), site("shop", true)]);
        cfg.http_port = 80;
        cfg.https_port = 443;
        cfg.ipv6 = false;
        assert_eq!(cfg.site_url(&cfg.sites[0]), "http://blog.test/");
        assert_eq!(cfg.site_url(&cfg.sites[1]), "https://shop.test/");
        let value = json(&cfg);
        assert_eq!(
            servers(&value)[HTTP_SERVER]["routes"][1]["handle"][0]["headers"]["Location"][0],
            "https://shop.test{http.request.uri}"
        );
        assert_eq!(
            servers(&value)[HTTP_SERVER]["listen"],
            serde_json::json!(["127.0.0.1:80"])
        );
    }

    #[test]
    fn invalid_configs_are_refused() {
        let mut cfg = config(vec![site("shop", true)]);
        cfg.ca = None;
        assert_eq!(
            cfg.render(),
            Err(ConfigError::SecureWithoutCa("shop.test".into()))
        );
        assert_eq!(
            config(vec![site("blog", false), site("blog", true)]).render(),
            Err(ConfigError::DuplicateSite("blog.test".into()))
        );
        let mut cfg = config(vec![]);
        cfg.https_port = cfg.http_port;
        assert_eq!(cfg.render(), Err(ConfigError::SamePort(8080)));
        cfg.http_port = 0;
        assert_eq!(cfg.render(), Err(ConfigError::ZeroPort));
    }
}
