//! The app's only way into Kiln ([ADR 0054] §3): `kilnd`'s API. Each call opens its own
//! connection, so a slow read never queues a click; one thread holds a subscribed connection
//! and passes the daemon's events on to the pages.
//!
//! [ADR 0054]: ../../../../docs/adr/0054-desktop-app-shell.md

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use kiln_daemon::{Client, ClientError};
use serde::Serialize;
use serde_json::Value;

/// The oldest API the app works with: version 2 has the reads and jobs it needs (ADR 0053).
pub const MIN_API: u32 = 2;

/// How long a call may wait for its answer: the API's longest read, and some.
const CALL_TIMEOUT: Duration = Duration::from_secs(150);

/// What the pages know about the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonState {
    Starting,
    Running,
    /// Its API is older than [`MIN_API`]: Kiln needs updating.
    TooOld,
    Unreachable,
}

#[derive(Debug)]
pub struct Bridge {
    pub socket: PathBuf,
    /// The `kiln` program next to the app.
    pub kiln: PathBuf,
    state: Mutex<DaemonState>,
}

impl Bridge {
    #[must_use]
    pub fn new(socket: PathBuf, kiln: PathBuf) -> Self {
        Self {
            socket,
            kiln,
            state: Mutex::new(DaemonState::Starting),
        }
    }

    #[must_use]
    pub fn state(&self) -> DaemonState {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the state; `true` if it changed.
    pub fn set_state(&self, state: DaemonState) -> bool {
        let mut now = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = *now != state;
        *now = state;
        changed
    }
}

/// The error as the pages show it.
#[must_use]
pub fn describe(error: ClientError) -> String {
    match error {
        ClientError::Rpc(error) => error.message,
        other => other.to_string(),
    }
}

/// One call on a connection of its own.
pub fn call(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    let mut client = Client::connect(socket).map_err(describe)?;
    client.set_timeout(CALL_TIMEOUT).map_err(describe)?;
    client.call(method, params).map_err(describe)
}

/// What the daemon's `kiln.hello` means for the app.
#[must_use]
pub fn state_for_api(api: u32) -> DaemonState {
    if api >= MIN_API {
        DaemonState::Running
    } else {
        DaemonState::TooOld
    }
}

/// A read timeout on the event connection: no event yet, not a broken connection.
#[must_use]
pub fn is_quiet(error: &ClientError) -> bool {
    matches!(error, ClientError::Io(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
}

/// Starts `kilnd` through `kiln daemon start`, without a window, and waits for it.
pub fn start_daemon(kiln: &Path) -> Result<(), String> {
    let mut command = Command::new(kiln);
    command
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot run {}: {e}", kiln.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Follows the daemon's events for as long as the app runs: connects (starting `kilnd`
/// once if it isn't running), subscribes, and hands every notification to `on_event`.
/// `on_state` hears each change of [`DaemonState`]; `on_connected` each new subscription,
/// after which the pages read everything again.
pub fn follow(
    bridge: &Bridge,
    on_state: &dyn Fn(DaemonState),
    on_connected: &dyn Fn(),
    on_event: &dyn Fn(Value),
) {
    let mut tried_start = false;
    let set = |state| {
        if bridge.set_state(state) {
            on_state(state);
        }
    };
    loop {
        match Client::connect(&bridge.socket) {
            Ok(mut client) => match client.hello() {
                Ok(hello) if state_for_api(hello.api) == DaemonState::Running => {
                    if client.subscribe().is_ok() {
                        // If it goes away later, starting it once more is worth a try.
                        tried_start = false;
                        set(DaemonState::Running);
                        on_connected();
                        loop {
                            match client.next_event(Duration::from_secs(30)) {
                                Ok(note) => {
                                    on_event(serde_json::to_value(note).unwrap_or_default());
                                }
                                Err(e) if is_quiet(&e) => {}
                                Err(_) => break,
                            }
                        }
                    }
                }
                Ok(_) => {
                    set(DaemonState::TooOld);
                    std::thread::sleep(Duration::from_secs(15));
                }
                Err(_) => set(DaemonState::Unreachable),
            },
            Err(ClientError::NotRunning) if !tried_start => {
                tried_start = true;
                set(DaemonState::Starting);
                if start_daemon(&bridge.kiln).is_err() {
                    set(DaemonState::Unreachable);
                }
                continue;
            }
            Err(_) => set(DaemonState::Unreachable),
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_api_is_refused() {
        assert_eq!(state_for_api(1), DaemonState::TooOld);
        assert_eq!(state_for_api(2), DaemonState::Running);
        assert_eq!(state_for_api(3), DaemonState::Running);
    }

    #[test]
    fn a_read_timeout_is_quiet_anything_else_reconnects() {
        assert!(is_quiet(&ClientError::Io(std::io::Error::from(
            ErrorKind::TimedOut
        ))));
        assert!(is_quiet(&ClientError::Io(std::io::Error::from(
            ErrorKind::WouldBlock
        ))));
        assert!(!is_quiet(&ClientError::Io(std::io::Error::from(
            ErrorKind::ConnectionReset
        ))));
        assert!(!is_quiet(&ClientError::NotRunning));
    }

    #[test]
    fn states_report_changes_only() {
        let bridge = Bridge::new(PathBuf::from("x"), PathBuf::from("y"));
        assert!(!bridge.set_state(DaemonState::Starting));
        assert!(bridge.set_state(DaemonState::Running));
        assert_eq!(bridge.state(), DaemonState::Running);
    }

    #[test]
    fn states_are_kebab_case_for_the_pages() {
        assert_eq!(
            serde_json::to_value(DaemonState::TooOld).unwrap(),
            "too-old"
        );
    }
}
