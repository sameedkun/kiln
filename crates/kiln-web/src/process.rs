//! Starting, checking and stopping Kiln's Caddy process ([ADR 0011] §11–12).
//!
//! The server runs in the background after `kiln` exits. A PID file records the process ID and
//! the executable path. Every check opens the process and compares its executable to the
//! recorded one, so a reused PID never makes Kiln report or stop an unrelated process.
//!
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md

use std::fs::{self, File};
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::{ConfigError, WebConfig};

pub const CONFIG_FILE: &str = "caddy.json";
pub const PID_FILE: &str = "caddy.pid";
/// Caddy's stdout/stderr: start-up messages written before its own logging is configured.
pub const CONSOLE_LOG_FILE: &str = "caddy-console.log";

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether Kiln's web server is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Stopped,
    Running {
        pid: u32,
    },
    /// A PID file is left over, but that process has exited (or is now something else).
    Stale {
        pid: u32,
    },
}

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

    fn pid_path(&self) -> PathBuf {
        self.dir.join(PID_FILE)
    }

    fn console_log_path(&self) -> PathBuf {
        self.dir.join(CONSOLE_LOG_FILE)
    }

    /// The recorded PID, if a PID file exists and names this server's executable.
    fn recorded(&self) -> Result<Option<u32>, WebError> {
        let path = self.pid_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err(format!("cannot read {}", path.display()))(e)),
        };
        let mut lines = text.lines();
        let pid = lines.next().and_then(|l| l.trim().parse().ok());
        let exe = lines.next().map(PathBuf::from);
        match (pid, exe) {
            (Some(pid), Some(exe)) if exe == self.exe => Ok(Some(pid)),
            // Unreadable, or written for another executable: treat as a stale PID 0.
            _ => Ok(Some(0)),
        }
    }

    pub fn status(&self) -> Result<Status, WebError> {
        Ok(match self.recorded()? {
            None => Status::Stopped,
            Some(pid) if platform::is_ours(pid, &self.exe) => Status::Running { pid },
            Some(pid) => Status::Stale { pid },
        })
    }

    /// Writes the config for `config`, validates it with Caddy and starts Caddy in the
    /// background. Returns once every listener accepts connections.
    pub fn start(&self, config: &WebConfig) -> Result<u32, WebError> {
        if !self.exe.is_file() {
            return Err(WebError::MissingBinary(self.exe.clone()));
        }
        match self.status()? {
            Status::Running { pid } => return Err(WebError::AlreadyRunning(pid)),
            Status::Stale { .. } => self.forget()?,
            Status::Stopped => {}
        }
        let json = config.render()?;
        let listeners = config.listeners();
        for addr in &listeners {
            if TcpListener::bind(addr).is_err() {
                return Err(WebError::PortInUse(*addr));
            }
        }

        fs::create_dir_all(&self.dir)
            .map_err(io_err(format!("cannot create {}", self.dir.display())))?;
        let config_path = self.config_path();
        write_atomically(&config_path, json.as_bytes())?;

        let output = Command::new(&self.exe)
            .arg("validate")
            .arg("--config")
            .arg(&config_path)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .output()
            .map_err(io_err(format!("cannot run {}", self.exe.display())))?;
        if !output.status.success() {
            let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            return Err(WebError::Validate(tail(&text, 20)));
        }

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
        let mut child = platform::spawn_background(&mut command)
            .map_err(io_err(format!("cannot start {}", self.exe.display())))?;
        let pid = child.id();
        write_atomically(
            &self.pid_path(),
            format!("{pid}\n{}\n", self.exe.display()).as_bytes(),
        )?;

        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(io_err("cannot check the web server process"))?
            {
                self.forget()?;
                return Err(WebError::Exited {
                    status: status.to_string(),
                    log: self.recent_log(),
                });
            }
            if listeners
                .iter()
                .all(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(200)).is_ok())
            {
                return Ok(pid);
            }
            if Instant::now() >= deadline {
                let log = self.recent_log();
                let _ = self.stop();
                return Err(WebError::NotReady {
                    seconds: READY_TIMEOUT.as_secs(),
                    log,
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Stops the server if it is running. Returns the PID it stopped.
    pub fn stop(&self) -> Result<Option<u32>, WebError> {
        let stopped = match self.status()? {
            Status::Running { pid } => {
                platform::terminate(pid, &self.exe, STOP_TIMEOUT)
                    .map_err(io_err(format!("cannot stop the web server (PID {pid})")))?;
                Some(pid)
            }
            Status::Stale { .. } | Status::Stopped => None,
        };
        self.forget()?;
        Ok(stopped)
    }

    fn forget(&self) -> Result<(), WebError> {
        match fs::remove_file(self.pid_path()) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err(format!(
                "cannot remove {}",
                self.pid_path().display()
            ))(e)),
            _ => Ok(()),
        }
    }

    /// The last lines of Caddy's console output, for error messages.
    #[must_use]
    pub fn recent_log(&self) -> String {
        fs::read_to_string(self.console_log_path())
            .map(|text| tail(&text, 20))
            .unwrap_or_default()
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

#[cfg(windows)]
mod platform {
    use std::io;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command};
    use std::time::Duration;

    use kiln_sys::win::Process;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    /// No console window, its own process group (so Ctrl+C in the terminal doesn't reach it),
    /// and out of the caller's job object where that's allowed, so it outlives `kiln`.
    pub fn spawn_background(command: &mut Command) -> io::Result<Child> {
        let flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
        command.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB);
        match command.spawn() {
            // A job that forbids breakaway refuses the flag; start inside it instead.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                command.creation_flags(flags);
                command.spawn()
            }
            other => other,
        }
    }

    fn same_file(a: &Path, b: &Path) -> bool {
        match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            (Ok(a), Ok(b)) => a
                .to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy()),
            _ => a
                .to_string_lossy()
                .eq_ignore_ascii_case(&b.to_string_lossy()),
        }
    }

    /// Opens `pid` only if it is running and its executable is `exe`.
    fn open_ours(pid: u32, exe: &Path) -> io::Result<Option<Process>> {
        let process = match Process::open(pid) {
            Ok(Some(process)) => process,
            Ok(None) => return Ok(None),
            // Another user's or a protected process: by definition not ours.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Ok(None),
            Err(e) => return Err(e),
        };
        if !process.is_running()? || !same_file(&process.image_path()?, exe) {
            return Ok(None);
        }
        Ok(Some(process))
    }

    pub fn is_ours(pid: u32, exe: &Path) -> bool {
        matches!(open_ours(pid, exe), Ok(Some(_)))
    }

    pub fn terminate(pid: u32, exe: &Path, timeout: Duration) -> io::Result<()> {
        let Some(process) = open_ours(pid, exe)? else {
            return Ok(());
        };
        process.terminate(1)?;
        match process.wait(timeout)? {
            Some(_) => Ok(()),
            None => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the process didn't exit after being terminated",
            )),
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    pub fn spawn_background(command: &mut Command) -> io::Result<Child> {
        use std::os::unix::process::CommandExt;
        // Own process group, so Ctrl+C in the terminal doesn't reach it.
        command.process_group(0);
        command.spawn()
    }

    fn alive(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    pub fn is_ours(pid: u32, exe: &Path) -> bool {
        if pid == 0 || !alive(pid) {
            return false;
        }
        // Linux names the executable exactly.
        if let Ok(image) = std::fs::read_link(format!("/proc/{pid}/exe")) {
            return std::fs::canonicalize(exe).is_ok_and(|exe| exe == image);
        }
        if cfg!(target_os = "linux") {
            return false;
        }
        // Elsewhere (macOS, BSD) ask `ps` for the program it runs. Never assume a live PID is
        // ours: that would let `stop` signal an unrelated process that reused the PID.
        let Ok(output) = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .stderr(Stdio::null())
            .output()
        else {
            return false;
        };
        let command = String::from_utf8_lossy(&output.stdout);
        let command = Path::new(command.trim());
        if command.is_absolute() {
            return match (std::fs::canonicalize(command), std::fs::canonicalize(exe)) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            };
        }
        // Some systems report only the (possibly truncated) name; require it to match.
        exe.file_name()
            .zip(command.file_name())
            .is_some_and(|(want, got)| want == got)
    }

    fn signal(pid: u32, name: &str) -> io::Result<()> {
        let status = Command::new("kill")
            .args([&format!("-{name}"), &pid.to_string()])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("kill -{name} {pid} failed")))
        }
    }

    pub fn terminate(pid: u32, exe: &Path, timeout: Duration) -> io::Result<()> {
        if !is_ours(pid, exe) {
            return Ok(());
        }
        signal(pid, "TERM")?;
        let deadline = Instant::now() + timeout;
        while alive(pid) {
            if Instant::now() >= deadline {
                signal(pid, "KILL")?;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
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
            log_file: dir.path().join("caddy.log"),
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
