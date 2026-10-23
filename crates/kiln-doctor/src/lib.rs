//! `kiln doctor` ([ADR 0017]): read-only health checks.
//!
//! The CLI gathers [`Facts`] (database, sockets, processes, files); [`diagnose`] turns them
//! into [`Finding`]s with pure rules, so every rule is tested with made-up facts.
//!
//! [ADR 0017]: ../../../docs/adr/0017-kiln-doctor.md

use std::fmt::Write as _;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use serde::Serialize;

/// How bad a finding is, mildest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Info,
    Warn,
    Fail,
}

/// What a finding is about, in report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Area {
    State,
    Environment,
    Ports,
    Web,
    Php,
    Sites,
    Dns,
    Certificates,
    Shims,
    OtherTools,
}

impl Area {
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::State => "State",
            Self::Environment => "Environment",
            Self::Ports => "Ports",
            Self::Web => "Web server",
            Self::Php => "PHP",
            Self::Sites => "Sites",
            Self::Dns => "DNS",
            Self::Certificates => "Certificates",
            Self::Shims => "Shims",
            Self::OtherTools => "Other tools",
        }
    }
}

/// One observation, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub area: Area,
    pub status: Status,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

fn finding(area: Area, status: Status, message: impl Into<String>) -> Finding {
    Finding {
        area,
        status,
        message: message.into(),
        fix: None,
    }
}

fn with_fix(mut finding: Finding, fix: impl Into<String>) -> Finding {
    finding.fix = Some(fix.into());
    finding
}

// ---------------------------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------------------------

/// Everything the rules look at, gathered by the caller.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub state: StateFacts,
    /// Executables of this process's ancestors, nearest first.
    pub ancestors: Vec<PathBuf>,
    pub ports: Vec<PortFacts>,
    pub web: WebFacts,
    pub php: PhpFacts,
    pub sites: SiteFacts,
    /// Only gathered when a site uses HTTPS.
    pub certificates: Option<CertFacts>,
    pub shims: ShimFacts,
    pub others: OtherToolsFacts,
    /// How this machine resolves names (Linux; `None` elsewhere).
    pub resolver: Option<ResolverFacts>,
}

#[derive(Debug, Clone, Default)]
pub struct StateFacts {
    /// Why the database can't be opened, if it can't.
    pub open_error: Option<String>,
    pub schema: i64,
    pub supported: i64,
    /// Operations left half-done: (id, kind, state).
    pub unfinished: Vec<(u64, String, String)>,
}

/// A port Kiln wants and who holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortFacts {
    pub addr: SocketAddr,
    /// "HTTP" or "HTTPS".
    pub purpose: &'static str,
    pub owner: Option<Owner>,
    /// Kiln picks this port itself, so `kiln web start` moves to a free one ([ADR 0019] §3).
    ///
    /// [ADR 0019]: ../../../docs/adr/0019-coexistence.md
    pub auto: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub pid: u32,
    /// Who it is, for people: `Laravel Herd (nginx.exe)`, a program path, or
    /// `a program Kiln can't inspect`.
    pub who: String,
    /// The owner is Kiln's own Caddy.
    pub is_kiln: bool,
}

/// Other local PHP tools ([ADR 0019]).
///
/// [ADR 0019]: ../../../docs/adr/0019-coexistence.md
#[derive(Debug, Clone, Default)]
pub struct OtherToolsFacts {
    /// (tool, folder)
    pub installed: Vec<(String, PathBuf)>,
    /// (who, addresses): other programs listening on the web ports Kiln may use.
    pub listeners: Vec<(String, Vec<SocketAddr>)>,
    pub shared_names: Vec<SharedName>,
    /// (tool, path) when `php` on this terminal's PATH is another tool's.
    pub php_on_path: Option<(String, PathBuf)>,
    pub herd_installed: bool,
}

/// A Kiln site name another program also puts in the hosts file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedName {
    pub host: String,
    /// "Laravel Herd", or "another program".
    pub owner: String,
    pub kiln_url: String,
}

#[derive(Debug, Clone, Default)]
pub struct WebFacts {
    pub installed: bool,
    pub version: Option<String>,
    pub running_pid: Option<u32>,
    /// `Some(false)`: running, but the admin socket didn't answer.
    pub admin_answers: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct PhpFacts {
    /// (version, complete on disk)
    pub installed: Vec<(String, bool)>,
    pub default: Option<String>,
    /// (site, pin, version used instead)
    pub fallbacks: Vec<(String, String, Option<String>)>,
    pub supervisor_pid: Option<u32>,
    /// (pool id, [(port, listening)])
    pub pools: Vec<(String, Vec<(u16, bool)>)>,
    pub recent_errors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SiteFacts {
    pub sites: Vec<SiteDns>,
    /// (folder, reason)
    pub skipped: Vec<(PathBuf, String)>,
    /// Names in Kiln's hosts block that are no longer sites.
    pub stale_hosts: Vec<String>,
}

/// How a Linux machine resolves names ([ADR 0020] §2).
///
/// [ADR 0020]: ../../../docs/adr/0020-linux-dns-and-privilege.md
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolverFacts {
    /// The `hosts:` line of `/etc/nsswitch.conf`, without the key; `None` if absent.
    pub nsswitch_hosts: Option<String>,
    /// Who manages `/etc/resolv.conf`: `"systemd-resolved"`, `"NetworkManager"`, or `None`.
    pub resolv_conf_manager: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteDns {
    pub host: String,
    pub in_hosts_block: bool,
    /// What the name resolves to; empty if it doesn't resolve.
    pub resolves_to: Vec<IpAddr>,
}

#[derive(Debug, Clone, Default)]
pub struct CertFacts {
    pub root_present: bool,
    /// `None` where trust can't be checked (not Windows).
    pub trusted: Option<bool>,
    pub intermediate_days_left: Option<i64>,
    /// Firefox profiles found (Firefox may keep its own trust store).
    pub firefox_profiles: usize,
    /// How the root key is kept, e.g. "in your desktop keyring (Secret Service)…".
    pub key_storage: Option<String>,
    /// Linux ([ADR 0021] §8): each browser NSS database found, and whether it trusts the
    /// root (`None`: couldn't tell, e.g. no `certutil`).
    ///
    /// [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md
    pub nss: Vec<(String, Option<bool>)>,
    /// Linux: whether `certutil` is installed; `None` elsewhere.
    pub certutil: Option<bool>,
    /// Linux: whether the system store has Kiln's root; `None` elsewhere.
    pub system_trusted: Option<bool>,
    /// The web server keeps site certificates issued from an earlier intermediate (I-57).
    pub stale_site_certificates: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ShimFacts {
    pub installed: bool,
    pub shims_dir: PathBuf,
    /// Where `php` resolves on this terminal's PATH.
    pub php_on_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------------------------

/// All findings for `facts`, in report order.
#[must_use]
pub fn diagnose(facts: &Facts) -> Report {
    let mut findings = Vec::new();
    findings.extend(state(&facts.state));
    findings.extend(environment(&facts.ancestors));
    findings.extend(ports(&facts.ports));
    findings.extend(web(&facts.web));
    findings.extend(php(&facts.php));
    findings.extend(sites(&facts.sites));
    findings.extend(dns(&facts.sites, facts.resolver.as_ref()));
    if let Some(certs) = &facts.certificates {
        findings.extend(certificates(certs));
    }
    findings.extend(shims(&facts.shims));
    findings.extend(other_tools(&facts.others));
    findings.sort_by_key(|f| f.area);
    Report { findings }
}

fn state(facts: &StateFacts) -> Vec<Finding> {
    let area = Area::State;
    if let Some(e) = &facts.open_error {
        return vec![finding(
            area,
            Status::Fail,
            format!("the state database can't be opened: {e}"),
        )];
    }
    let mut out = vec![finding(
        area,
        Status::Ok,
        format!(
            "database schema v{} (this Kiln supports v{})",
            facts.schema, facts.supported
        ),
    )];
    for (id, kind, state) in &facts.unfinished {
        out.push(with_fix(
            finding(
                area,
                Status::Warn,
                format!("operation #{id} ({kind}) was left {state}"),
            ),
            format!("kiln undo {id}"),
        ));
    }
    out
}

/// Programs installed from the Microsoft Store / MSIX live here; processes they start may see
/// a redirected `AppData` ([I-45]).
///
/// [I-45]: ../../../docs/issues.md#i-45
fn environment(ancestors: &[PathBuf]) -> Vec<Finding> {
    // Packaged apps live in a `WindowsApps` folder, on whichever drive apps are installed to
    // (Windows reports the real path, e.g. `E:\WindowsApps\…`, not `C:\Program Files\…`).
    // A program running from a package's redirected AppData (`…\Packages\<id>\LocalCache\…`)
    // is inside that container too.
    let packaged = ancestors.iter().find(|image| {
        let path = image.to_string_lossy().to_ascii_lowercase();
        path.contains(r"\windowsapps\")
            || (path.contains(r"\appdata\local\packages\") && path.contains(r"\localcache\"))
    });
    match packaged {
        Some(image) => vec![with_fix(
            finding(
                Area::Environment,
                Status::Warn,
                format!(
                    "kiln runs under a packaged app ({}); Windows may redirect its AppData, so Kiln's state here can differ from a normal terminal's",
                    image.display()
                ),
            ),
            "run kiln from a normal terminal (Windows Terminal or PowerShell from the Start menu)",
        )],
        None => vec![finding(
            Area::Environment,
            Status::Ok,
            "normal terminal (no packaged-app container)",
        )],
    }
}

fn ports(facts: &[PortFacts]) -> Vec<Finding> {
    let area = Area::Ports;
    facts
        .iter()
        .map(|port| match &port.owner {
            None => finding(
                area,
                Status::Ok,
                format!("{} {} is free", port.purpose, port.addr),
            ),
            Some(owner) if owner.is_kiln => finding(
                area,
                Status::Ok,
                format!(
                    "{} {} is Kiln's web server (PID {})",
                    port.purpose, port.addr, owner.pid
                ),
            ),
            Some(owner) if port.auto => finding(
                area,
                Status::Info,
                format!(
                    "{} {} is held by {}, PID {}; kiln web start picks a free port instead",
                    port.purpose, port.addr, owner.who, owner.pid
                ),
            ),
            Some(owner) => {
                let flag = if port.purpose == "HTTP" {
                    "--http-port"
                } else {
                    "--https-port"
                };
                with_fix(
                    finding(
                        area,
                        Status::Warn,
                        format!(
                            "{} {} (your choice) is held by {}, PID {}",
                            port.purpose, port.addr, owner.who, owner.pid
                        ),
                    ),
                    format!("kiln web start --auto-ports  (or {flag} <port>)"),
                )
            }
        })
        .collect()
}

fn web(facts: &WebFacts) -> Vec<Finding> {
    let area = Area::Web;
    if !facts.installed {
        return vec![with_fix(
            finding(area, Status::Warn, "Caddy isn't installed"),
            "kiln web install",
        )];
    }
    let version = facts.version.as_deref().unwrap_or("?");
    let mut out = vec![finding(
        area,
        Status::Ok,
        format!("Caddy {version} installed"),
    )];
    match (facts.running_pid, facts.admin_answers) {
        (None, _) => out.push(with_fix(
            finding(area, Status::Info, "the web server isn't running"),
            "kiln web start",
        )),
        (Some(pid), Some(false)) => out.push(with_fix(
            finding(
                area,
                Status::Fail,
                format!("running (PID {pid}), but its admin socket doesn't answer, so changes can't be applied"),
            ),
            "kiln web restart",
        )),
        (Some(pid), _) => out.push(finding(area, Status::Ok, format!("running (PID {pid})"))),
    }
    out
}

fn php(facts: &PhpFacts) -> Vec<Finding> {
    let area = Area::Php;
    let mut out = Vec::new();
    if facts.installed.is_empty() {
        return vec![with_fix(
            finding(
                area,
                Status::Info,
                "no PHP installed; sites are served as static files",
            ),
            "kiln php install 8.4",
        )];
    }
    for (version, complete) in &facts.installed {
        out.push(if *complete {
            finding(area, Status::Ok, format!("PHP {version} installed"))
        } else {
            with_fix(
                finding(
                    area,
                    Status::Fail,
                    format!("PHP {version} is incomplete on disk"),
                ),
                format!("kiln php install {version}"),
            )
        });
    }
    if let Some(default) = &facts.default {
        out.push(finding(
            area,
            Status::Info,
            format!("default for sites: PHP {default}"),
        ));
    }
    for (site, pin, used) in &facts.fallbacks {
        out.push(with_fix(
            finding(
                area,
                Status::Warn,
                format!(
                    "{site} is pinned to PHP {pin}, which isn't installed; it uses {} instead",
                    used.as_deref().unwrap_or("nothing")
                ),
            ),
            format!("kiln php install {pin}  (or: kiln unisolate {site})"),
        ));
    }
    match facts.supervisor_pid {
        None if !facts.pools.is_empty() => out.push(with_fix(
            finding(
                area,
                Status::Fail,
                "PHP pools are configured but the supervisor isn't running",
            ),
            "kiln web restart",
        )),
        None => {}
        Some(pid) => {
            for (pool, workers) in &facts.pools {
                let down: Vec<String> = workers
                    .iter()
                    .filter(|(_, listening)| !listening)
                    .map(|(port, _)| port.to_string())
                    .collect();
                out.push(if down.is_empty() {
                    finding(
                        area,
                        Status::Ok,
                        format!(
                            "pool {pool}: {} workers listening (supervisor PID {pid})",
                            workers.len()
                        ),
                    )
                } else {
                    with_fix(
                        finding(
                            area,
                            Status::Warn,
                            format!(
                                "pool {pool}: workers on port(s) {} aren't listening",
                                down.join(", ")
                            ),
                        ),
                        "see supervisor.log and php-errors.log in %LOCALAPPDATA%\\Kiln\\supervisor",
                    )
                });
            }
        }
    }
    if !facts.recent_errors.is_empty() {
        out.push(finding(
            area,
            Status::Info,
            format!(
                "recent php-errors.log lines:\n{}",
                facts.recent_errors.join("\n")
            ),
        ));
    }
    out
}

fn sites(facts: &SiteFacts) -> Vec<Finding> {
    let area = Area::Sites;
    let mut out = vec![if facts.sites.is_empty() {
        with_fix(
            finding(area, Status::Info, "no sites yet"),
            "kiln link (in a project) or kiln park (in a folder of projects)",
        )
    } else {
        finding(area, Status::Ok, format!("{} site(s)", facts.sites.len()))
    }];
    for (folder, reason) in &facts.skipped {
        out.push(finding(
            area,
            Status::Info,
            format!("skipped {}: {reason}", folder.display()),
        ));
    }
    out
}

fn dns(facts: &SiteFacts, resolver: Option<&ResolverFacts>) -> Vec<Finding> {
    let area = Area::Dns;
    let mut out = Vec::new();
    let missing: Vec<&str> = facts
        .sites
        .iter()
        .filter(|s| !s.in_hosts_block)
        .map(|s| s.host.as_str())
        .collect();
    if !missing.is_empty() {
        out.push(with_fix(
            finding(
                area,
                Status::Warn,
                format!("not in the hosts file: {}", missing.join(", ")),
            ),
            "kiln hosts sync",
        ));
    }
    for site in facts.sites.iter().filter(|s| s.in_hosts_block) {
        if site.resolves_to.is_empty() {
            out.push(finding(
                area,
                Status::Warn,
                format!("{} is in the hosts file but doesn't resolve (DNS client cache, or a VPN/DNS-over-HTTPS setting)", site.host),
            ));
        } else if site.resolves_to.iter().any(|ip| !ip.is_loopback()) {
            let ips: Vec<String> = site.resolves_to.iter().map(ToString::to_string).collect();
            out.push(finding(
                area,
                Status::Fail,
                format!(
                    "{} resolves to {}, not this computer",
                    site.host,
                    ips.join(", ")
                ),
            ));
        }
    }
    if !facts.stale_hosts.is_empty() {
        out.push(with_fix(
            finding(
                area,
                Status::Info,
                format!(
                    "in the hosts file but no longer sites: {}",
                    facts.stale_hosts.join(", ")
                ),
            ),
            "kiln hosts sync",
        ));
    }
    if out.is_empty() && !facts.sites.is_empty() {
        out.push(finding(
            area,
            Status::Ok,
            "every site resolves to this computer",
        ));
    }
    if let Some(resolver) = resolver {
        out.extend(resolver_findings(resolver));
    }
    out
}

/// Whether the `hosts:` line of `nsswitch.conf` consults `/etc/hosts` (`files`) before any DNS
/// source (`dns`, `resolve`). `[...]` actions are ignored.
#[must_use]
pub fn hosts_file_first(nsswitch_hosts: &str) -> bool {
    let sources: Vec<&str> = nsswitch_hosts
        .split_whitespace()
        .filter(|s| !s.starts_with('['))
        .collect();
    let position = |name: &str| sources.iter().position(|s| *s == name);
    match position("files") {
        None => false,
        Some(files) => ["dns", "resolve"]
            .iter()
            .filter_map(|dns| position(dns))
            .all(|dns| files < dns),
    }
}

fn resolver_findings(facts: &ResolverFacts) -> Vec<Finding> {
    let area = Area::Dns;
    let mut out = Vec::new();
    match facts.nsswitch_hosts.as_deref() {
        Some(line) if !hosts_file_first(line) => out.push(with_fix(
            finding(
                area,
                Status::Warn,
                format!("/etc/nsswitch.conf has `hosts: {line}`, so /etc/hosts, where Kiln's names are, isn't consulted first"),
            ),
            "put `files` first on the hosts: line of /etc/nsswitch.conf",
        )),
        Some(_) => out.push(finding(
            area,
            Status::Ok,
            "/etc/hosts is consulted before DNS (nsswitch.conf)",
        )),
        None => {}
    }
    if let Some(manager) = &facts.resolv_conf_manager {
        let also = if manager == "systemd-resolved" {
            ", which answers from /etc/hosts too"
        } else {
            ""
        };
        out.push(finding(
            area,
            Status::Info,
            format!("DNS is managed by {manager}{also}"),
        ));
    }
    out
}

/// Linux trust ([ADR 0021] §8): per browser NSS database, `certutil`, and the system store.
///
/// [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md
fn linux_trust(facts: &CertFacts, certutil: bool) -> Vec<Finding> {
    let area = Area::Certificates;
    let mut out = Vec::new();
    if facts.nss.is_empty() {
        out.push(finding(
            area,
            Status::Info,
            "no browser certificate database found (Firefox, or a Chromium browser started once)",
        ));
    } else if !certutil {
        out.push(with_fix(
            finding(
                area,
                Status::Warn,
                "certutil isn't installed, so Kiln can't make Chrome or Firefox trust its CA",
            ),
            "sudo apt install libnss3-tools  (Fedora: nss-tools; Arch: nss), then kiln ca trust",
        ));
    }
    for (label, trusted) in &facts.nss {
        match trusted {
            Some(true) => out.push(finding(
                area,
                Status::Ok,
                format!("{label} trusts Kiln's CA"),
            )),
            Some(false) => out.push(with_fix(
                finding(
                    area,
                    Status::Warn,
                    format!("{label} doesn't trust Kiln's CA; it will warn on HTTPS sites"),
                ),
                "kiln ca trust",
            )),
            None => {}
        }
    }
    match facts.system_trusted {
        Some(true) => out.push(finding(
            area,
            Status::Ok,
            "the system store trusts Kiln's CA (curl, PHP, Node, git)",
        )),
        Some(false) => out.push(with_fix(
            finding(
                area,
                Status::Info,
                "command-line tools (curl, PHP, Node, git) don't trust Kiln's CA; browsers don't need this",
            ),
            "kiln ca trust --system",
        )),
        None => {}
    }
    out
}

fn certificates(facts: &CertFacts) -> Vec<Finding> {
    let area = Area::Certificates;
    if !facts.root_present {
        return vec![with_fix(
            finding(
                area,
                Status::Fail,
                "a site uses HTTPS but Kiln's CA doesn't exist",
            ),
            "kiln ca init && kiln ca trust",
        )];
    }
    let mut out = match (facts.trusted, facts.certutil) {
        (Some(true), _) => vec![finding(
            area,
            Status::Ok,
            "Kiln's root CA is trusted for your account",
        )],
        (Some(false), _) => vec![with_fix(
            finding(
                area,
                Status::Warn,
                "Kiln's root CA isn't trusted; browsers will warn on HTTPS sites",
            ),
            "kiln ca trust",
        )],
        (None, Some(certutil)) => linux_trust(facts, certutil),
        (None, None) => vec![finding(
            area,
            Status::Info,
            "trust can't be checked on this platform yet",
        )],
    };
    if facts.stale_site_certificates {
        out.push(with_fix(
            finding(
                area,
                Status::Warn,
                "the web server still has site certificates from an earlier Kiln CA; browsers trusting the current one reject them",
            ),
            "kiln web restart",
        ));
    }
    if let Some(kept) = &facts.key_storage {
        out.push(finding(
            area,
            if kept.contains("file permissions only") {
                Status::Info
            } else {
                Status::Ok
            },
            format!("the root key is {kept}"),
        ));
    }
    match facts.intermediate_days_left {
        Some(days) if days < 10 => out.push(with_fix(
            finding(
                area,
                Status::Warn,
                format!("the web server's intermediate certificate expires in {days} day(s)"),
            ),
            "kiln web restart  (renews it)",
        )),
        Some(days) => out.push(finding(
            area,
            Status::Ok,
            format!("intermediate certificate valid for {days} more days"),
        )),
        None => out.push(with_fix(
            finding(area, Status::Warn, "no intermediate certificate yet"),
            "kiln web start  (issues it)",
        )),
    }
    // Windows-only notes; Linux reports each browser's own database instead.
    if facts.certutil.is_some() {
        return out;
    }
    out.push(finding(
        area,
        Status::Info,
        "curl.exe and Git on Windows (Schannel) refuse local CAs with \"revocation status unknown\"; add --ssl-revoke-best-effort (curl) or http.schannelCheckRevoke=false (git) for .test URLs",
    ));
    if facts.firefox_profiles > 0 {
        out.push(finding(
            area,
            Status::Info,
            format!(
                "Firefox found ({} profile(s)); if it warns on .test HTTPS sites, set security.enterprise_roots.enabled = true in about:config so it uses the Windows trust store",
                facts.firefox_profiles
            ),
        ));
    }
    out
}

fn shims(facts: &ShimFacts) -> Vec<Finding> {
    let area = Area::Shims;
    if !facts.installed {
        return vec![with_fix(
            finding(area, Status::Info, "php/composer shims aren't installed"),
            "kiln shims install",
        )];
    }
    match &facts.php_on_path {
        Some(php) if php.starts_with(&facts.shims_dir) => {
            vec![finding(
                area,
                Status::Ok,
                "`php` on this terminal's PATH is Kiln's shim",
            )]
        }
        Some(php) => vec![with_fix(
            finding(
                area,
                Status::Warn,
                format!(
                    "`php` on this terminal's PATH is {}, not Kiln's shim",
                    php.display()
                ),
            ),
            "open a new terminal; if it persists: kiln shims install",
        )],
        None => vec![with_fix(
            finding(
                area,
                Status::Warn,
                "the shims are installed but `php` isn't on this terminal's PATH",
            ),
            "open a new terminal; if it persists: kiln shims install",
        )],
    }
}

/// Never a failure: other tools are fine to have. Only a shared site name is a warning, since
/// it can open the wrong site ([ADR 0019] §4–5).
///
/// [ADR 0019]: ../../../docs/adr/0019-coexistence.md
fn other_tools(facts: &OtherToolsFacts) -> Vec<Finding> {
    let area = Area::OtherTools;
    let mut out = Vec::new();
    for (tool, folder) in &facts.installed {
        out.push(finding(
            area,
            Status::Info,
            format!("{tool} is installed ({})", folder.display()),
        ));
    }
    for (who, addrs) in &facts.listeners {
        let addrs: Vec<String> = addrs.iter().map(ToString::to_string).collect();
        out.push(finding(
            area,
            Status::Info,
            format!("in use by {who}: {}", addrs.join(", ")),
        ));
    }
    for shared in &facts.shared_names {
        out.push(with_fix(
            finding(
                area,
                Status::Warn,
                format!(
                    "{} is also in the hosts file for {}; both point to this computer, so the port decides which server answers",
                    shared.host, shared.owner
                ),
            ),
            format!(
                "open Kiln's site at {}, or link the project under another name (kiln link <name>)",
                shared.kiln_url
            ),
        ));
    }
    if let Some((tool, php)) = &facts.php_on_path {
        out.push(with_fix(
            finding(
                area,
                Status::Info,
                format!(
                    "`php` on this terminal's PATH is {tool}'s ({})",
                    php.display()
                ),
            ),
            "kiln shims install  (puts Kiln's php first; kiln undo reverses it)",
        ));
    }
    if facts.herd_installed {
        out.push(finding(
            area,
            Status::Info,
            "Laravel Herd stops every nginx.exe when it starts; Kiln's server is kiln-caddy.exe, so it isn't affected",
        ));
    }
    if out.is_empty() {
        out.push(finding(
            area,
            Status::Ok,
            "no other local PHP tools found (Herd, XAMPP, Laragon, WampServer)",
        ));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    #[must_use]
    pub fn worst(&self) -> Status {
        self.findings
            .iter()
            .map(|f| f.status)
            .max()
            .unwrap_or(Status::Ok)
    }

    /// Plain text, grouped by area.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        let mut area = None;
        for f in &self.findings {
            if area != Some(f.area) {
                area = Some(f.area);
                let _ = writeln!(text, "{}", f.area.title());
            }
            let mark = match f.status {
                Status::Ok => "  ok  ",
                Status::Info => "  --  ",
                Status::Warn => "  !!  ",
                Status::Fail => "  XX  ",
            };
            let mut lines = f.message.lines();
            let _ = writeln!(text, "{mark}{}", lines.next().unwrap_or(""));
            for line in lines {
                let _ = writeln!(text, "        {line}");
            }
            if let Some(fix) = &f.fix {
                let _ = writeln!(text, "        → {fix}");
            }
        }
        let (warns, fails) = self
            .findings
            .iter()
            .fold((0, 0), |(w, x), f| match f.status {
                Status::Warn => (w + 1, x),
                Status::Fail => (w, x + 1),
                _ => (w, x),
            });
        let _ = writeln!(
            text,
            "\n{fails} problem(s), {warns} warning(s). kiln doctor changes nothing."
        );
        text
    }

    /// The findings as pretty JSON, for scripts and the GUI.
    ///
    /// # Panics
    ///
    /// Never: findings hold only strings and enums.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("findings always serialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn of(report: &Report, area: Area) -> Vec<&Finding> {
        report.findings.iter().filter(|f| f.area == area).collect()
    }

    fn base() -> Facts {
        Facts {
            state: StateFacts {
                open_error: None,
                schema: 5,
                supported: 5,
                unfinished: Vec::new(),
            },
            web: WebFacts {
                installed: true,
                version: Some("2.11.7".into()),
                ..WebFacts::default()
            },
            ..Facts::default()
        }
    }

    #[test]
    fn a_healthy_idle_setup_has_no_warnings() {
        let report = diagnose(&base());
        assert!(report.worst() <= Status::Info, "{}", report.render());
        assert!(report.render().contains("kiln doctor changes nothing"));
    }

    #[test]
    fn half_done_operations_point_at_undo() {
        let mut facts = base();
        facts.state.unfinished = vec![(4, "dns.hosts.block".into(), "applying".into())];
        let report = diagnose(&facts);
        let f = of(&report, Area::State)[1];
        assert_eq!(f.status, Status::Warn);
        assert_eq!(f.fix.as_deref(), Some("kiln undo 4"));
    }

    #[test]
    fn a_packaged_app_ancestor_is_flagged() {
        let mut facts = base();
        facts.ancestors = vec![
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            PathBuf::from(
                r"C:\Program Files\WindowsApps\Claude_3.1623.0.0_x64__pzs8sxrjxfjjc\app\Claude.exe",
            ),
        ];
        let report = diagnose(&facts);
        let env = of(&report, Area::Environment);
        assert_eq!(env[0].status, Status::Warn);
        assert!(env[0].message.contains("Claude.exe"));

        // What Windows actually reports when apps are installed on another drive, and a
        // program running from a package's redirected AppData (both seen on the dev machine).
        for image in [
            r"E:\WindowsApps\Claude_3.1623.1.0_x64__pzs8sxrjxfjjc\app\claude.exe",
            r"E:\WpSystem\S-1-5-21-1\AppData\Local\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude-code\claude.exe",
        ] {
            facts.ancestors = vec![PathBuf::from(image)];
            let report = diagnose(&facts);
            assert_eq!(
                of(&report, Area::Environment)[0].status,
                Status::Warn,
                "{image}"
            );
        }
        facts.ancestors = vec![PathBuf::from(r"C:\Windows\explorer.exe")];
        let report = diagnose(&facts);
        assert_eq!(of(&report, Area::Environment)[0].status, Status::Ok);
    }

    #[test]
    fn a_port_held_by_another_program_names_it_and_suggests_another_port() {
        let mut facts = base();
        facts.ports = vec![
            PortFacts {
                addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 80)),
                purpose: "HTTP",
                owner: Some(Owner {
                    pid: 4321,
                    who: "Laravel Herd (nginx.exe)".into(),
                    is_kiln: false,
                }),
                auto: false,
            },
            PortFacts {
                addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 8080)),
                purpose: "HTTP",
                owner: Some(Owner {
                    pid: 99,
                    who: "kiln-caddy.exe".into(),
                    is_kiln: true,
                }),
                auto: false,
            },
        ];
        let report = diagnose(&facts);
        let ports = of(&report, Area::Ports);
        assert_eq!(ports[0].status, Status::Warn);
        assert!(
            ports[0]
                .message
                .contains("Laravel Herd (nginx.exe), PID 4321")
        );
        assert!(ports[0].fix.as_deref().unwrap().contains("--auto-ports"));
        assert!(ports[0].fix.as_deref().unwrap().contains("--http-port"));
        assert_eq!(ports[1].status, Status::Ok);

        // A port Kiln picks itself just moves at the next start.
        facts.ports[0].auto = true;
        let report = diagnose(&facts);
        let ports = of(&report, Area::Ports);
        assert_eq!(ports[0].status, Status::Info);
        assert!(ports[0].message.contains("picks a free port"));
    }

    #[test]
    fn other_tools_are_reported_and_only_shared_names_warn() {
        let report = diagnose(&base());
        let others = of(&report, Area::OtherTools);
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].status, Status::Ok);

        let mut facts = base();
        facts.others = OtherToolsFacts {
            installed: vec![("Laravel Herd".into(), PathBuf::from("Herd"))],
            listeners: vec![(
                "Laravel Herd (nginx.exe), PID 812".into(),
                vec![SocketAddr::from((Ipv4Addr::LOCALHOST, 80))],
            )],
            shared_names: vec![SharedName {
                host: "blog.test".into(),
                owner: "Laravel Herd".into(),
                kiln_url: "http://blog.test:8080/".into(),
            }],
            php_on_path: Some(("Laravel Herd".into(), PathBuf::from("php.bat"))),
            herd_installed: true,
        };
        let report = diagnose(&facts);
        let others = of(&report, Area::OtherTools);
        assert_eq!(report.worst(), Status::Warn, "{}", report.render());
        let warns: Vec<_> = others.iter().filter(|f| f.status == Status::Warn).collect();
        assert_eq!(warns.len(), 1);
        assert!(warns[0].message.contains("blog.test"));
        assert!(
            warns[0]
                .fix
                .as_deref()
                .unwrap()
                .contains("http://blog.test:8080/")
        );
        assert!(others.iter().any(|f| f.message.contains("127.0.0.1:80")));
        assert!(others.iter().any(|f| f.message.contains("kiln-caddy.exe")));
        assert!(others.iter().any(|f| f.message.contains("php.bat")));
        assert!(report.render().contains("Other tools\n"));
    }

    #[test]
    fn php_problems_are_reported_with_fixes() {
        let mut facts = base();
        facts.php = PhpFacts {
            installed: vec![("8.3.36".into(), true), ("8.4.27".into(), false)],
            default: Some("8.3.36".into()),
            fallbacks: vec![("legacy.test".into(), "8.1".into(), Some("8.3.36".into()))],
            supervisor_pid: Some(7),
            pools: vec![("8.3.36".into(), vec![(9101, true), (9102, false)])],
            recent_errors: vec!["PHP Warning: x".into()],
        };
        let report = diagnose(&facts);
        let php = of(&report, Area::Php);
        let worst = php.iter().map(|f| f.status).max().unwrap();
        assert_eq!(worst, Status::Fail, "incomplete 8.4.27");
        assert!(php.iter().any(|f| f.message.contains("pinned to PHP 8.1")));
        assert!(php.iter().any(|f| f.message.contains("port(s) 9102")));

        facts.php.supervisor_pid = None;
        let report = diagnose(&facts);
        assert!(
            of(&report, Area::Php)
                .iter()
                .any(|f| f.message.contains("supervisor isn't running"))
        );
    }

    #[test]
    fn dns_problems_are_told_apart() {
        let mut facts = base();
        let lo = IpAddr::from(Ipv4Addr::LOCALHOST);
        facts.sites.sites = vec![
            SiteDns {
                host: "ok.test".into(),
                in_hosts_block: true,
                resolves_to: vec![lo],
            },
            SiteDns {
                host: "new.test".into(),
                in_hosts_block: false,
                resolves_to: vec![],
            },
            SiteDns {
                host: "vpn.test".into(),
                in_hosts_block: true,
                resolves_to: vec![],
            },
            SiteDns {
                host: "hijack.test".into(),
                in_hosts_block: true,
                resolves_to: vec![IpAddr::from([10, 0, 0, 5])],
            },
        ];
        facts.sites.stale_hosts = vec!["old.test".into()];
        let report = diagnose(&facts);
        let dns = of(&report, Area::Dns);
        assert!(
            dns.iter()
                .any(|f| f.message.contains("new.test")
                    && f.fix.as_deref() == Some("kiln hosts sync"))
        );
        assert!(
            dns.iter()
                .any(|f| f.message.contains("vpn.test") && f.status == Status::Warn)
        );
        assert!(
            dns.iter()
                .any(|f| f.message.contains("10.0.0.5") && f.status == Status::Fail)
        );
        assert!(dns.iter().any(|f| f.message.contains("old.test")));
        assert!(!dns.iter().any(|f| f.message.contains("ok.test")));
    }

    #[test]
    fn linux_reports_each_browser_database_and_the_system_store() {
        let mut facts = base();
        facts.certificates = Some(CertFacts {
            root_present: true,
            intermediate_days_left: Some(20),
            key_storage: Some("in your desktop keyring (Secret Service)".into()),
            nss: vec![
                ("Chrome and other Chromium browsers".into(), Some(true)),
                ("Firefox (snap) profile x.default".into(), Some(false)),
            ],
            certutil: Some(true),
            system_trusted: Some(false),
            ..CertFacts::default()
        });
        let report = diagnose(&facts);
        let certs = of(&report, Area::Certificates);
        let warns: Vec<_> = certs.iter().filter(|f| f.status == Status::Warn).collect();
        assert_eq!(warns.len(), 1, "{}", report.render());
        assert!(warns[0].message.contains("Firefox (snap)"));
        assert_eq!(warns[0].fix.as_deref(), Some("kiln ca trust"));
        assert!(
            certs
                .iter()
                .any(|f| f.fix.as_deref() == Some("kiln ca trust --system"))
        );
        assert!(certs.iter().any(|f| f.message.contains("desktop keyring")));
        assert!(
            !certs.iter().any(|f| f.message.contains("Schannel")),
            "Windows notes stay on Windows"
        );

        // Site certificates from an earlier CA (I-57) are reported, and only then.
        assert!(
            !certs.iter().any(|f| f.message.contains("earlier Kiln CA")),
            "no stale certificates reported unless there are some"
        );
        facts.certificates.as_mut().unwrap().stale_site_certificates = true;
        assert!(
            of(&diagnose(&facts), Area::Certificates)
                .iter()
                .any(|f| f.fix.as_deref() == Some("kiln web restart"))
        );

        // Without certutil the browsers can't be checked or changed: one clear warning.
        let linux = facts.certificates.as_mut().unwrap();
        linux.certutil = Some(false);
        linux.nss = vec![("Firefox profile y".into(), None)];
        let report = diagnose(&facts);
        let certs = of(&report, Area::Certificates);
        assert!(certs.iter().any(|f| {
            f.status == Status::Warn
                && f.fix
                    .as_deref()
                    .is_some_and(|fix| fix.contains("libnss3-tools"))
        }));
    }

    #[test]
    fn https_sites_get_trust_expiry_and_the_schannel_note() {
        let mut facts = base();
        facts.certificates = Some(CertFacts {
            root_present: true,
            trusted: Some(false),
            intermediate_days_left: Some(3),
            firefox_profiles: 1,
            ..CertFacts::default()
        });
        let report = diagnose(&facts);
        let certs = of(&report, Area::Certificates);
        assert!(
            certs
                .iter()
                .any(|f| f.fix.as_deref() == Some("kiln ca trust"))
        );
        assert!(certs.iter().any(|f| f.message.contains("expires in 3 day")));
        assert!(
            certs
                .iter()
                .any(|f| f.message.contains("--ssl-revoke-best-effort"))
        );
        assert!(certs.iter().any(|f| f.message.contains("enterprise_roots")));
    }

    #[test]
    fn shims_report_what_php_resolves_to() {
        let mut facts = base();
        // Built with `join`, so the paths are real paths on every OS the tests run on.
        let shims_dir = PathBuf::from("kiln").join("bin").join("shims");
        facts.shims = ShimFacts {
            installed: true,
            shims_dir: shims_dir.clone(),
            php_on_path: Some(PathBuf::from("herd").join("bin").join("php.bat")),
        };
        let report = diagnose(&facts);
        assert_eq!(of(&report, Area::Shims)[0].status, Status::Warn);
        facts.shims.php_on_path = Some(shims_dir.join("php.exe"));
        assert_eq!(of(&diagnose(&facts), Area::Shims)[0].status, Status::Ok);
    }

    #[test]
    fn nsswitch_order_decides_whether_etc_hosts_is_read_first() {
        assert!(hosts_file_first(
            "files mdns4_minimal [NOTFOUND=return] dns"
        ));
        assert!(hosts_file_first(
            "files myhostname resolve [!UNAVAIL=return] dns"
        ));
        assert!(hosts_file_first("files"));
        assert!(!hosts_file_first("resolve [!UNAVAIL=return] files dns"));
        assert!(!hosts_file_first("dns files"));
        assert!(!hosts_file_first("mdns4_minimal dns"));

        let mut facts = base();
        facts.resolver = Some(ResolverFacts {
            nsswitch_hosts: Some("dns files".into()),
            resolv_conf_manager: Some("systemd-resolved".into()),
        });
        let report = diagnose(&facts);
        let dns = of(&report, Area::Dns);
        assert!(
            dns.iter()
                .any(|f| f.status == Status::Warn && f.message.contains("dns files"))
        );
        assert!(dns.iter().any(|f| f.message.contains("systemd-resolved")));

        facts.resolver.as_mut().unwrap().nsswitch_hosts =
            Some("files mdns4_minimal [NOTFOUND=return] dns".into());
        assert!(diagnose(&facts).worst() <= Status::Info);
    }

    #[test]
    fn report_renders_grouped_text_and_json() {
        let mut facts = base();
        facts.state.open_error = Some("locked".into());
        let report = diagnose(&facts);
        assert_eq!(report.worst(), Status::Fail);
        let text = report.render();
        assert!(text.starts_with("State\n  XX  the state database can't be opened: locked"));
        let json: serde_json::Value = serde_json::from_str(&report.to_json()).unwrap();
        assert_eq!(json["findings"][0]["status"], "fail");
        assert_eq!(json["findings"][0]["area"], "state");
    }
}
