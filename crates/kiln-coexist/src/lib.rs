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
/// Linux and macOS come with P1-04 and Phase 3).
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

/// What a well-known server program is, from its file name.
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
        _ => None,
    }
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
