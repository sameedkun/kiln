//! Starting real workers: php-cgi processes ([ADR 0014] §3–5), PHP-FPM masters
//! ([ADR 0022]) and other programs, such as service instances ([ADR 0036]).
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md
//! [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
//! [ADR 0036]: ../../../docs/adr/0036-service-framework.md

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::pools::{PoolKind, PoolSpec, WorkerSpec};
use crate::reconcile::{Launcher, Worker};

/// Starts `php-cgi -b 127.0.0.1:<port> -c <php.ini>` with the worker's recycle limit. On
/// Windows every worker joins a kill-on-close job held by this launcher, so workers can't
/// outlive the supervisor.
#[derive(Debug)]
pub struct PhpCgiLauncher {
    errors: File,
    #[cfg(windows)]
    job: kiln_sys::win::KillOnCloseJob,
}

impl PhpCgiLauncher {
    /// Worker stderr (PHP start-up errors) is appended to `error_log`.
    pub fn new(error_log: &Path) -> io::Result<Self> {
        Ok(Self {
            errors: OpenOptions::new()
                .create(true)
                .append(true)
                .open(error_log)?,
            #[cfg(windows)]
            job: kiln_sys::win::KillOnCloseJob::new()?,
        })
    }
}

impl PhpCgiLauncher {
    /// `php-fpm --nodaemonize --fpm-config <conf> -c <php.ini>` ([ADR 0022] §5–6): one master
    /// on the pool's port, managing its own children. On Linux it gets SIGTERM if the
    /// supervisor dies, and shuts its children down with it.
    ///
    /// [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md
    fn launch_fpm(&self, pool: &PoolSpec) -> io::Result<Box<dyn Worker>> {
        let config = pool.fpm_config.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "an FPM pool needs fpm_config")
        })?;
        let mut command = Command::new(&pool.program);
        command
            .arg("--nodaemonize")
            .arg("--fpm-config")
            .arg(config)
            .arg("-c")
            .arg(&pool.ini)
            .current_dir(pool.program.parent().unwrap_or_else(|| Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(self.errors.try_clone()?);
        #[cfg(target_os = "linux")]
        kiln_sys::linux::terminate_with_parent(&mut command);
        Ok(Box::new(FpmMaster(command.spawn()?)))
    }
}

impl PhpCgiLauncher {
    /// Any other program ([ADR 0036] §1), such as a service instance: its own arguments,
    /// environment and working folder, its output appended to its own log. It joins the
    /// same kill-on-close job as php-cgi on Windows, and gets SIGTERM if the supervisor dies
    /// on Linux.
    ///
    /// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
    #[cfg_attr(not(windows), allow(clippy::unused_self))] // the job exists on Windows only
    fn launch_program(&self, pool: &PoolSpec) -> io::Result<Box<dyn Worker>> {
        let spec = pool.program_spec.clone().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a program needs program_spec")
        })?;
        let log = open_log(&spec.log)?;
        let cwd = spec
            .cwd
            .clone()
            .or_else(|| pool.program.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| ".".into());
        let mut command = Command::new(&pool.program);
        command
            .args(&spec.args)
            .envs(&spec.env)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        #[cfg(target_os = "linux")]
        kiln_sys::linux::terminate_with_parent(&mut command);
        let child = command.spawn()?;
        #[cfg(windows)]
        if let Err(e) = self.job.assign(&child) {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        // Its own job too, nested in the supervisor's, so stopping it ends what it started
        // (a queue worker's job processes, Vite's helpers) and not only itself ([ADR 0060]
        // §3). Without one, it still ends with the supervisor.
        //
        // [ADR 0060]: ../../../docs/adr/0060-dev-processes.md
        #[cfg(windows)]
        let tree = kiln_sys::win::KillOnCloseJob::new()
            .ok()
            .filter(|job| job.assign(&child).is_ok());
        Ok(Box::new(ProgramWorker {
            child,
            spec,
            cwd,
            log,
            #[cfg(windows)]
            tree,
        }))
    }
}

impl Launcher for PhpCgiLauncher {
    fn launch(&mut self, pool: &PoolSpec, worker: &WorkerSpec) -> io::Result<Box<dyn Worker>> {
        match pool.kind {
            PoolKind::Fpm => return self.launch_fpm(pool),
            PoolKind::Program => return self.launch_program(pool),
            PoolKind::Cgi => {}
        }
        let mut command = Command::new(&pool.program);
        command
            .arg("-b")
            .arg(format!("127.0.0.1:{}", worker.port))
            .arg("-c")
            .arg(&pool.ini)
            .env("PHP_FCGI_MAX_REQUESTS", worker.max_requests.to_string())
            .current_dir(pool.program.parent().unwrap_or_else(|| Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(self.errors.try_clone()?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let child = command.spawn()?;
        #[cfg(windows)]
        if let Err(e) = self.job.assign(&child) {
            // Never run a worker that could outlive the supervisor.
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        Ok(Box::new(ChildWorker(child)))
    }
}

struct ChildWorker(Child);

impl Worker for ChildWorker {
    fn exit_status(&mut self) -> Option<bool> {
        match self.0.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => Some(status.success()),
            // The handle is unusable: treat the worker as gone, and as a failure.
            Err(_) => Some(false),
        }
    }

    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A PHP-FPM master. Killing it outright would leave its children serving on, so it is
/// asked to stop (SIGTERM, which FPM passes on to its children) and only killed if it hasn't
/// exited within [`FPM_STOP_TIMEOUT`].
struct FpmMaster(Child);

/// How long a PHP-FPM master gets to stop its children and exit.
const FPM_STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl Worker for FpmMaster {
    fn exit_status(&mut self) -> Option<bool> {
        match self.0.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => Some(status.success()),
            Err(_) => Some(false),
        }
    }

    fn stop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(None)) {
            return;
        }
        if ask_to_terminate(&self.0) && exits_within(&mut self.0, FPM_STOP_TIMEOUT) {
            return;
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Sends SIGTERM through `kill`, like `kiln-sys::background`: no signal code of our own outside
/// kiln-sys. Returns whether it was sent. Not available on Windows.
fn ask_to_terminate(child: &Child) -> bool {
    if cfg!(windows) {
        return false;
    }
    Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether `child` exits within `timeout`.
fn exits_within(child: &mut Child, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// A program ([ADR 0036] §1). Stopping it runs its stop command first (a database's own
/// shutdown), then waits; on Linux it is then asked to terminate; only if it still runs is
/// it killed, since killing a database can cost it data.
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
struct ProgramWorker {
    child: Child,
    spec: crate::pools::ProgramSpec,
    cwd: std::path::PathBuf,
    /// Its output file, shared with its stop command (see [`open_log`]).
    log: File,
    /// Its process tree: closed after it stops, ending anything it left running.
    #[cfg(windows)]
    tree: Option<kiln_sys::win::KillOnCloseJob>,
}

/// How long a program gets to exit after each way of asking it to.
const PROGRAM_STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl Worker for ProgramWorker {
    fn exit_status(&mut self) -> Option<bool> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => Some(status.success()),
            Err(_) => Some(false),
        }
    }

    fn stop(&mut self) {
        self.stop_program();
        #[cfg(windows)]
        {
            self.tree = None;
        }
    }
}

impl ProgramWorker {
    /// Asks the program to stop (its stop command, then SIGTERM where there is one), and
    /// kills it if it doesn't.
    fn stop_program(&mut self) {
        if !matches!(self.child.try_wait(), Ok(None)) {
            return;
        }
        if let Some((program, args)) = self
            .spec
            .stop_command
            .as_ref()
            .and_then(|c| c.split_first())
        {
            let log = self.log.try_clone();
            let mut command = Command::new(program);
            command
                .args(args)
                .envs(&self.spec.env)
                .current_dir(&self.cwd)
                .stdin(Stdio::null());
            if let Ok(log) = log {
                if let Ok(out) = log.try_clone() {
                    command.stdout(out);
                }
                command.stderr(log);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                command.creation_flags(CREATE_NO_WINDOW);
            }
            if let Ok(mut stopper) = command.spawn() {
                if !exits_within(&mut stopper, PROGRAM_STOP_TIMEOUT) {
                    let _ = stopper.kill();
                    let _ = stopper.wait();
                }
                if exits_within(&mut self.child, PROGRAM_STOP_TIMEOUT) {
                    return;
                }
            }
        }
        if ask_to_terminate(&self.child) && exits_within(&mut self.child, PROGRAM_STOP_TIMEOUT) {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A program's output file, opened for writing at its end rather than in append mode. On
/// Windows an append-mode handle has no plain write access, and programs built on the
/// MSYS2 or Cygwin runtime (Redis, [ADR 0041]) write nothing through it. Every writer gets
/// a clone of this one handle, so they share one position and don't overwrite each other.
///
/// [ADR 0041]: ../../../docs/adr/0041-redis.md
fn open_log(path: &Path) -> io::Result<File> {
    use std::io::{Seek, SeekFrom};
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    file.seek(SeekFrom::End(0))?;
    Ok(file)
}
