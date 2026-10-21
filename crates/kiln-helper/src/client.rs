//! `kiln`'s side of a helper session.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use kiln_platform::DnsEntry;
use kiln_sys::win::{ElevatedProcess, connect_pipe, spawn_elevated};

use crate::proto::{
    Channel, ErrorCode, HostEntry, PROTOCOL_VERSION, ProtoError, Request, Response, SessionKey,
    new_pipe_name, new_session_key, to_hex,
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
    pub unchanged: bool,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

/// An authenticated session with a running helper.
#[derive(Debug)]
pub struct HelperClient {
    channel: Channel<File>,
}

impl HelperClient {
    /// Connects to the helper's pipe and checks it speaks our protocol.
    pub fn connect(
        pipe_name: &str,
        key: SessionKey,
        timeout: Duration,
    ) -> Result<Self, ClientError> {
        let file = connect_pipe(pipe_name, timeout).map_err(ClientError::Connect)?;
        let mut client = Self {
            channel: Channel::new(file, key),
        };
        client.ping()?;
        Ok(client)
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
    pub fn hosts_set_block(
        &mut self,
        tld: &str,
        entries: &[DnsEntry],
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
        })? {
            Response::HostsWritten {
                created,
                unchanged,
                bytes_before,
                bytes_after,
            } => Ok(HostsWritten {
                created,
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
pub fn launch(
    helper_exe: &Path,
    timeout: Duration,
) -> Result<(ElevatedProcess, HelperClient), ClientError> {
    let key = new_session_key();
    let pipe = new_pipe_name();
    let pid = std::process::id().to_string();
    let key_hex = to_hex(&key);
    let process = spawn_elevated(
        helper_exe,
        &["--pipe", &pipe, "--client-pid", &pid, "--key", &key_hex],
    )
    .map_err(ClientError::Launch)?;
    let client = HelperClient::connect(&pipe, key, timeout)?;
    Ok((process, client))
}
