//! The helper's side of a session: on Windows over a private named pipe (ADR 0009 §2–6), on
//! Linux over the stdin and stdout `pkexec` gives it (ADR 0020 §3–6). The requests and their
//! handling are the same on both.

use std::io::{Read, Write};
use std::path::Path;

use crate::proto::{
    Channel, ErrorCode, Joined, PROTOCOL_VERSION, ProtoError, Request, Response, read_key_line,
};
use kiln_platform::trust_system::Anchors;

use crate::{hosts, system_trust, validate};

/// What happened in a completed session.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub requests: u64,
    pub hosts_writes: u64,
}

fn handle(
    request: Request,
    summary: &mut Summary,
    hosts_path: &Path,
    anchors: Option<&Anchors>,
) -> Response {
    match request {
        Request::Ping => Response::Pong {
            protocol: PROTOCOL_VERSION,
            helper_pid: std::process::id(),
        },
        Request::HostsSetBlock {
            tld,
            entries,
            remove_file_if_empty,
        } => {
            let entries = match validate::hosts_entries(&tld, &entries) {
                Ok(e) => e,
                Err(message) => {
                    return Response::Error {
                        code: ErrorCode::InvalidRequest,
                        message,
                    };
                }
            };
            match hosts::set_block(hosts_path, &entries, remove_file_if_empty) {
                Ok(out) => {
                    summary.hosts_writes += u64::from(!out.unchanged);
                    Response::HostsWritten {
                        created: out.created,
                        deleted: out.deleted,
                        unchanged: out.unchanged,
                        bytes_before: out.bytes_before,
                        bytes_after: out.bytes_after,
                    }
                }
                Err(f) => Response::Error {
                    code: f.code,
                    message: f.message,
                },
            }
        }
        Request::TrustSystemCa { pem } => trust_result(system_trust::trust(anchors, &pem)),
        Request::UntrustSystemCa { sha256 } => {
            trust_result(system_trust::untrust(anchors, &sha256))
        }
        Request::Shutdown => Response::Bye,
    }
}

fn trust_result(result: Result<(std::path::PathBuf, bool), hosts::Failure>) -> Response {
    match result {
        Ok((file, changed)) => Response::SystemTrustChanged {
            file: file.display().to_string(),
            changed,
        },
        Err(f) => Response::Error {
            code: f.code,
            message: f.message,
        },
    }
}

/// Answers authenticated requests on `channel` until `Shutdown` or the end of input. Any
/// authentication or ordering failure ends the session with an error.
pub fn run_session<S: Read + Write>(
    channel: &mut Channel<S>,
    hosts_path: &Path,
    anchors: Option<&Anchors>,
) -> Result<Summary, ProtoError> {
    let mut summary = Summary::default();
    loop {
        let (seq, request) = match channel.next_request() {
            Ok(r) => r,
            Err(ProtoError::Closed) => return Ok(summary),
            Err(e) => return Err(e),
        };
        summary.requests += 1;
        let shutdown = request == Request::Shutdown;
        let response = handle(request, &mut summary, hosts_path, anchors);
        channel.respond(seq, &response)?;
        if shutdown {
            return Ok(summary);
        }
    }
}

/// A session on `input` / `output` (the helper's stdin and stdout under `pkexec`): the key
/// line first, then authenticated frames ([ADR 0020] §4).
///
/// [ADR 0020]: ../../../docs/adr/0020-linux-dns-and-privilege.md
pub fn serve_stdio<R: Read, W: Write>(
    mut input: R,
    output: W,
    hosts_path: &Path,
    anchors: Option<&Anchors>,
) -> Result<Summary, ProtoError> {
    let key = read_key_line(&mut input)?;
    let mut channel = Channel::new(Joined::new(input, output), key);
    run_session(&mut channel, hosts_path, anchors)
}

#[cfg(windows)]
pub use pipe::{ServeError, ServerConfig, pipe_sddl, serve};

#[cfg(windows)]
mod pipe {
    use std::path::PathBuf;

    use kiln_sys::win::{ServerPipe, process_user_sid};

    use super::{Summary, run_session};
    use crate::proto::{Channel, ProtoError, SessionKey};

    /// Everything a pipe session needs. `hosts_path` is configurable for tests; the binary
    /// always passes the path derived from the system directory.
    #[derive(Debug, Clone)]
    pub struct ServerConfig {
        pub pipe_name: String,
        pub client_pid: u32,
        pub key: SessionKey,
        pub hosts_path: PathBuf,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum ServeError {
        #[error("cannot identify the user of client process {pid}: {source}")]
        ClientIdentity { pid: u32, source: std::io::Error },
        #[error("cannot create the helper pipe (already exists or access denied): {0}")]
        CreatePipe(std::io::Error),
        #[error("pipe connection failed: {0}")]
        Connect(std::io::Error),
        #[error("refusing connection from process {actual}; expected {expected}")]
        WrongClient { expected: u32, actual: u32 },
        #[error(transparent)]
        Protocol(#[from] ProtoError),
    }

    /// The pipe's security descriptor: only SYSTEM and the client's user, protected from
    /// inheritance, with a medium integrity label so a non-elevated client can write.
    #[must_use]
    pub fn pipe_sddl(user_sid: &str) -> String {
        format!("D:P(A;;GA;;;SY)(A;;GRGW;;;{user_sid})S:(ML;;NW;;;ME)")
    }

    /// Runs one session: create the private pipe, accept exactly one client, check it is the
    /// process that launched us, then answer authenticated requests until `Shutdown` or
    /// disconnect.
    pub fn serve(config: &ServerConfig) -> Result<Summary, ServeError> {
        let user_sid =
            process_user_sid(config.client_pid).map_err(|source| ServeError::ClientIdentity {
                pid: config.client_pid,
                source,
            })?;
        let pipe = ServerPipe::create(&config.pipe_name, &pipe_sddl(&user_sid))
            .map_err(ServeError::CreatePipe)?;
        pipe.wait_for_client().map_err(ServeError::Connect)?;
        let actual = pipe.client_process_id().map_err(ServeError::Connect)?;
        if actual != config.client_pid {
            return Err(ServeError::WrongClient {
                expected: config.client_pid,
                actual,
            });
        }
        let mut channel = Channel::new(pipe.into_file(), config.key);
        Ok(run_session(&mut channel, &config.hosts_path, None)?)
    }
}
