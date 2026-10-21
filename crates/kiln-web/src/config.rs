//! Caddy JSON config, generated from typed structs ([ADR 0005], [ADR 0011]).
//!
//! Only the subset of Caddy's schema that Kiln uses is modelled. The invariants that matter
//! (loopback listeners, a catch-all 404, no automatic HTTPS, no admin endpoint, Kiln's CA
//! without trust installation, HTTPS only for sites marked secure) are asserted by the tests
//! at the bottom of this file.
//!
//! [ADR 0005]: ../../../docs/adr/0005-web-server.md
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};

use kiln_core::HostName;
use serde::Serialize;

/// Name of Kiln's CA inside Caddy's PKI app.
pub const CA_ID: &str = "kiln";
/// Caddy server names, visible in Caddy's logs.
pub const HTTP_SERVER: &str = "kiln_http";
pub const HTTPS_SERVER: &str = "kiln_https";

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

    fn build(&self) -> Result<Caddy, ConfigError> {
        self.check()?;

        let mut plain_routes = Vec::new();
        let mut tls_routes = Vec::new();
        for site in &self.sites {
            let host = site.host.as_str();
            if site.secure {
                let target = origin("https", host, self.https_port, 443) + "{http.request.uri}";
                plain_routes.push(Route::for_host(host, Handler::redirect(target)));
                tls_routes.push(Route::for_host(host, Handler::files(&site.root)?));
            } else {
                plain_routes.push(Route::for_host(host, Handler::files(&site.root)?));
            }
        }
        plain_routes.push(Route::not_linked());

        let mut servers = BTreeMap::new();
        servers.insert(
            HTTP_SERVER,
            Server::new(self.listen(self.http_port), plain_routes, false),
        );

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
                    HTTPS_SERVER,
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
                disabled: true,
                config: AdminConfig { persist: false },
            },
            logging: Logging {
                logs: BTreeMap::from([(
                    "default",
                    Log {
                        writer: LogWriter {
                            output: "file",
                            filename: utf8(&self.log_file)?,
                            roll_size_mb: 10,
                            roll_keep: 3,
                        },
                        level: "INFO",
                    },
                )]),
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
    disabled: bool,
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
    level: &'static str,
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
    servers: BTreeMap<&'static str, Server>,
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
}

impl Server {
    fn new(listen: Vec<String>, routes: Vec<Route>, tls: bool) -> Self {
        Self {
            listen,
            automatic_https: AutomaticHttps { disable: true },
            tls_connection_policies: tls.then(|| vec![TlsConnectionPolicy {}]),
            protocols: vec!["h1", "h2"],
            routes,
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
                host: vec![host.to_owned()],
            }],
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

#[derive(Debug, Serialize)]
struct Matcher {
    host: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "handler", rename_all = "snake_case")]
enum Handler {
    FileServer {
        root: String,
        index_names: Vec<&'static str>,
    },
    StaticResponse {
        status_code: u16,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<&'static str, Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
}

impl Handler {
    fn files(root: &Path) -> Result<Self, ConfigError> {
        Ok(Self::FileServer {
            root: utf8(root)?,
            index_names: vec!["index.html", "index.htm"],
        })
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
            certificate_authorities: BTreeMap::from([(
                CA_ID,
                PkiCa {
                    name: "Kiln Local Development CA",
                    install_trust: false,
                    root: KeyPair {
                        certificate: utf8(&files.root_cert)?,
                        private_key: None,
                    },
                    intermediate: KeyPair {
                        certificate: utf8(&files.intermediate_cert)?,
                        private_key: Some(utf8(&files.intermediate_key)?),
                    },
                },
            )]),
        })
    }
}

#[derive(Debug, Serialize)]
struct PkiCa {
    name: &'static str,
    /// Never let Caddy touch OS trust stores; Kiln's `trust.ca.user` operation does (ADR 0010).
    install_trust: bool,
    root: KeyPair,
    intermediate: KeyPair,
}

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
        assert_eq!(ca["root"]["certificate"], "/kiln/ca/root.crt");
        assert!(ca["root"].get("private_key").is_none());
        assert_eq!(
            ca["intermediate"]["private_key"],
            "/kiln/ca/intermediate.key"
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

    #[test]
    fn admin_is_disabled_and_nothing_is_persisted() {
        let value = json(&config(vec![site("blog", false)]));
        assert_eq!(value["admin"]["disabled"], true);
        assert_eq!(value["admin"]["config"]["persist"], false);
        assert_eq!(value["storage"]["root"], "/kiln/caddy/storage");
        assert_eq!(
            value["logging"]["logs"]["default"]["writer"]["filename"],
            "/kiln/caddy/caddy.log"
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
