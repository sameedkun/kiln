//! Linux process helpers ([ADR 0022] §6).
//!
//! [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Makes the process `command` starts receive SIGTERM when the thread that started it ends
/// (in practice, when the supervisor exits or dies). On Windows a kill-on-close job does
/// this; PHP-FPM shuts its children down cleanly on SIGTERM.
pub fn terminate_with_parent(command: &mut Command) {
    let hook = || -> io::Result<()> {
        // SAFETY: PR_SET_PDEATHSIG with a valid signal number has no memory effects.
        if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // The parent may have died between fork and prctl: then nobody would send the
        // signal, so give up now (ESRCH: no such process). No allocation here.
        // SAFETY: getppid has no preconditions.
        if unsafe { libc::getppid() } == 1 {
            return Err(io::Error::from_raw_os_error(libc::ESRCH));
        }
        Ok(())
    };
    // SAFETY: `hook` runs in the child between fork and exec, and calls only
    // async-signal-safe functions (`prctl`, `getppid`) without allocating.
    unsafe {
        command.pre_exec(hook);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_started_this_way_still_runs() {
        let mut command = Command::new("true");
        terminate_with_parent(&mut command);
        assert!(command.status().unwrap().success());
    }
}
