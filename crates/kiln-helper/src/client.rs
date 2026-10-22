//! `kiln`'s side of a helper session: the same authenticated client over a Windows named
//! pipe (ADR 0009) or the stdin/stdout of a helper started by `pkexec` (ADR 0020).

use std::fmt::Debug;
use std::io::{Read, Write};
use std::path::Path;

use kiln_platform::DnsEntry;

use crate::proto::{
    Channel, ErrorCode, HostEntry, PROTOCOL_VERSION, ProtoError, Request, Response, SessionKey,
};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("cannot reach the helper: {0}")]
    Connect(std::io::Error),
    #[error("cannot start the helper: {0}")]
    Launch(std::io::Error),
    #[error(transparent)]
    Protocol(#[from] ProtoError),
    #[error("helper refused the request ({code:?}): {message}")]
    Refused { code: ErrorCode, message: String },
    #[error("helper speaks protocol {0}, this kiln speaks {PROTOCOL_VERSION}")]
    VersionMismatch(u32),
    #[error("unexpected helper response: {0:?}")]
    Unexpected(Box<Response>),
}

/// Result of a successful `HostsSetBlock`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostsWritten {
    pub created: bool,
    pub deleted: bool,
    pub unchanged: bool,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

/// A byte stream a session can run over.
pub trait Stream: Read + Write + Debug {}
impl<T: Read + Write + Debug> Stream for T {}

/// An authenticated session with a running helper.
#[derive(Debug)]
pub struct HelperClient {
    channel: Channel<Box<dyn Stream>>,
}

impl HelperClient {
    /// A session over `stream`, checked with a `Ping` (the helper must speak our protocol).
    pub fn over(stream: impl Stream + 'static, key: SessionKey) -> Result<Self, ClientError> {
        let mut client = Self {
            channel: Channel::new(Box::new(stream), key),
        };
        client.ping()?;
        Ok(client)
    }

    /// Connects to the helper's pipe and checks it speaks our protocol.
    #[cfg(windows)]
    pub fn connect(
        pipe_name: &str,
        key: SessionKey,
        timeout: std::time::Duration,
    ) -> Result<Self, ClientError> {
        let file = kiln_sys::win::connect_pipe(pipe_name, timeout).map_err(ClientError::Connect)?;
        Self::over(file, key)
    }

    fn call(&mut self, request: &Request) -> Result<Response, ClientError> {
        match self.channel.call(request)? {
            Response::Error { code, message } => Err(ClientError::Refused { code, message }),
            other => Ok(other),
        }
    }

    /// Returns the helper's process id.
    pub fn ping(&mut self) -> Result<u32, ClientError> {
        match self.call(&Request::Ping)? {
            Response::Pong {
                protocol,
                helper_pid,
            } if protocol == PROTOCOL_VERSION => Ok(helper_pid),
            Response::Pong { protocol, .. } => Err(ClientError::VersionMismatch(protocol)),
            other => Err(ClientError::Unexpected(Box::new(other))),
        }
    }

    /// Makes Kiln's hosts block contain exactly `entries` under `tld` (empty removes it).
    /// `remove_file_if_empty` deletes a file left with zero bytes (undoing a creation).
    pub fn hosts_set_block(
        &mut self,
        tld: &str,
        entries: &[DnsEntry],
        remove_file_if_empty: bool,
    ) -> Result<HostsWritten, ClientError> {
        let entries = entries
            .iter()
            .map(|e| HostEntry {
                name: e.name.to_string(),
                ip: e.ip,
            })
            .collect();
        match self.call(&Request::HostsSetBlock {
            tld: tld.to_owned(),
            entries,
            remove_file_if_empty,
        })? {
            Response::HostsWritten {
                created,
                deleted,
                unchanged,
                bytes_before,
                bytes_after,
            } => Ok(HostsWritten {
                created,
                deleted,
                unchanged,
                bytes_before,
                bytes_after,
            }),
            other => Err(ClientError::Unexpected(Box::new(other))),
        }
    }

    /// Ends the session; the helper exits afterwards.
    pub fn shutdown(mut self) -> Result<(), ClientError> {
        match self.call(&Request::Shutdown)? {
            Response::Bye => Ok(()),
            other => Err(ClientError::Unexpected(Box::new(other))),
        }
    }
}

/// Starts `helper_exe` through UAC with a fresh pipe name and session key, then connects.
/// One consent prompt; the helper serves this session only.
#[cfg(windows)]
pub fn launch(
    helper_exe: &Path,
    timeout: std::time::Duration,
) -> Result<(kiln_sys::win::ElevatedProcess, HelperClient), ClientError> {
    use crate::proto::{new_pipe_name, new_session_key, to_hex};
    let key = new_session_key();
    let pipe = new_pipe_name();
    let pid = std::process::id().to_string();
    let key_hex = to_hex(&key);
    let process = kiln_sys::win::spawn_elevated(
        helper_exe,
        &["--pipe", &pipe, "--client-pid", &pid, "--key", &key_hex],
    )
    .map_err(ClientError::Launch)?;
    let client = HelperClient::connect(&pipe, key, timeout)?;
    Ok((process, client))
}

/// How the helper is started as root on this machine: `pkexec` (polkit), else `sudo`
/// ([ADR 0020] §3).
///
/// [ADR 0020]: ../../../docs/adr/0020-linux-dns-and-privilege.md
#[cfg(unix)]
fn root_launcher() -> Option<(std::path::PathBuf, &'static [&'static str])> {
    // Fixed locations, not PATH: what asks for the administrator password must be the
    // system's own program.
    let find = |name: &str| {
        ["/usr/bin", "/bin", "/usr/local/bin"]
            .iter()
            .map(|dir| Path::new(dir).join(name))
            .find(|path| path.is_file())
    };
    find("pkexec")
        .map(|p| (p, &[][..]))
        .or_else(|| find("sudo").map(|p| (p, &["--"][..])))
}

/// Starts `helper_exe --stdio` as root (one password prompt), sends the session key on its
/// stdin and checks it answers. A declined prompt is a `Launch` error with
/// `PermissionDenied`.
#[cfg(unix)]
pub fn launch(helper_exe: &Path) -> Result<(std::process::Child, HelperClient), ClientError> {
    use std::process::{Command, Stdio};

    use crate::proto::{Joined, new_session_key, write_key_line};

    let (launcher, launcher_args) = root_launcher().ok_or_else(|| {
        ClientError::Launch(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "neither pkexec (polkit) nor sudo is installed; install one, e.g. `sudo apt install pkexec`",
        ))
    })?;
    let mut child = Command::new(&launcher)
        .args(launcher_args)
        .arg(helper_exe)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(ClientError::Launch)?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        unreachable!("both are piped above");
    };
    let key = new_session_key();
    let started = write_key_line(&mut stdin, &key)
        .map_err(|e| ClientError::Protocol(e.into()))
        .and_then(|()| HelperClient::over(Joined::new(stdout, stdin), key));
    match started {
        Ok(client) => Ok((child, client)),
        Err(e) => {
            // pkexec exits 126 when the dialog is dismissed and 127 when authorization
            // fails; sudo exits 1 after failed password attempts.
            let declined = child
                .wait()
                .ok()
                .and_then(|status| status.code())
                .is_some_and(|code| {
                    matches!(code, 126 | 127) || (code == 1 && launcher.ends_with("sudo"))
                });
            if declined {
                Err(ClientError::Launch(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "administrator authentication was declined",
                )))
            } else {
                Err(e)
            }
        }
    }
}
