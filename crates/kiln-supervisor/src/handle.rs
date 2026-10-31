//! Starting, updating and stopping the supervisor from the CLI ([ADR 0014] §1–2, §10–11).
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use kiln_sys::background::{self, PidFile, ProcStatus};

use crate::pools::Pools;

pub const POOLS_FILE: &str = "pools.json";
pub const PID_FILE: &str = "supervisor.pid";
pub const LOG_FILE: &str = "supervisor.log";
pub const ERRORS_FILE: &str = "php-errors.log";
pub const EXE_NAME: &str = if cfg!(windows) {
    "kiln-supervisor.exe"
} else {
    "kiln-supervisor"
};

/// How long PHP workers get to start listening.
pub const READY_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the PHP supervisor gets to stop its workers and exit.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a supervisor of programs gets: each program may take its stop command, a wait
/// and SIGTERM, about 30 s, before it is killed.
pub const PROGRAM_STOP_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
    #[error("{} is missing; build or install kiln-supervisor next to kiln", .0.display())]
    MissingBinary(PathBuf),
    #[error("the supervisor exited during start-up:\n{0}")]
    Exited(String),
    #[error("{what} didn't start listening within {seconds} s:\n{log}")]
    NotReady {
        what: &'static str,
        seconds: u64,
        log: String,
    },
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> SupervisorError {
    let context = context.into();
    move |source| SupervisorError::Io { context, source }
}

/// `kiln-supervisor` next to the running executable.
pub fn default_exe() -> io::Result<PathBuf> {
    let me = std::env::current_exe()?;
    Ok(me.with_file_name(EXE_NAME))
}

/// The supervisor of one run directory.
#[derive(Debug, Clone)]
pub struct SupervisorHandle {
    exe: PathBuf,
    dir: PathBuf,
    /// What it runs, for messages: "PHP workers" or "services".
    what: &'static str,
    ready_timeout: Duration,
    stop_timeout: Duration,
}

impl SupervisorHandle {
    /// The PHP supervisor of `dir`.
    #[must_use]
    pub fn new(exe: PathBuf, dir: PathBuf) -> Self {
        Self {
            exe,
            dir,
            what: "PHP workers",
            ready_timeout: READY_TIMEOUT,
            stop_timeout: STOP_TIMEOUT,
        }
    }

    /// A supervisor of programs, such as services ([ADR 0036] §1): `what` names them in
    /// messages, and they get `ready_timeout` to start listening.
    ///
    /// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
    #[must_use]
    pub fn for_programs(
        exe: PathBuf,
        dir: PathBuf,
        what: &'static str,
        ready_timeout: Duration,
    ) -> Self {
        Self {
            exe,
            dir,
            what,
            ready_timeout,
            stop_timeout: PROGRAM_STOP_TIMEOUT,
        }
    }

    #[must_use]
    pub fn pools_path(&self) -> PathBuf {
        self.dir.join(POOLS_FILE)
    }

    #[must_use]
    pub fn log_path(&self) -> PathBuf {
        self.dir.join(LOG_FILE)
    }

    #[must_use]
    pub fn errors_path(&self) -> PathBuf {
        self.dir.join(ERRORS_FILE)
    }

    fn pid_file(&self) -> PidFile {
        PidFile::new(self.dir.join(PID_FILE), self.exe.clone())
    }

    pub fn status(&self) -> io::Result<ProcStatus> {
        self.pid_file().status()
    }

    /// The pools the supervisor is working towards, if it has any.
    pub fn pools(&self) -> io::Result<Option<Pools>> {
        Pools::read(&self.pools_path())
    }

    /// Starts the supervisor (or updates a running one) and waits until every worker in
    /// `pools` accepts connections. Returns the supervisor's PID.
    pub fn start(&self, pools: &Pools) -> Result<u32, SupervisorError> {
        if let ProcStatus::Running { pid } = self
            .status()
            .map_err(io_err("cannot read supervisor.pid"))?
        {
            self.update(pools)?;
            return Ok(pid);
        }
        if !self.exe.is_file() {
            return Err(SupervisorError::MissingBinary(self.exe.clone()));
        }
        background::create_private_dir(&self.dir)
            .map_err(io_err(format!("cannot create {}", self.dir.display())))?;
        let log_start = fs::metadata(self.log_path()).map_or(0, |m| m.len());
        pools
            .write(&self.pools_path())
            .map_err(io_err("cannot write pools.json"))?;
        let mut command = Command::new(&self.exe);
        command
            .arg("--dir")
            .arg(&self.dir)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = background::spawn_background(&mut command)
            .map_err(io_err(format!("cannot start {}", self.exe.display())))?;
        let pid = child.id();
        self.pid_file()
            .write(pid)
            .map_err(io_err("cannot record the supervisor's PID"))?;
        if let Err(e) = self.wait_ready(pools, Some(&mut child), log_start) {
            let _ = self.stop();
            return Err(e);
        }
        Ok(pid)
    }

    /// Hands the supervisor new pools and waits until all of their workers are listening.
    pub fn update(&self, pools: &Pools) -> Result<(), SupervisorError> {
        let log_start = fs::metadata(self.log_path()).map_or(0, |m| m.len());
        pools
            .write(&self.pools_path())
            .map_err(io_err("cannot write pools.json"))?;
        self.wait_ready(pools, None, log_start)
    }

    fn wait_ready(
        &self,
        pools: &Pools,
        mut child: Option<&mut Child>,
        log_start: u64,
    ) -> Result<(), SupervisorError> {
        let deadline = Instant::now() + self.ready_timeout;
        loop {
            if let Some(child) = child.as_deref_mut()
                && let Ok(Some(status)) = child.try_wait()
            {
                return Err(SupervisorError::Exited(format!(
                    "{status}\n{}",
                    self.recent_log(log_start, pools)
                )));
            }
            let ready = pools
                .ready_addresses()
                .iter()
                .all(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(200)).is_ok());
            if ready {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(SupervisorError::NotReady {
                    what: self.what,
                    seconds: self.ready_timeout.as_secs(),
                    log: self.recent_log(log_start, pools),
                });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Stops the supervisor and its workers: gracefully by removing `pools.json`, by
    /// terminating the (verified) process only if it doesn't exit in time.
    pub fn stop(&self) -> io::Result<Option<u32>> {
        let status = self.status()?;
        match fs::remove_file(self.pools_path()) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let ProcStatus::Running { pid } = status else {
            self.pid_file().remove()?;
            return Ok(None);
        };
        let exited =
            background::wait_until(|| !background::is_ours(pid, &self.exe), self.stop_timeout);
        if !exited {
            background::terminate(pid, &self.exe, self.stop_timeout)?;
        }
        self.pid_file().remove()?;
        Ok(Some(pid))
    }

    /// Supervisor log lines written after byte `offset`, plus the tails of `php-errors.log`
    /// and of each program's own log in `pools`.
    fn recent_log(&self, offset: u64, pools: &Pools) -> String {
        let tail = |path: &Path, from: u64, lines: usize| {
            fs::read(path)
                .ok()
                .and_then(|b| b.get(usize::try_from(from).ok()?..).map(<[u8]>::to_vec))
                .map(|b| {
                    let text = String::from_utf8_lossy(&b).into_owned();
                    let all: Vec<&str> = text.lines().collect();
                    all[all.len().saturating_sub(lines)..].join("\n")
                })
                .unwrap_or_default()
        };
        let mut text = tail(&self.log_path(), offset, 10);
        let errors = tail(&self.errors_path(), 0, 5);
        if !errors.is_empty() {
            text.push_str("\nphp-errors.log:\n");
            text.push_str(&errors);
        }
        for pool in &pools.pools {
            let Some(spec) = &pool.program_spec else {
                continue;
            };
            let lines = tail(&spec.log, 0, 8);
            if !lines.is_empty() {
                let _ = write!(text, "\n{}:\n{lines}", pool.id);
            }
        }
        text
    }
}
