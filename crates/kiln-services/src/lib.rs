//! Kiln's services ([ADR 0035], [ADR 0036]): each kind is a [`ServiceProvider`] that says
//! where its pinned binaries come from, how to set up an instance's data folder, and how to
//! run and stop it. The CLI records instances in the state database and hands their
//! [`Launch`] specs to a `kiln-supervisor`.
//!
//! [ADR 0035]: ../../../docs/adr/0035-services.md
//! [ADR 0036]: ../../../docs/adr/0036-service-framework.md

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod install;
mod mailpit;
mod mysql;
mod postgres;

pub use install::{InstallError, install};

/// `.exe` on Windows, nothing elsewhere.
pub const EXE_SUFFIX: &str = std::env::consts::EXE_SUFFIX;

/// Windows' "a DLL was not found" exit status, from a program that couldn't start.
pub(crate) const STATUS_DLL_NOT_FOUND: i32 = 0xC000_0135_u32.cast_signed();

/// Why a server that exited with [`STATUS_DLL_NOT_FOUND`] can't run ([ADR 0038] §6).
///
/// [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md
pub(crate) fn needs_vc_runtime(display: &str) -> String {
    format!(
        "{display} needs Microsoft's Visual C++ runtime (2015–2022, x64), which this computer \
         lacks. Install it from https://aka.ms/vs/17/release/vc_redist.x64.exe and try \
         again. Kiln doesn't install system components itself."
    )
}

/// Runs one of a service's tools to completion, without a console window; returns its exit
/// code and its output and errors together.
pub(crate) fn run(
    program: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<(i32, String), String> {
    let mut command = std::process::Command::new(program);
    command.args(args).envs(env);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot run {}: {e}", program.display()))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((output.status.code().unwrap_or(-1), text))
}

/// The last `lines` non-empty lines of `text`, for an error message.
pub(crate) fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// One pinned release archive of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    pub version: &'static str,
    /// `std::env::consts::OS` / `ARCH` values it runs on.
    pub os: &'static str,
    pub arch: &'static str,
    /// File name of the archive.
    pub archive: &'static str,
    /// Where it is downloaded from.
    pub url: &'static str,
    /// SHA-256 of the archive, lowercase hex.
    pub sha256: &'static str,
    /// Path of the executable: inside the archive for a single-file install, or inside
    /// [`Release::tree`] for a whole-tree one.
    pub executable: &'static str,
    /// For a whole-tree install ([ADR 0038] §2): the archive's top folder, whose contents
    /// (those [`ServiceProvider::keep`] accepts) become the version's `bin` folder.
    ///
    /// [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md
    pub tree: Option<&'static str>,
}

/// A port a service listens on, by its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRole {
    /// e.g. `smtp`.
    pub role: &'static str,
    /// The port tools expect, e.g. 1025; Kiln takes the next free one if it is taken.
    pub usual: u16,
    /// What it is for, e.g. "SMTP (where apps send mail)".
    pub what: &'static str,
}

/// Where one instance's files are ([ADR 0036] §3).
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceDirs {
    /// `<services>/bin/<kind>/<version>`: the binaries, shared by instances of that version.
    pub bin: PathBuf,
    /// `<services>/<name>/data`.
    pub data: PathBuf,
    /// `<services>/<name>/<name>.log`: its output.
    pub log: PathBuf,
}

impl InstanceDirs {
    /// The folders of instance `name` of `kind` `version`, under `services` (Kiln's
    /// `<local data>/services`).
    #[must_use]
    pub fn new(services: &Path, kind: &str, version: &str, name: &str) -> Self {
        Self {
            bin: bin_dir(services, kind, version),
            data: services.join(name).join("data"),
            log: services.join(name).join(format!("{name}.log")),
        }
    }

    /// `<services>/<name>`, which holds the data folder and the log.
    #[must_use]
    pub fn instance(&self) -> &Path {
        self.data.parent().unwrap_or(&self.data)
    }

    /// `<services>/<name>/kiln.cnf`: the config Kiln writes for providers that have one.
    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.instance().join("kiln.cnf")
    }
}

/// `<services>/bin/<kind>/<version>`.
#[must_use]
pub fn bin_dir(services: &Path, kind: &str, version: &str) -> PathBuf {
    services.join("bin").join(kind).join(version)
}

/// How an instance runs, for the supervisor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    /// Run first to stop it gracefully, e.g. `mysqladmin shutdown`.
    pub stop_command: Option<Vec<String>>,
}

/// One kind of service ([ADR 0035] §2). Implementations are compiled in and pass the
/// contract tests below.
///
/// [ADR 0035]: ../../../docs/adr/0035-services.md
pub trait ServiceProvider: Sync {
    /// e.g. `mailpit`; lowercase ASCII.
    fn kind(&self) -> &'static str;
    /// e.g. "Mailpit".
    fn display_name(&self) -> &'static str;
    /// Its license, shown when it is installed, e.g. "MIT".
    fn license(&self) -> &'static str;
    /// Its ports; the first is its main one.
    fn ports(&self) -> &'static [PortRole];
    /// Pinned releases, newest first.
    fn releases(&self) -> &'static [Release];
    /// The name its executable is installed under, without [`EXE_SUFFIX`]: Kiln-specific,
    /// so tools that stop programs by name leave Kiln's alone ([I-11]).
    ///
    /// [I-11]: ../../../docs/issues.md#i-11
    fn executable_name(&self) -> &'static str;
    /// Sets up a new instance's data folder, after its config ([`ServiceProvider::config`])
    /// has been written. The error is for people.
    fn init_data(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Result<(), String>;
    /// How to run an instance with these `ports` (role → port), listening on loopback only.
    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch;

    /// The installed executable in `bin`.
    fn executable(&self, bin: &Path) -> PathBuf {
        bin.join(format!("{}{EXE_SUFFIX}", self.executable_name()))
    }

    /// The config file Kiln writes to [`InstanceDirs::config`] before each start, if the
    /// provider has one. It is rewritten every time, so a port change takes effect.
    fn config(&self, _dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Option<String> {
        None
    }

    /// For a whole-tree install: whether to keep the file at `path` (relative to the tree,
    /// `/`-separated). Everything by default.
    fn keep(&self, _path: &str) -> bool {
        true
    }

    /// Checks a freshly installed version runs on this system, and prepares what it needs
    /// from it ([ADR 0038] §6). The error is for people.
    ///
    /// [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md
    fn check(&self, _bin: &Path) -> Result<(), String> {
        Ok(())
    }

    /// The part of `version` that names an instance by default: the major version, e.g.
    /// `1` for Mailpit 1.31.6.
    fn series(&self, version: &str) -> String {
        version.split('.').next().unwrap_or(version).to_owned()
    }

    /// Whether a stopped instance has finished stopping. A database closes its port first
    /// and flushes its data after, so the port alone doesn't say ([ADR 0038] §5). True by
    /// default.
    ///
    /// [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md
    fn stopped(&self, _dirs: &InstanceDirs) -> bool {
        true
    }

    /// The files `kiln service logs` shows, in order: the instance's output by default; a
    /// server with its own error log adds it.
    fn logs(&self, dirs: &InstanceDirs) -> Vec<PathBuf> {
        vec![dirs.log.clone()]
    }

    /// The client to open on an instance (`kiln db`), if there is one.
    fn client(
        &self,
        _dirs: &InstanceDirs,
        _ports: &BTreeMap<String, u16>,
        _database: Option<&str>,
    ) -> Option<Launch> {
        None
    }
}

/// Every provider Kiln has.
#[must_use]
pub fn providers() -> &'static [&'static dyn ServiceProvider] {
    static PROVIDERS: [&dyn ServiceProvider; 4] = [
        &mailpit::Mailpit,
        &mysql::MYSQL,
        &mysql::MARIADB,
        &postgres::Postgres,
    ];
    &PROVIDERS
}

/// The provider of `kind`.
#[must_use]
pub fn provider(kind: &str) -> Option<&'static dyn ServiceProvider> {
    providers().iter().copied().find(|p| p.kind() == kind)
}

/// The pinned release of `provider` for this OS and CPU: `version` if given (exactly, or
/// the newest whose version starts with it followed by a dot), else the newest.
#[must_use]
pub fn release_for_this_platform(
    provider: &dyn ServiceProvider,
    version: Option<&str>,
) -> Option<&'static Release> {
    provider.releases().iter().find(|r| {
        r.os == std::env::consts::OS
            && r.arch == std::env::consts::ARCH
            && version.is_none_or(|v| r.version == v || r.version.starts_with(&format!("{v}.")))
    })
}

#[cfg(test)]
mod contract {
    //! Every provider passes these ([ADR 0036] §4).
    //!
    //! [ADR 0036]: ../../../docs/adr/0036-service-framework.md

    use super::*;

    fn sample_ports(provider: &dyn ServiceProvider) -> BTreeMap<String, u16> {
        provider
            .ports()
            .iter()
            .map(|p| (p.role.to_owned(), p.usual))
            .collect()
    }

    #[test]
    fn kinds_are_unique_lowercase_names() {
        let mut kinds: Vec<_> = providers().iter().map(|p| p.kind()).collect();
        for kind in &kinds {
            assert!(
                !kind.is_empty()
                    && kind
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{kind}"
            );
            assert_eq!(provider(kind).map(ServiceProvider::kind), Some(*kind));
        }
        let n = kinds.len();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), n, "two providers share a kind");
    }

    #[test]
    fn ports_have_unique_roles_and_usual_numbers() {
        for provider in providers() {
            let ports = provider.ports();
            assert!(!ports.is_empty(), "{}", provider.kind());
            let mut roles: Vec<_> = ports.iter().map(|p| p.role).collect();
            roles.sort_unstable();
            roles.dedup();
            assert_eq!(
                roles.len(),
                ports.len(),
                "{}: duplicate role",
                provider.kind()
            );
            for port in ports {
                assert!(
                    port.usual >= 1024,
                    "{}: {port:?} needs no admin",
                    provider.kind()
                );
                assert_ne!(port.what, "");
            }
        }
    }

    #[test]
    fn pins_are_well_formed() {
        for provider in providers() {
            assert!(!provider.releases().is_empty(), "{}", provider.kind());
            for r in provider.releases() {
                assert_eq!(r.sha256.len(), 64, "{r:?}");
                assert!(
                    r.sha256
                        .chars()
                        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                    "{r:?}"
                );
                assert!(
                    r.url.starts_with("https://") && r.url.ends_with(r.archive),
                    "{r:?}"
                );
                assert!(
                    !r.archive.contains(['/', '\\']) && !r.archive.contains(".."),
                    "{r:?}"
                );
                assert!(!r.executable.contains(".."), "{r:?}");
                assert!(["windows", "linux", "macos"].contains(&r.os), "{r:?}");
            }
        }
    }

    #[test]
    fn executables_have_kiln_names() {
        for provider in providers() {
            assert!(
                provider.executable_name().starts_with("kiln-"),
                "{}",
                provider.kind()
            );
        }
    }

    #[test]
    fn instances_listen_on_loopback_and_stay_in_their_folders() {
        let services = Path::new("/k/services");
        for provider in providers() {
            let dirs = InstanceDirs::new(services, provider.kind(), "1.0", "x-1");
            let launch = provider.launch(&dirs, &sample_ports(*provider));
            assert!(launch.program.starts_with(&dirs.bin), "{launch:?}");
            for arg in &launch.args {
                assert!(
                    !arg.contains("0.0.0.0") && !arg.contains("[::]"),
                    "{}: listens beyond loopback: {arg}",
                    provider.kind()
                );
                // Any address it names is a loopback one.
                if let Some((host, port)) = arg.rsplit_once(':')
                    && port.parse::<u16>().is_ok()
                    && !host.is_empty()
                    && !host.contains(['/', '\\'])
                {
                    assert!(
                        host == "127.0.0.1" || host == "[::1]" || host == "localhost",
                        "{}: {arg}",
                        provider.kind()
                    );
                }
            }
            if let Some(path) = &launch.cwd {
                assert!(path.starts_with(services), "{path:?}");
            }
            // A config file listens on loopback only too ([ADR 0038] §3): MySQL's
            // `bind-address` or PostgreSQL's `listen_addresses`.
            let config = provider
                .config(&dirs, &sample_ports(*provider))
                .unwrap_or_default();
            let mut listens = false;
            for line in config.lines() {
                let line = line.trim();
                assert!(
                    !line.contains("0.0.0.0") && !line.contains('*') && !line.ends_with("::"),
                    "{}: {line}",
                    provider.kind()
                );
                let address = line
                    .strip_prefix("bind-address=")
                    .or_else(|| line.strip_prefix("listen_addresses="));
                if let Some(address) = address {
                    assert!(
                        ["127.0.0.1", "::1", "'127.0.0.1'", "'::1'"].contains(&address),
                        "{}: {line}",
                        provider.kind()
                    );
                    listens = true;
                }
            }
            assert!(
                config.is_empty() || listens,
                "{}: a config must say where to listen",
                provider.kind()
            );
            // Every port it was given appears in its command line or its config.
            for port in sample_ports(*provider).values() {
                let in_args = launch.args.iter().any(|a| a.ends_with(&format!(":{port}")));
                let in_config = config.lines().any(|l| l.trim() == format!("port={port}"));
                assert!(
                    in_args || in_config,
                    "{}: port {port} unused",
                    provider.kind()
                );
            }
            // A stop command runs one of its own programs.
            if let Some(stop) = &launch.stop_command {
                assert!(
                    Path::new(&stop[0]).starts_with(&dirs.bin),
                    "{}: {stop:?}",
                    provider.kind()
                );
            }
        }
    }

    #[test]
    fn a_version_picks_the_newest_matching_pin() {
        let mailpit = provider("mailpit").unwrap();
        if let Some(newest) = release_for_this_platform(mailpit, None) {
            assert_eq!(
                release_for_this_platform(mailpit, Some(newest.version)),
                Some(newest)
            );
            let major = newest.version.split('.').next().unwrap();
            assert_eq!(
                release_for_this_platform(mailpit, Some(major)).map(|r| r.version),
                Some(newest.version)
            );
        }
        assert_eq!(release_for_this_platform(mailpit, Some("0.0.1")), None);
        // "1" doesn't match "10.0".
        assert!(
            release_for_this_platform(mailpit, Some("1.3"))
                .is_none_or(|r| r.version.starts_with("1.3."))
        );
    }

    #[test]
    fn instance_folders_follow_the_layout() {
        let dirs = InstanceDirs::new(Path::new("/k/services"), "mailpit", "1.31.6", "mail");
        assert_eq!(dirs.bin, Path::new("/k/services/bin/mailpit/1.31.6"));
        assert_eq!(dirs.data, Path::new("/k/services/mail/data"));
        assert_eq!(dirs.log, Path::new("/k/services/mail/mail.log"));
        assert_eq!(dirs.instance(), Path::new("/k/services/mail"));
    }
}
