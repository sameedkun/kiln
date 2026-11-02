//! Background processes Kiln starts and later checks or stops: Caddy, the php-cgi supervisor
//! ([ADR 0011] §11–12, [ADR 0014]).
//!
//! A [`PidFile`] records the process ID **and** the executable. Every check opens the process
//! and compares its executable with the recorded one, so a reused PID never makes Kiln report
//! or stop an unrelated process.
//!
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use std::time::Duration;

/// Whether a background process recorded in a [`PidFile`] is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcStatus {
    Stopped,
    Running {
        pid: u32,
    },
    /// A PID file is left over, but that process has exited (or is now something else).
    Stale {
        pid: u32,
    },
}

/// `<pid>\n<executable>\n`, naming the process that runs `exe`.
#[derive(Debug, Clone)]
pub struct PidFile {
    path: PathBuf,
    exe: PathBuf,
}

impl PidFile {
    #[must_use]
    pub fn new(path: PathBuf, exe: PathBuf) -> Self {
        Self { path, exe }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn exe(&self) -> &Path {
        &self.exe
    }

    /// The recorded PID. `Some(0)` stands for an unreadable file or one written for another
    /// executable, which is always stale.
    pub fn read(&self) -> io::Result<Option<u32>> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut lines = text.lines();
        let pid = lines.next().and_then(|l| l.trim().parse().ok());
        let exe = lines.next().map(PathBuf::from);
        Ok(match (pid, exe) {
            (Some(pid), Some(exe)) if exe == self.exe => Some(pid),
            _ => Some(0),
        })
    }

    pub fn write(&self, pid: u32) -> io::Result<()> {
        let temp = self.path.with_extension("tmp");
        fs::write(&temp, format!("{pid}\n{}\n", self.exe.display()))?;
        crate::rename_replace(&temp, &self.path)
    }

    pub fn remove(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    pub fn status(&self) -> io::Result<ProcStatus> {
        Ok(match self.read()? {
            None => ProcStatus::Stopped,
            Some(pid) if is_ours(pid, &self.exe) => ProcStatus::Running { pid },
            Some(pid) => ProcStatus::Stale { pid },
        })
    }

    /// Terminates the recorded process if it is running (verified), then removes the file.
    /// Returns the PID it stopped.
    pub fn terminate(&self, timeout: Duration) -> io::Result<Option<u32>> {
        let stopped = match self.status()? {
            ProcStatus::Running { pid } => {
                terminate(pid, &self.exe, timeout)?;
                Some(pid)
            }
            ProcStatus::Stale { .. } | ProcStatus::Stopped => None,
        };
        self.remove()?;
        Ok(stopped)
    }
}

/// Polls `done` every 50 ms until it is true or `timeout` passes; returns whether it became true.
pub fn wait_until(mut done: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while !done() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

pub use platform::{create_private_dir, is_ours, spawn_background, terminate};

/// A background process just started, watched while it comes up.
#[derive(Debug)]
pub enum Started {
    Child(std::process::Child),
    #[cfg(windows)]
    Process(crate::win::Process, u32),
}

impl Started {
    #[must_use]
    pub fn id(&self) -> u32 {
        match self {
            Self::Child(child) => child.id(),
            #[cfg(windows)]
            Self::Process(_, pid) => *pid,
        }
    }

    /// How it ended, if it has.
    pub fn exited(&mut self) -> Option<String> {
        match self {
            Self::Child(child) => child.try_wait().ok().flatten().map(|s| s.to_string()),
            #[cfg(windows)]
            Self::Process(process, _) => process
                .wait(Duration::ZERO)
                .ok()
                .flatten()
                .map(|code| format!("exit code: {code}")),
        }
    }
}

/// [`spawn_background`], but without administrator rights on Windows even when Kiln was
/// started elevated ([ADR 0040] §3): `command`'s program, arguments and folder are used, and it
/// gets no standard handles. Elsewhere the same as [`spawn_background`].
///
/// [ADR 0040]: ../../../docs/adr/0040-postgresql.md
pub fn spawn_background_without_admin(command: &mut std::process::Command) -> io::Result<Started> {
    #[cfg(windows)]
    {
        let args: Vec<&std::ffi::OsStr> = command.get_args().collect();
        let program = Path::new(command.get_program());
        let (process, pid) =
            crate::win::spawn_without_admin(program, &args, command.get_current_dir())?;
        Ok(Started::Process(process, pid))
    }
    #[cfg(not(windows))]
    {
        spawn_background(command).map(Started::Child)
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command};
    use std::time::Duration;

    use crate::win::{Process, keep_std_handles};

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    /// The folder inherits the per-user profile ACL (the user, SYSTEM, Administrators).
    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)
    }

    /// No console window, its own process group (so Ctrl+C in the terminal doesn't reach it),
    /// and out of the caller's job object where that's allowed, so it outlives `kiln`. It gets
    /// none of `kiln`'s own standard handles, only the ones `command` gives it (I-60).
    pub fn spawn_background(command: &mut Command) -> io::Result<Child> {
        let _own = keep_std_handles();
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

    /// Whether `pid` is running `exe`.
    #[must_use]
    pub fn is_ours(pid: u32, exe: &Path) -> bool {
        matches!(open_ours(pid, exe), Ok(Some(_)))
    }

    /// Terminates `pid` if (and only if) it is running `exe`, and waits for it to exit.
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

    /// Mode 0700: reachable by this user only.
    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }

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

    /// Whether `pid` is running `exe`.
    #[must_use]
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

    /// Terminates `pid` if (and only if) it is running `exe`, and waits for it to exit.
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
    fn a_missing_pid_file_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let file = PidFile::new(dir.path().join("x.pid"), dir.path().join("x"));
        assert_eq!(file.status().unwrap(), ProcStatus::Stopped);
        assert_eq!(file.terminate(Duration::from_secs(1)).unwrap(), None);
    }

    #[test]
    fn a_pid_file_for_another_program_is_stale_and_never_signalled() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("not-this-test");
        let file = PidFile::new(dir.path().join("x.pid"), exe);
        // This test process is alive, but isn't `exe`.
        let me = std::process::id();
        file.write(me).unwrap();
        assert_eq!(file.read().unwrap(), Some(me));
        assert_eq!(file.status().unwrap(), ProcStatus::Stale { pid: me });
        assert_eq!(file.terminate(Duration::from_secs(1)).unwrap(), None);
        assert!(!file.path().exists(), "stale file removed");
    }

    #[test]
    fn a_pid_file_written_for_another_executable_reads_as_stale() {
        let dir = tempfile::tempdir().unwrap();
        let a = PidFile::new(dir.path().join("x.pid"), dir.path().join("a"));
        let b = PidFile::new(dir.path().join("x.pid"), dir.path().join("b"));
        a.write(42).unwrap();
        assert_eq!(b.read().unwrap(), Some(0));
        fs::write(dir.path().join("x.pid"), "garbage").unwrap();
        assert_eq!(a.read().unwrap(), Some(0));
    }

    #[test]
    fn this_process_is_recognised_as_its_own_executable() {
        let me = std::env::current_exe().unwrap();
        assert!(is_ours(std::process::id(), &me));
    }
}
