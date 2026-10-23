//! Starting real PHP workers: php-cgi processes ([ADR 0014] §3–5) and PHP-FPM masters
//! ([ADR 0022]).
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md
//! [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md

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

impl Launcher for PhpCgiLauncher {
    fn launch(&mut self, pool: &PoolSpec, worker: &WorkerSpec) -> io::Result<Box<dyn Worker>> {
        if pool.kind == PoolKind::Fpm {
            return self.launch_fpm(pool);
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
        // `kill`, like kiln-sys::background: no signal code of our own outside kiln-sys.
        let asked = Command::new("kill")
            .arg("-TERM")
            .arg(self.0.id().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if asked {
            let deadline = std::time::Instant::now() + FPM_STOP_TIMEOUT;
            while std::time::Instant::now() < deadline {
                if !matches!(self.0.try_wait(), Ok(None)) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
