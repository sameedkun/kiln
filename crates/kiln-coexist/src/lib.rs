//! Living next to other local PHP tools ([ADR 0019]): which are installed, who holds a port,
//! which `.test` names they claim in the hosts file, and which free ports Kiln can use.
//!
//! Everything here is read-only: Kiln never stops, edits or reconfigures another tool.
//!
//! [ADR 0019]: ../../../docs/adr/0019-coexistence.md

use std::path::{Path, PathBuf};

use kiln_platform::hosts::{BEGIN_MARKER, END_MARKER};

/// HTTP ports `kiln web start` tries, in order, when the user hasn't chosen one.
pub const HTTP_PORTS: [u16; 5] = [80, 8080, 8000, 8081, 8888];
/// HTTPS ports `kiln web start` tries, in order, when the user hasn't chosen one.
pub const HTTPS_PORTS: [u16; 4] = [443, 8443, 4443, 9443];

/// A local PHP tool Kiln knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tool {
    Herd,
    Xampp,
    Laragon,
    Wamp,
}

impl Tool {
    pub const ALL: [Self; 4] = [Self::Herd, Self::Xampp, Self::Laragon, Self::Wamp];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Herd => "Laravel Herd",
            Self::Xampp => "XAMPP",
            Self::Laragon => "Laragon",
            Self::Wamp => "WampServer",
        }
    }

    /// Folder names that, anywhere in a program's path, mean the program belongs to this tool.
    fn folder_names(self) -> &'static [&'static str] {
        match self {
            Self::Herd => &["herd"],
            Self::Xampp => &["xampp"],
            Self::Laragon => &["laragon"],
            Self::Wamp => &["wamp64", "wamp"],
        }
    }

    /// The lines this tool puts around its own block in the hosts file, if it marks one.
    fn hosts_markers(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Herd => Some(("# Herd generated Hosts", "# End Herd generated Hosts")),
            Self::Xampp | Self::Laragon | Self::Wamp => None,
        }
    }
}

/// The well-known places a tool is installed, as `(tool, folder)`.
///
/// `program_files` is `%ProgramFiles%`, `home` the user's profile folder and `system_drive`
/// the root of `%SystemDrive%` (e.g. `C:\`).
#[must_use]
pub fn known_locations(
    program_files: Option<&Path>,
    home: Option<&Path>,
    system_drive: Option<&Path>,
) -> Vec<(Tool, PathBuf)> {
    let mut out = Vec::new();
    if let Some(dir) = program_files {
        out.push((Tool::Herd, dir.join("Herd")));
    }
    if let Some(dir) = home {
        out.push((Tool::Herd, dir.join(".config").join("herd")));
    }
    if let Some(root) = system_drive {
        out.push((Tool::Xampp, root.join("xampp")));
        out.push((Tool::Laragon, root.join("laragon")));
        out.push((Tool::Wamp, root.join("wamp64")));
        out.push((Tool::Wamp, root.join("wamp")));
    }
    out
}

/// The tools found in `locations` (see [`known_locations`]), each once, with the first folder
/// found for it.
#[must_use]
pub fn installed(locations: &[(Tool, PathBuf)]) -> Vec<(Tool, PathBuf)> {
    let mut out: Vec<(Tool, PathBuf)> = Vec::new();
    for (tool, dir) in locations {
        if dir.is_dir() && !out.iter().any(|(t, _)| t == tool) {
            out.push((*tool, dir.clone()));
        }
    }
    out
}

/// The tools installed on this machine, from their well-known folders (Windows only for now;
/// Linux detection is still to come; macOS comes with Phase 4b).
#[must_use]
pub fn installed_here() -> Vec<(Tool, PathBuf)> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let env_dir = |name| std::env::var_os(name).map(PathBuf::from);
    let system_drive = env_dir("SystemDrive").map(|d| {
        let mut root = d.into_os_string();
        root.push("\\");
        PathBuf::from(root)
    });
    installed(&known_locations(
        env_dir("ProgramFiles").as_deref(),
        env_dir("USERPROFILE").as_deref(),
        system_drive.as_deref(),
    ))
}

/// Which tool a program belongs to, from its path.
#[must_use]
pub fn tool_of(path: &Path) -> Option<Tool> {
    let text = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    let mut components = text.split('/');
    // Every folder in the path, not the file name: `C:\tools\herd.exe` isn't Herd's.
    components.next_back();
    let folders: Vec<&str> = components.collect();
    Tool::ALL.into_iter().find(|tool| {
        tool.folder_names()
            .iter()
            .any(|name| folders.contains(name))
    })
}

/// Who holds a port, for messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    /// `None` if the process can't be inspected.
    pub image: Option<PathBuf>,
    pub tool: Option<Tool>,
}

impl Holder {
    #[must_use]
    pub fn new(pid: u32, image: Option<PathBuf>) -> Self {
        let tool = image.as_deref().and_then(tool_of);
        Self { pid, image, tool }
    }

    fn file_name(&self) -> Option<String> {
        self.image
            .as_ref()
            .and_then(|i| i.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// A short name: `Laravel Herd (nginx.exe)`, `Apache httpd (C:\Apache24\bin\httpd.exe)`,
    /// `the Windows HTTP service (IIS / http.sys)`.
    #[must_use]
    pub fn describe(&self) -> String {
        if let Some(tool) = self.tool {
            return match self.file_name() {
                Some(file) => format!("{} ({file})", tool.name()),
                None => tool.name().to_owned(),
            };
        }
        // PID 4 is the System process; on port 80/443 that is http.sys, usually for IIS.
        if cfg!(windows) && self.pid == 4 {
            return "the Windows HTTP service (IIS / http.sys)".to_owned();
        }
        match (&self.image, self.file_name()) {
            (Some(image), Some(file)) => match program_kind(&file) {
                Some(kind) => format!("{kind} ({})", image.display()),
                None => image.display().to_string(),
            },
            _ => "a program Kiln can't inspect".to_owned(),
        }
    }
}

/// What a well-known server program is, from its file name: web servers, PHP, and the
/// database, cache and mail programs other tools run ([ADR 0037] §1).
///
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
fn program_kind(file: &str) -> Option<&'static str> {
    let stem = file
        .to_ascii_lowercase()
        .trim_end_matches(".exe")
        .to_owned();
    match stem.as_str() {
        "nginx" => Some("nginx"),
        "httpd" | "apache" | "apache2" => Some("Apache httpd"),
        "caddy" => Some("Caddy"),
        "php-cgi" | "php-fpm" | "php" => Some("PHP"),
        "w3wp" => Some("IIS"),
        "mysqld" => Some("MySQL"),
        "mariadbd" => Some("MariaDB"),
        "postgres" => Some("PostgreSQL"),
        "redis-server" => Some("Redis"),
        "valkey-server" => Some("Valkey"),
        "memurai" => Some("Memurai"),
        "garnetserver" => Some("Garnet"),
        "mailpit" => Some("Mailpit"),
        "mailhog" => Some("MailHog"),
        "com.docker.backend" | "docker-proxy" | "vpnkit" => Some("Docker"),
        "wslrelay" => Some("WSL"),
        _ => None,
    }
}

/// One of the usual ports of the services Kiln runs ([ADR 0035] §5).
///
/// [ADR 0035]: ../../../docs/adr/0035-services.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServicePort {
    pub port: u16,
    /// What normally listens there, for messages.
    pub what: &'static str,
    /// The [`SystemService`] kinds that use it.
    pub kinds: &'static [&'static str],
}

/// The usual ports of the services Kiln runs: `kiln doctor` reports who holds them.
pub const SERVICE_PORTS: [ServicePort; 5] = [
    ServicePort {
        port: 3306,
        what: "MySQL / MariaDB",
        kinds: &["MySQL", "MariaDB"],
    },
    ServicePort {
        port: 5432,
        what: "PostgreSQL",
        kinds: &["PostgreSQL"],
    },
    ServicePort {
        port: 6379,
        what: "Redis / Valkey",
        kinds: &["Redis", "Valkey", "Memurai", "Garnet"],
    },
    ServicePort {
        port: 1025,
        what: "SMTP (mail catcher)",
        kinds: &["Mailpit", "MailHog"],
    },
    ServicePort {
        port: 8025,
        what: "a mail catcher's web UI",
        kinds: &["Mailpit", "MailHog"],
    },
];

/// The usual-port entry for `port`, if it is one.
#[must_use]
pub fn service_port(port: u16) -> Option<&'static ServicePort> {
    SERVICE_PORTS.iter().find(|p| p.port == port)
}

/// A system service of a kind Kiln also runs: the Windows service or systemd unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemService {
    /// e.g. `MySQL80`, `postgresql@16-main.service`.
    pub name: String,
    /// e.g. "MySQL".
    pub kind: &'static str,
    pub running: bool,
}

/// What a system service is, from its name, if it is a database, cache or mail service:
/// `MySQL80`, `mysql.service`, `postgresql-x64-16`, `redis-server.service`, `Memurai`, …
#[must_use]
pub fn system_service_kind(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    [
        ("mariadb", "MariaDB"),
        ("mysql", "MySQL"),
        ("postgresql", "PostgreSQL"),
        ("redis", "Redis"),
        ("valkey", "Valkey"),
        ("memurai", "Memurai"),
        ("garnet", "Garnet"),
        ("mailpit", "Mailpit"),
        ("mailhog", "MailHog"),
    ]
    .into_iter()
    .find(|(prefix, _)| name.starts_with(prefix))
    .map(|(_, kind)| kind)
}

/// The database, cache and mail services in the output of `sc.exe query type= service
/// state= all`. Only the service names and the state codes are read, since the labels and
/// state words can be translated; 4 is "running" ([ADR 0037] §5).
///
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
#[must_use]
pub fn parse_sc_query(text: &str) -> Vec<SystemService> {
    let mut out: Vec<SystemService> = Vec::new();
    let mut current: Option<(String, &'static str)> = None;
    let mut block_start = true;
    for line in text.lines() {
        if line.trim().is_empty() {
            block_start = true;
            continue;
        }
        let Some((_, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if !line.starts_with(char::is_whitespace) {
            // A block's first line is the service name, whatever its label says; the
            // display name follows, also unindented.
            if block_start {
                current = system_service_kind(value).map(|kind| (value.to_owned(), kind));
                block_start = false;
            }
            continue;
        }
        // The state line is the one whose value starts with a one-digit code (1–7); the
        // type line's codes have two or more digits, and the exit codes come after it.
        let mut words = value.split_whitespace();
        if let (Some((name, kind)), Some(code)) = (&current, words.next())
            && code.len() == 1
            && let Ok(code) = code.parse::<u8>()
            && (1..=7).contains(&code)
        {
            out.push(SystemService {
                name: name.clone(),
                kind,
                running: code == 4,
            });
            current = None;
        }
    }
    out
}

/// The database, cache and mail services in the output of `systemctl list-units
/// --type=service --all --plain --no-legend --no-pager`: unit, load, active, sub, description.
#[must_use]
pub fn parse_systemctl_units(text: &str) -> Vec<SystemService> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (unit, sub) = (fields.first()?, fields.get(3)?);
            let kind = system_service_kind(unit)?;
            Some(SystemService {
                name: (*unit).to_owned(),
                kind,
                running: *sub == "running",
            })
        })
        .collect()
}

/// A name another program put in the hosts file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub name: String,
    /// The tool whose marked block holds it; `None` if it isn't in a block Kiln recognizes.
    pub tool: Option<Tool>,
}

impl Claim {
    /// Who claims the name, for messages.
    #[must_use]
    pub fn owner(&self) -> &'static str {
        self.tool.map_or("another program", Tool::name)
    }
}

/// Every name in the hosts file outside Kiln's own block, lowercased, each once, attributed to
/// the tool whose marked block it is in.
#[must_use]
pub fn hosts_claims(text: &[u8]) -> Vec<Claim> {
    let text = String::from_utf8_lossy(text);
    let mut claims: Vec<Claim> = Vec::new();
    let mut in_kiln = false;
    let mut in_tool: Option<Tool> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == BEGIN_MARKER {
            in_kiln = true;
            continue;
        }
        if line == END_MARKER {
            in_kiln = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            let comment = format!("#{rest}");
            for tool in Tool::ALL {
                if let Some((begin, end)) = tool.hosts_markers() {
                    if comment.starts_with(end) {
                        in_tool = None;
                    } else if comment.starts_with(begin) {
                        in_tool = Some(tool);
                    }
                }
            }
            continue;
        }
        if in_kiln {
            continue;
        }
        let line = line.split('#').next().unwrap_or_default();
        // `<ip> <name> [<alias> …]`
        for name in line.split_whitespace().skip(1) {
            let name = name.to_ascii_lowercase();
            if !claims.iter().any(|c| c.name == name) {
                claims.push(Claim {
                    name,
                    tool: in_tool,
                });
            }
        }
    }
    claims
}

/// The first port in `preferred` that `is_free` accepts.
#[must_use]
pub fn pick(preferred: &[u16], is_free: impl Fn(u16) -> bool) -> Option<u16> {
    preferred.iter().copied().find(|&port| is_free(port))
}

/// Listening TCP sockets in the text of Linux's `/proc/net/tcp` or `/proc/net/tcp6`, as
/// `(address, socket inode)`. The inode links a socket to the process holding it, through
/// `/proc/<pid>/fd`.
#[must_use]
pub fn parse_proc_net_tcp(text: &str) -> Vec<(std::net::SocketAddr, u64)> {
    // The kernel prints each 32-bit word of an address in host byte order; Kiln runs on
    // little-endian CPUs (x86-64, AArch64).
    fn word(hex: &str) -> Option<[u8; 4]> {
        u32::from_str_radix(hex, 16).ok().map(u32::to_le_bytes)
    }
    fn address(field: &str) -> Option<std::net::SocketAddr> {
        let (ip, port) = field.split_once(':')?;
        let port = u16::from_str_radix(port, 16).ok()?;
        let ip: std::net::IpAddr = match ip.len() {
            8 => std::net::Ipv4Addr::from(word(ip)?).into(),
            32 => {
                let mut bytes = [0u8; 16];
                for (i, chunk) in bytes.chunks_mut(4).enumerate() {
                    chunk.copy_from_slice(&word(ip.get(i * 8..i * 8 + 8)?)?);
                }
                std::net::Ipv6Addr::from(bytes).into()
            }
            _ => return None,
        };
        Some(std::net::SocketAddr::new(ip, port))
    }
    const LISTEN: &str = "0A";
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.get(3) != Some(&LISTEN) {
                return None;
            }
            Some((address(fields.get(1)?)?, fields.get(9)?.parse().ok()?))
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_attributed_by_their_folders() {
        let herd = Path::new("C:")
            .join("Program Files")
            .join("Herd")
            .join("resources")
            .join("bin")
            .join("nginx")
            .join("nginx.exe");
        assert_eq!(tool_of(&herd), Some(Tool::Herd));
        assert_eq!(
            tool_of(Path::new(
                r"C:\Users\dev\.config\herd\bin\php84\php-cgi.exe"
            )),
            Some(Tool::Herd)
        );
        assert_eq!(
            tool_of(Path::new(r"C:\xampp\apache\bin\httpd.exe")),
            Some(Tool::Xampp)
        );
        assert_eq!(
            tool_of(Path::new("/opt/laragon/bin/nginx")),
            Some(Tool::Laragon)
        );
        assert_eq!(
            tool_of(Path::new(r"C:\wamp64\bin\apache\httpd.exe")),
            Some(Tool::Wamp)
        );
        assert_eq!(tool_of(Path::new(r"C:\tools\herd.exe")), None, "file name");
        assert_eq!(tool_of(Path::new(r"C:\nginx\nginx.exe")), None);
        assert_eq!(tool_of(Path::new(r"C:\Users\dev\herdsman\x.exe")), None);
    }

    #[test]
    fn holders_are_described_for_people() {
        // Built with `join`, so these are real paths on every OS the tests run on.
        let path = |parts: &[&str]| parts.iter().collect::<PathBuf>();
        let herd = Holder::new(
            812,
            Some(path(&[
                "Program Files",
                "Herd",
                "bin",
                "nginx",
                "nginx.exe",
            ])),
        );
        assert_eq!(herd.describe(), "Laravel Herd (nginx.exe)");
        let httpd = path(&["Apache24", "bin", "httpd.exe"]);
        let apache = Holder::new(9, Some(httpd.clone()));
        assert_eq!(
            apache.describe(),
            format!("Apache httpd ({})", httpd.display())
        );
        assert_eq!(
            Holder::new(4, None).describe().contains("IIS"),
            cfg!(windows),
            "PID 4 is http.sys only on Windows"
        );
        assert_eq!(
            Holder::new(77, None).describe(),
            "a program Kiln can't inspect"
        );
        let thing = path(&["apps", "thing.exe"]);
        let other = Holder::new(5, Some(thing.clone()));
        assert_eq!(other.describe(), thing.display().to_string());
    }

    #[test]
    fn service_programs_are_recognized_and_attributed() {
        let path = |parts: &[&str]| parts.iter().collect::<PathBuf>();
        let xampp = Holder::new(30, Some(path(&["xampp", "mysql", "bin", "mysqld.exe"])));
        assert_eq!(xampp.describe(), "XAMPP (mysqld.exe)");
        let mysqld = path(&[
            "Program Files",
            "MySQL",
            "MySQL Server 8.4",
            "bin",
            "mysqld.exe",
        ]);
        assert_eq!(
            Holder::new(31, Some(mysqld.clone())).describe(),
            format!("MySQL ({})", mysqld.display())
        );
        for (file, kind) in [
            ("postgres", "PostgreSQL"),
            ("redis-server", "Redis"),
            ("valkey-server", "Valkey"),
            ("GarnetServer.exe", "Garnet"),
            ("mailpit", "Mailpit"),
            ("com.docker.backend.exe", "Docker"),
            ("docker-proxy", "Docker"),
            ("wslrelay.exe", "WSL"),
        ] {
            let program = path(&["opt", file]);
            assert_eq!(
                Holder::new(1, Some(program.clone())).describe(),
                format!("{kind} ({})", program.display())
            );
        }
        // Kiln's own instance isn't another tool's.
        let ours = path(&[
            "Kiln",
            "services",
            "bin",
            "mailpit",
            "1.31.6",
            "kiln-mailpit",
        ]);
        assert_eq!(
            Holder::new(2, Some(ours.clone())).describe(),
            ours.display().to_string()
        );
    }

    #[test]
    fn windows_services_are_read_by_name_and_state_code() {
        // `sc.exe query type= service state= all`, trimmed; the labels and words could be in
        // any language, so only names and codes matter.
        let text = "\r\nSERVICE_NAME: MySQL80\r\nDISPLAY_NAME: MySQL80\r\n        TYPE               : 10  WIN32_OWN_PROCESS  \r\n        STATE              : 4  RUNNING \r\n                                (STOPPABLE, NOT_PAUSABLE, ACCEPTS_SHUTDOWN)\r\n        WIN32_EXIT_CODE    : 0  (0x0)\r\n        CHECKPOINT         : 0x0\r\n\r\nSERVICE_NAME: postgresql-x64-16\r\nDISPLAY_NAME: postgresql-x64-16 - PostgreSQL Server 16\r\n        TYPE               : 10  WIN32_OWN_PROCESS  \r\n        STATE              : 1  STOPPED \r\n        WIN32_EXIT_CODE    : 0  (0x0)\r\n\r\nSERVICE_NAME: Spooler\r\nDISPLAY_NAME: Print Spooler\r\n        TYPE               : 110  WIN32_OWN_PROCESS (interactive)\r\n        STATE              : 4  RUNNING \r\n\r\nSERVICE_NAME: Memurai\r\nDISPLAY_NAME: Redis-compatible store\r\n        TYPE               : 10  WIN32_OWN_PROCESS  \r\n        STATE              : 4  RUNNING \r\n";
        assert_eq!(
            parse_sc_query(text),
            [
                SystemService {
                    name: "MySQL80".into(),
                    kind: "MySQL",
                    running: true
                },
                SystemService {
                    name: "postgresql-x64-16".into(),
                    kind: "PostgreSQL",
                    running: false
                },
                SystemService {
                    name: "Memurai".into(),
                    kind: "Memurai",
                    running: true
                },
            ]
        );
    }

    #[test]
    fn systemd_units_are_read_with_their_sub_state() {
        let text = "mysql.service loaded active running MySQL Community Server\n\
                    postgresql@16-main.service loaded inactive dead PostgreSQL Cluster 16-main\n\
                    redis-server.service loaded active running Advanced key-value store\n\
                    ssh.service loaded active running OpenBSD Secure Shell server\n\
                    mariadb.service not-found inactive dead mariadb.service\n";
        let found = parse_systemctl_units(text);
        let names: Vec<_> = found
            .iter()
            .map(|s| (s.name.as_str(), s.kind, s.running))
            .collect();
        assert_eq!(
            names,
            [
                ("mysql.service", "MySQL", true),
                ("postgresql@16-main.service", "PostgreSQL", false),
                ("redis-server.service", "Redis", true),
                ("mariadb.service", "MariaDB", false),
            ]
        );
        assert_eq!(system_service_kind("MariaDB"), Some("MariaDB"));
        assert_eq!(system_service_kind("nginx.service"), None);
    }

    #[test]
    fn hosts_claims_skip_kiln_s_block_and_credit_herd_s() {
        let text = format!(
            "# Copyright\r\n127.0.0.1 localhost\r\n\
             # Herd generated Hosts. Do not change.\r\n\
             127.0.0.1 Blog.test #herd\r\n::1 blog.test\r\n127.0.0.1 shop.test\r\n\
             # End Herd generated Hosts\r\n\
             {BEGIN_MARKER}\r\n127.0.0.1 kiln-only.test\r\n{END_MARKER}\r\n\
             127.0.0.1 hand.test alias.test\r\n"
        );
        let claims = hosts_claims(text.as_bytes());
        let find = |name: &str| claims.iter().find(|c| c.name == name).cloned();
        assert_eq!(find("localhost").unwrap().tool, None);
        assert_eq!(find("blog.test").unwrap().tool, Some(Tool::Herd));
        assert_eq!(claims.iter().filter(|c| c.name == "blog.test").count(), 1);
        assert_eq!(find("shop.test").unwrap().owner(), "Laravel Herd");
        assert_eq!(find("hand.test").unwrap().owner(), "another program");
        assert!(find("alias.test").is_some());
        assert!(find("kiln-only.test").is_none());
        assert!(find("#herd").is_none(), "comments aren't names");
    }

    #[test]
    fn installed_tools_are_found_in_their_folders() {
        let dir = tempfile::tempdir().unwrap();
        let program_files = dir.path().join("Program Files");
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join(".config").join("herd")).unwrap();
        std::fs::create_dir_all(dir.path().join("wamp")).unwrap();
        let locations = known_locations(Some(&program_files), Some(&home), Some(dir.path()));
        let found = installed(&locations);
        assert_eq!(
            found,
            vec![
                (Tool::Herd, home.join(".config").join("herd")),
                (Tool::Wamp, dir.path().join("wamp")),
            ]
        );
    }

    #[test]
    fn listening_sockets_are_read_from_proc_net_tcp() {
        // Real lines from Ubuntu 24.04: Kiln's Caddy on 127.0.0.1:8080, systemd-resolved on
        // 127.0.0.53:53, an established connection (state 01, ignored).
        let v4 = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41234 1 0000000000000000 100 0 0 10 0\n   1: 3500007F:0035 00000000:0000 0A 00000000:00000000 00:00000000 00000000   991        0 5678 1 0000000000000000 100 0 0 10 5\n   2: 0100007F:1F90 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 0 1 0000000000000000 20 4 30 10 -1\n";
        assert_eq!(
            parse_proc_net_tcp(v4),
            vec![
                ("127.0.0.1:8080".parse().unwrap(), 41234),
                ("127.0.0.53:53".parse().unwrap(), 5678),
            ]
        );
        let v6 = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 41235 1 0000000000000000 100 0 0 10 0\n   1: 00000000000000000000000000000000:0016 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 900 1 0000000000000000 100 0 0 10 0\n";
        assert_eq!(
            parse_proc_net_tcp(v6),
            vec![
                ("[::1]:8080".parse().unwrap(), 41235),
                ("[::]:22".parse().unwrap(), 900),
            ]
        );
        assert_eq!(parse_proc_net_tcp("garbage\nnot a line\n"), vec![]);
    }
    #[test]
    fn the_first_free_port_wins() {
        assert_eq!(pick(&HTTP_PORTS, |p| p != 80), Some(8080));
        assert_eq!(pick(&HTTP_PORTS, |p| p != 80 && p != 8080), Some(8000));
        assert_eq!(pick(&HTTPS_PORTS, |_| true), Some(443));
        assert_eq!(pick(&HTTPS_PORTS, |_| false), None);
    }
}
