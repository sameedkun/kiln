//! Starting real php-cgi workers ([ADR 0014] §3–5).
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::pools::{PoolSpec, WorkerSpec};
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

impl Launcher for PhpCgiLauncher {
    fn launch(&mut self, pool: &PoolSpec, worker: &WorkerSpec) -> io::Result<Box<dyn Worker>> {
        let mut command = Command::new(&pool.php_cgi);
        command
            .arg("-b")
            .arg(format!("127.0.0.1:{}", worker.port))
            .arg("-c")
            .arg(&pool.ini)
            .env("PHP_FCGI_MAX_REQUESTS", worker.max_requests.to_string())
            .current_dir(pool.php_cgi.parent().unwrap_or_else(|| Path::new(".")))
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
