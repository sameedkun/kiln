//! A lazily started, shared helper session that implements
//! [`kiln_platform::hosts_op::PrivilegedHosts`]: the first write that actually needs
//! elevation launches `kiln-helper` (one UAC prompt); later writes in the same batch reuse it.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use kiln_core::Tld;
use kiln_ops::OpError;
use kiln_platform::DnsEntry;
use kiln_platform::hosts_op::PrivilegedHosts;
use kiln_sys::win::ElevatedProcess;

use crate::client::{ClientError, HelperClient, launch};

/// `kiln-helper.exe` next to the running executable (how Kiln is installed and how cargo
/// lays out `target/<profile>`).
pub fn default_helper_exe() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("kiln-helper.exe"))
}

#[derive(Debug)]
enum Source {
    /// Launch `helper_exe` through UAC on first use.
    Launch {
        helper_exe: PathBuf,
        timeout: Duration,
    },
    /// Use an already connected client (tests, or a caller that launched it).
    Connected,
}

#[derive(Debug, Default)]
struct State {
    client: Option<HelperClient>,
    process: Option<ElevatedProcess>,
    launches: u32,
    writes: u32,
}

/// One helper session shared by the operations of a batch.
#[derive(Debug)]
pub struct HelperSession {
    source: Source,
    state: RefCell<State>,
}

fn op_error(e: &ClientError) -> OpError {
    match e {
        ClientError::Launch(io) if io.kind() == std::io::ErrorKind::PermissionDenied => {
            OpError::Failed("administrator approval was declined; nothing was changed".into())
        }
        other => OpError::Failed(format!("privileged helper: {other}")),
    }
}

impl HelperSession {
    /// Launches `helper_exe` elevated on the first privileged write.
    #[must_use]
    pub fn launching(helper_exe: PathBuf) -> Self {
        Self {
            source: Source::Launch {
                helper_exe,
                timeout: Duration::from_secs(60),
            },
            state: RefCell::default(),
        }
    }

    /// Uses an already connected helper.
    #[must_use]
    pub fn connected(client: HelperClient) -> Self {
        Self {
            source: Source::Connected,
            state: RefCell::new(State {
                client: Some(client),
                ..State::default()
            }),
        }
    }

    /// How many times the helper was launched (UAC prompts shown); 0 if none was needed.
    #[must_use]
    pub fn launches(&self) -> u32 {
        self.state.borrow().launches
    }

    /// Privileged writes sent through this session.
    #[must_use]
    pub fn writes(&self) -> u32 {
        self.state.borrow().writes
    }

    /// Ends the session (the helper exits). Safe to call more than once; also run on drop.
    pub fn finish(&self) -> Result<(), ClientError> {
        let mut state = self.state.borrow_mut();
        let result = state.client.take().map_or(Ok(()), HelperClient::shutdown);
        if let Some(process) = state.process.take() {
            let _ = process.wait(Duration::from_secs(10));
        }
        result
    }
}

impl PrivilegedHosts for HelperSession {
    fn set_block(
        &self,
        tld: &Tld,
        entries: &[DnsEntry],
        remove_file_if_empty: bool,
    ) -> Result<(), OpError> {
        let mut state = self.state.borrow_mut();
        if state.client.is_none() {
            match &self.source {
                Source::Launch {
                    helper_exe,
                    timeout,
                } => {
                    let (process, client) =
                        launch(helper_exe, *timeout).map_err(|e| op_error(&e))?;
                    state.launches += 1;
                    state.process = Some(process);
                    state.client = Some(client);
                }
                Source::Connected => {
                    return Err(OpError::Failed("the helper session has ended".into()));
                }
            }
        }
        let client = state.client.as_mut().expect("set above");
        client
            .hosts_set_block(tld.as_str(), entries, remove_file_if_empty)
            .map_err(|e| op_error(&e))?;
        state.writes += 1;
        Ok(())
    }
}

impl Drop for HelperSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
