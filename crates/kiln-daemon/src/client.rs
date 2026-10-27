//! A client for the daemon's API, used by `kiln daemon …` and the tests.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::proto::{Hello, MAX_LINE, Notification, Request, Response, RpcError};
use crate::transport::{self, Stream};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Kiln's daemon isn't running (`kiln daemon start` starts it)")]
    NotRunning,
    #[error("cannot talk to Kiln's daemon: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Rpc(RpcError),
    #[error("unexpected answer from Kiln's daemon: {0}")]
    Protocol(String),
}

/// One connection to the daemon.
#[derive(Debug)]
pub struct Client {
    reader: BufReader<Stream>,
    writer: Stream,
    next_id: u64,
}

impl Client {
    /// Connects to the socket at `path`. A missing socket or a refused connection means the
    /// daemon isn't running.
    pub fn connect(path: &Path) -> Result<Self, ClientError> {
        let stream = transport::connect(path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => ClientError::NotRunning,
            _ => ClientError::Io(e),
        })?;
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        Ok(Self {
            writer: stream.try_clone()?,
            reader: BufReader::new(stream),
            next_id: 1,
        })
    }

    /// Calls `method` and waits for its answer (notifications arriving meanwhile are skipped).
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let text = serde_json::to_string(&Request::new(id, method, params))
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        writeln!(self.writer, "{text}")?;
        self.writer.flush()?;
        loop {
            let line = self.read_line()?;
            let value: Value =
                serde_json::from_str(&line).map_err(|e| ClientError::Protocol(e.to_string()))?;
            if value.get("id").is_none() {
                continue; // a notification
            }
            let response: Response =
                serde_json::from_value(value).map_err(|e| ClientError::Protocol(e.to_string()))?;
            if response.id.as_u64() != Some(id) {
                return Err(ClientError::Protocol(format!(
                    "answer to call {} while waiting for {id}",
                    response.id
                )));
            }
            return match (response.result, response.error) {
                (_, Some(error)) => Err(ClientError::Rpc(error)),
                (Some(result), None) => Ok(result),
                (None, None) => Err(ClientError::Protocol("no result or error".into())),
            };
        }
    }

    /// `kiln.hello`: the API version, the daemon's version and its methods.
    pub fn hello(&mut self) -> Result<Hello, ClientError> {
        let value = self.call("kiln.hello", Value::Null)?;
        serde_json::from_value(value).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    /// Subscribes to events; then [`Client::next_event`] returns them as they happen.
    pub fn subscribe(&mut self) -> Result<(), ClientError> {
        self.call("events.subscribe", Value::Null).map(|_| ())
    }

    /// Waits up to `timeout` for the next event.
    pub fn next_event(&mut self, timeout: Duration) -> Result<Notification, ClientError> {
        self.reader.get_ref().set_read_timeout(Some(timeout))?;
        let line = self.read_line()?;
        serde_json::from_str(&line).map_err(|e| ClientError::Protocol(e.to_string()))
    }

    fn read_line(&mut self) -> Result<String, ClientError> {
        let mut line = String::new();
        let read = (&mut self.reader)
            .take(MAX_LINE as u64 + 1)
            .read_line(&mut line)?;
        if read == 0 {
            return Err(ClientError::Protocol(
                "the daemon closed the connection".into(),
            ));
        }
        if line.len() > MAX_LINE {
            return Err(ClientError::Protocol("answer too long".into()));
        }
        Ok(line)
    }
}
