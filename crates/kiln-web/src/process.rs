//! Starting, checking, reloading and stopping Kiln's Caddy process ([ADR 0011] §11–12,
//! [ADR 0012]).
//!
//! The server runs in the background after `kiln` exits. A PID file records the process ID and
//! the executable path. Every check opens the process and compares its executable to the
//! recorded one, so a reused PID never makes Kiln report or stop an unrelated process.
//!
//! Config changes go to the running server through its admin socket (`POST /load`), so they
//! apply without dropping connections. Stopping asks Caddy to shut down gracefully first.
//!
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md
//! [ADR 0012]: ../../../docs/adr/0012-caddy-admin-channel.md

use std::fs::{self, File};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kiln_sys::background::{self, PidFile};

use crate::admin::{AdminClient, AdminError};
use crate::config::{ConfigError, WebConfig};

pub const CONFIG_FILE: &str = "caddy.json";
pub const PID_FILE: &str = "caddy.pid";
/// The admin endpoint's Unix socket (ADR 0012).
pub const ADMIN_SOCKET_FILE: &str = "admin.sock";
/// Caddy's stdout/stderr: start-up messages written before its own logging is configured.
pub const CONSOLE_LOG_FILE: &str = "caddy-console.log";
/// Caddy's own (rolled) log.
pub const LOG_FILE: &str = "caddy.log";

/// Caddy's access log, one JSON line per request ([ADR 0065]), next to [`LOG_FILE`].
///
/// [ADR 0065]: ../../../docs/adr/0065-request-timing.md
pub const ACCESS_LOG_FILE: &str = "access.log";

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether Kiln's web server is running.
pub use kiln_sys::background::ProcStatus as Status;

#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("the web server binary {} is not installed; run `kiln web install`", .0.display())]
    MissingBinary(PathBuf),
    #[error("Kiln's web server is already running (PID {0})")]
    AlreadyRunning(u32),
    #[error(
        "{0} is already in use by another program (another web server such as Laravel Herd?); \
         pick other ports with --http-port / --https-port"
    )]
    PortInUse(SocketAddr),
    #[error("Caddy rejected the generated config:\n{0}")]
    Validate(String),
    #[error("the web server exited during start-up ({status}):\n{log}")]
    Exited { status: String, log: String },
    #[error("the web server didn't start listening within {seconds} s:\n{log}")]
    NotReady { seconds: u64, log: String },
    #[error("Kiln's web server isn't running; start it with `kiln web start`")]
    NotRunning,
    #[error(transparent)]
    Admin(#[from] AdminError),
    #[error(
        "the config names admin socket {}, but this server uses {}",
        .0.display(),
        .1.display()
    )]
    AdminSocketMismatch(PathBuf, PathBuf),
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> WebError {
    let context = context.into();
    move |source| WebError::Io { context, source }
}

/// Kiln's Caddy: its executable plus the directory holding its config, PID file and logs.
#[derive(Debug, Clone)]
pub struct WebServer {
    exe: PathBuf,
    dir: PathBuf,
}

impl WebServer {
    #[must_use]
    pub fn new(exe: PathBuf, dir: PathBuf) -> Self {
        Self { exe, dir }
    }

    #[must_use]
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE)
    }

    /// Where the admin socket lives; put this in [`WebConfig::admin_socket`].
    #[must_use]
    pub fn admin_socket(&self) -> PathBuf {
        self.dir.join(ADMIN_SOCKET_FILE)
    }

    fn admin(&self) -> AdminClient {
        AdminClient::new(self.admin_socket())
    }

    fn check_socket(&self, config: &WebConfig) -> Result<(), WebError> {
        if config.admin_socket == self.admin_socket() {
            Ok(())
        } else {
            Err(WebError::AdminSocketMismatch(
                config.admin_socket.clone(),
                self.admin_socket(),
            ))
        }
    }

    fn console_log_path(&self) -> PathBuf {
        self.dir.join(CONSOLE_LOG_FILE)
    }

    fn pid_file(&self) -> PidFile {
        PidFile::new(self.dir.join(PID_FILE), self.exe.clone())
    }

    pub fn status(&self) -> Result<Status, WebError> {
        self.pid_file().status().map_err(io_err(format!(
            "cannot read {}",
            self.dir.join(PID_FILE).display()
        )))
    }

    /// Writes the config for `config`, validates it with Caddy and starts Caddy in the
    /// background. Returns once every listener accepts connections.
    pub fn start(&self, config: &WebConfig) -> Result<u32, WebError> {
        if !self.exe.is_file() {
            return Err(WebError::MissingBinary(self.exe.clone()));
        }
        match self.status()? {
            Status::Running { pid } => return Err(WebError::AlreadyRunning(pid)),
            Status::Stale { .. } | Status::Stopped => self.forget()?,
        }
        self.check_socket(config)?;
        for addr in &config.listeners() {
            if TcpListener::bind(addr).is_err() {
                return Err(WebError::PortInUse(*addr));
            }
        }
        self.prepare(config, &self.config_path())?;
        self.launch(config)
    }

    /// Restarts the running server with `config`, for changes a reload can't apply (I-57).
    /// The new config is written and validated while the old server still serves, so sites
    /// go unanswered only from the old server's stop until the new one listens (ADR 0034).
    pub fn restart(&self, config: &WebConfig) -> Result<u32, WebError> {
        if !self.exe.is_file() {
            return Err(WebError::MissingBinary(self.exe.clone()));
        }
        if !matches!(self.status()?, Status::Running { .. }) {
            return Err(WebError::NotRunning);
        }
        self.check_socket(config)?;
        // As for a reload: a listener the running config doesn't have must be free.
        let running = fs::read_to_string(self.config_path()).unwrap_or_default();
        for addr in config.listeners() {
            let known = running.contains(&format!("\"{addr}\""));
            if !known && TcpListener::bind(addr).is_err() {
                return Err(WebError::PortInUse(addr));
            }
        }
        let next = self.dir.join(format!("{CONFIG_FILE}.next"));
        if let Err(e) = self.prepare(config, &next) {
            let _ = fs::remove_file(&next);
            return Err(e); // the running server is left as it was
        }
        self.stop()?;
        kiln_sys::rename_replace(&next, &self.config_path())
            .map_err(io_err(format!("cannot move {} into place", next.display())))?;
        self.launch(config)
    }

    /// Writes `config` to `path` and has Caddy validate it.
    fn prepare(&self, config: &WebConfig, path: &Path) -> Result<(), WebError> {
        let json = config.render()?;
        background::create_private_dir(&self.dir)
            .map_err(io_err(format!("cannot create {}", self.dir.display())))?;
        write_atomically(path, json.as_bytes())?;

        let output = Command::new(&self.exe)
            .arg("validate")
            .arg("--config")
            .arg(path)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .output()
            .map_err(io_err(format!("cannot run {}", self.exe.display())))?;
        if !output.status.success() {
            let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            return Err(WebError::Validate(tail(&text, 20)));
        }
        Ok(())
    }

    /// Starts Caddy in the background on the written config. Returns once every listener of
    /// `config` accepts connections.
    fn launch(&self, config: &WebConfig) -> Result<u32, WebError> {
        let config_path = self.config_path();
        let listeners = config.listeners();
        // Only errors Caddy logs from here on belong to this start.
        let log_start = fs::metadata(self.log_file()).map_or(0, |m| m.len());
        let console_path = self.console_log_path();
        let console = File::create(&console_path)
            .map_err(io_err(format!("cannot create {}", console_path.display())))?;
        let console_err = console
            .try_clone()
            .map_err(io_err("cannot duplicate the log handle"))?;
        let mut command = Command::new(&self.exe);
        command
            .arg("run")
            .arg("--config")
            .arg(&config_path)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(console)
            .stderr(console_err);
        let mut child = background::spawn_background(&mut command)
            .map_err(io_err(format!("cannot start {}", self.exe.display())))?;
        let pid = child.id();
        self.pid_file()
            .write(pid)
            .map_err(io_err("cannot record the web server's PID"))?;

        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(io_err("cannot check the web server process"))?
            {
                // Report why it exited, even if cleaning up after it fails.
                let _ = self.forget();
                return Err(WebError::Exited {
                    status: status.to_string(),
                    log: self.recent_log_since(log_start),
                });
            }
            if listeners
                .iter()
                .all(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(200)).is_ok())
            {
                return Ok(pid);
            }
            if Instant::now() >= deadline {
                let log = self.recent_log_since(log_start);
                let _ = self.stop();
                return Err(WebError::NotReady {
                    seconds: READY_TIMEOUT.as_secs(),
                    log,
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Applies `config` to the running server without restarting it (`POST /load`). Caddy
    /// validates it and keeps the old config if it is rejected. On success the config file is
    /// updated too, so the next start uses it.
    pub fn reload(&self, config: &WebConfig) -> Result<(), WebError> {
        if !matches!(self.status()?, Status::Running { .. }) {
            return Err(WebError::NotRunning);
        }
        self.check_socket(config)?;
        let json = config.render()?;
        // A listener the running config doesn't have yet (the HTTPS port, when the first site
        // becomes secure) must be free; ones it has are held by this server.
        let running = fs::read_to_string(self.config_path()).unwrap_or_default();
        for addr in config.listeners() {
            let known = running.contains(&format!("\"{addr}\""));
            if !known && TcpListener::bind(addr).is_err() {
                return Err(WebError::PortInUse(addr));
            }
        }
        self.admin().load(&json)?;
        write_atomically(&self.config_path(), json.as_bytes())
    }

    /// Stops the server if it is running: gracefully through the admin API, and by
    /// terminating the (verified) process only if that fails. Returns the PID it stopped.
    pub fn stop(&self) -> Result<Option<u32>, WebError> {
        let stopped = match self.status()? {
            Status::Running { pid } => {
                let exited = || !background::is_ours(pid, &self.exe);
                let graceful =
                    self.admin().stop().is_ok() && background::wait_until(exited, STOP_TIMEOUT);
                if !graceful {
                    background::terminate(pid, &self.exe, STOP_TIMEOUT)
                        .map_err(io_err(format!("cannot stop the web server (PID {pid})")))?;
                }
                Some(pid)
            }
            Status::Stale { .. } | Status::Stopped => None,
        };
        self.forget()?;
        Ok(stopped)
    }

    /// Removes the PID file, and any socket file left by a server that is no longer running.
    ///
    /// The socket is best effort: Caddy removes it on a graceful stop and replaces a stale one
    /// on start, and a leftover must never hide the error that matters (I-45).
    fn forget(&self) -> Result<(), WebError> {
        let _ = fs::remove_file(self.admin_socket());
        self.pid_file()
            .remove()
            .map_err(io_err("cannot remove the web server's PID file"))
    }

    /// Caddy's own log file; put this in [`WebConfig::log_file`].
    #[must_use]
    pub fn log_file(&self) -> PathBuf {
        self.dir.join(LOG_FILE)
    }

    /// Caddy's access log ([ADR 0065]).
    ///
    /// [ADR 0065]: ../../../docs/adr/0065-request-timing.md
    #[must_use]
    pub fn access_log_file(&self) -> PathBuf {
        self.dir.join(ACCESS_LOG_FILE)
    }

    /// For error messages: the last console lines plus Caddy's most recent logged errors
    /// (once its logging is configured, start-up failures go to its log file, not the console).
    #[must_use]
    pub fn recent_log(&self) -> String {
        self.recent_log_since(0)
    }

    /// Like [`Self::recent_log`], but only log-file errors written after byte `offset`.
    fn recent_log_since(&self, offset: u64) -> String {
        let console = fs::read_to_string(self.console_log_path()).unwrap_or_default();
        let logged = fs::read(self.log_file())
            .ok()
            .and_then(|bytes| {
                bytes
                    .get(usize::try_from(offset).ok()?..)
                    .map(<[u8]>::to_vec)
            })
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let errors: Vec<&str> = logged
            .lines()
            .filter(|line| line.contains("\"level\":\"error\""))
            .collect();
        let mut text = tail(&console, 10);
        for line in &errors[errors.len().saturating_sub(5)..] {
            if !text.contains(line) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(line);
            }
        }
        text
    }
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn write_atomically(path: &Path, data: &[u8]) -> Result<(), WebError> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, data).map_err(io_err(format!("cannot write {}", temp.display())))?;
    kiln_sys::rename_replace(&temp, path)
        .map_err(io_err(format!("cannot replace {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_pid_file_the_server_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let server = WebServer::new(dir.path().join("kiln-caddy"), dir.path().into());
        assert_eq!(server.status().unwrap(), Status::Stopped);
        assert_eq!(server.stop().unwrap(), None);
    }

    #[test]
    fn a_pid_file_naming_another_process_is_stale_and_never_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("kiln-caddy");
        let server = WebServer::new(exe.clone(), dir.path().into());
        // This test process is alive, but it isn't `exe`.
        let me = std::process::id();
        fs::write(
            dir.path().join(PID_FILE),
            format!("{me}\n{}\n", exe.display()),
        )
        .unwrap();
        assert_eq!(server.status().unwrap(), Status::Stale { pid: me });
        assert_eq!(server.stop().unwrap(), None);
        assert!(
            !dir.path().join(PID_FILE).exists(),
            "stale PID file removed"
        );
    }

    #[test]
    fn a_garbled_pid_file_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let server = WebServer::new(dir.path().join("kiln-caddy"), dir.path().into());
        fs::write(dir.path().join(PID_FILE), "not a pid").unwrap();
        assert_eq!(server.status().unwrap(), Status::Stale { pid: 0 });
    }

    #[test]
    fn start_without_the_binary_says_how_to_install_it() {
        let dir = tempfile::tempdir().unwrap();
        let server = WebServer::new(dir.path().join("kiln-caddy"), dir.path().into());
        let config = WebConfig {
            http_port: 8080,
            https_port: 8443,
            ipv6: false,
            sites: Vec::new(),
            ca: None,
            storage_dir: dir.path().join("storage"),
            log_file: dir.path().join(LOG_FILE),
            admin_socket: dir.path().join(ADMIN_SOCKET_FILE),
            lan: Vec::new(),
            php_pools: std::collections::BTreeMap::new(),
        };
        let err = server.start(&config).unwrap_err();
        assert!(matches!(err, WebError::MissingBinary(_)), "{err}");
        assert!(err.to_string().contains("kiln web install"));
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        assert_eq!(tail("a\nb\nc\n", 2), "b\nc");
        assert_eq!(tail("a", 5), "a");
    }
}
