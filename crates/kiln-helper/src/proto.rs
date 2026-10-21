//! Wire protocol between `kiln` and `kiln-helper` (ADR 0009 §7–8).
//!
//! A frame is a 4-byte little-endian length followed by a JSON [`Envelope`]. The envelope
//! carries the message body as a JSON *string*, so the MAC covers the exact bytes that will
//! be parsed. Bodies are only parsed after their MAC has been verified.

use std::io::{self, Read, Write};
use std::net::IpAddr;

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;

/// Protocol label mixed into every MAC.
pub const PROTOCOL: &str = "kiln-helper/2";
/// Version reported by `Ping`; client and helper must match exactly.
/// v2 (P1-03): `HostsSetBlock.remove_file_if_empty`, `HostsWritten.deleted`.
pub const PROTOCOL_VERSION: u32 = 2;
/// Largest accepted frame, in bytes.
pub const MAX_FRAME: usize = 1 << 20;
/// Largest accepted number of hosts entries in one request.
pub const MAX_ENTRIES: usize = 10_000;
/// Session key length, in bytes.
pub const KEY_LEN: usize = 32;

pub type SessionKey = [u8; KEY_LEN];

/// The fixed list of operations the helper performs. There is deliberately no generic
/// "run this" variant; new operations need an ADR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Ping,
    /// Replace Kiln's block in the system hosts file; an empty list removes it.
    HostsSetBlock {
        tld: String,
        entries: Vec<HostEntry>,
        /// Delete the file if it would end up with zero bytes (undoing a creation). A file
        /// with any other content is never deleted.
        #[serde(default)]
        remove_file_if_empty: bool,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostEntry {
    pub name: String,
    pub ip: IpAddr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Pong {
        protocol: u32,
        helper_pid: u32,
    },
    HostsWritten {
        /// The hosts file did not exist and was created (revert should delete it).
        created: bool,
        /// The file was deleted (`remove_file_if_empty` and nothing else left).
        deleted: bool,
        /// Nothing needed to change.
        unchanged: bool,
        bytes_before: u64,
        bytes_after: u64,
    },
    Bye,
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request failed validation (TLD, names, addresses, limits).
    InvalidRequest,
    /// The hosts file is marked read-only; Kiln does not override that by default.
    HostsReadOnly,
    /// The hosts file stayed locked by another process.
    HostsBusy,
    /// The hosts file has ambiguous Kiln markers; it needs a manual fix.
    HostsMalformed,
    /// Any other I/O failure.
    Io,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("channel I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("peer closed the channel")]
    Closed,
    #[error("frame of {0} bytes exceeds the {MAX_FRAME}-byte limit")]
    FrameTooLarge(usize),
    #[error("message authentication failed")]
    BadMac,
    #[error("sequence number {got} is not after {last} (replayed or reordered message)")]
    Replay { got: u64, last: u64 },
    #[error("response sequence {got} does not answer request {expected}")]
    SeqMismatch { got: u64, expected: u64 },
    #[error("malformed message: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    seq: u64,
    body: String,
    mac: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Request,
    Response,
}

impl Direction {
    fn label(self) -> &'static [u8] {
        match self {
            Direction::Request => b"req",
            Direction::Response => b"resp",
        }
    }
}

fn mac(key: &SessionKey, direction: Direction, seq: u64, body: &[u8]) -> Hmac<Sha256> {
    let mut m =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(PROTOCOL.as_bytes());
    m.update(&[0]);
    m.update(direction.label());
    m.update(&[0]);
    m.update(&seq.to_le_bytes());
    m.update(body);
    m
}

/// Lowercase hex encoding.
#[must_use]
pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Decodes lowercase or uppercase hex; `None` on odd length or a non-hex digit.
#[must_use]
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// A fresh random session key.
///
/// # Panics
/// If the OS random source is unavailable (no safe fallback exists).
#[must_use]
pub fn new_session_key() -> SessionKey {
    let mut key = [0u8; KEY_LEN];
    getrandom::fill(&mut key).expect("OS random source");
    key
}

/// A fresh, unguessable pipe name (`kiln-helper-<128-bit hex>`).
///
/// # Panics
/// If the OS random source is unavailable.
#[must_use]
pub fn new_pipe_name() -> String {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).expect("OS random source");
    format!("kiln-helper-{}", to_hex(&id))
}

fn write_frame(stream: &mut impl Write, bytes: &[u8]) -> Result<(), ProtoError> {
    if bytes.len() > MAX_FRAME {
        return Err(ProtoError::FrameTooLarge(bytes.len()));
    }
    let len = u32::try_from(bytes.len()).map_err(|_| ProtoError::FrameTooLarge(bytes.len()))?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()?;
    Ok(())
}

fn read_frame(stream: &mut impl Read) -> Result<Vec<u8>, ProtoError> {
    let mut len = [0u8; 4];
    match stream.read_exact(&mut len) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe
            ) =>
        {
            return Err(ProtoError::Closed);
        }
        Err(e) => return Err(e.into()),
    }
    let len = usize::try_from(u32::from_le_bytes(len)).unwrap_or(usize::MAX);
    if len > MAX_FRAME {
        return Err(ProtoError::FrameTooLarge(len));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

/// One authenticated, ordered conversation over a byte stream.
#[derive(Debug)]
pub struct Channel<S> {
    stream: S,
    key: SessionKey,
    /// Client: last request seq sent. Server: last request seq accepted.
    seq: u64,
}

impl<S: Read + Write> Channel<S> {
    pub fn new(stream: S, key: SessionKey) -> Self {
        Self {
            stream,
            key,
            seq: 0,
        }
    }

    fn send(
        &mut self,
        direction: Direction,
        seq: u64,
        body: &impl Serialize,
    ) -> Result<(), ProtoError> {
        let body = serde_json::to_string(body)?;
        let tag = mac(&self.key, direction, seq, body.as_bytes())
            .finalize()
            .into_bytes();
        let envelope = Envelope {
            seq,
            body,
            mac: to_hex(&tag),
        };
        write_frame(&mut self.stream, &serde_json::to_vec(&envelope)?)
    }

    fn receive<T: DeserializeOwned>(
        &mut self,
        direction: Direction,
    ) -> Result<(u64, T), ProtoError> {
        let frame = read_frame(&mut self.stream)?;
        let envelope: Envelope = serde_json::from_slice(&frame)?;
        let tag = from_hex(&envelope.mac).ok_or(ProtoError::BadMac)?;
        mac(&self.key, direction, envelope.seq, envelope.body.as_bytes())
            .verify_slice(&tag)
            .map_err(|_| ProtoError::BadMac)?;
        // Authenticated; only now is the body parsed.
        Ok((envelope.seq, serde_json::from_str(&envelope.body)?))
    }

    /// Client side: sends `request` and returns the helper's authenticated answer to it.
    pub fn call(&mut self, request: &Request) -> Result<Response, ProtoError> {
        self.seq += 1;
        let seq = self.seq;
        self.send(Direction::Request, seq, request)?;
        let (got, response) = self.receive::<Response>(Direction::Response)?;
        if got != seq {
            return Err(ProtoError::SeqMismatch { got, expected: seq });
        }
        Ok(response)
    }

    /// Server side: the next authenticated request, which must be newer than the last one.
    pub fn next_request(&mut self) -> Result<(u64, Request), ProtoError> {
        let (seq, request) = self.receive::<Request>(Direction::Request)?;
        if seq <= self.seq {
            return Err(ProtoError::Replay {
                got: seq,
                last: self.seq,
            });
        }
        self.seq = seq;
        Ok((seq, request))
    }

    /// Server side: answers request `seq`.
    pub fn respond(&mut self, seq: u64, response: &Response) -> Result<(), ProtoError> {
        self.send(Direction::Response, seq, response)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// In-memory stream: reads from `input`, collects writes in `output`.
    #[derive(Debug, Default)]
    pub(crate) struct Duplex {
        pub input: Cursor<Vec<u8>>,
        pub output: Vec<u8>,
    }
    impl Read for Duplex {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }
    impl Write for Duplex {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    const KEY: SessionKey = [7u8; KEY_LEN];

    fn request_frame(key: SessionKey, seq: u64, request: &Request) -> Vec<u8> {
        let mut client = Channel::new(Duplex::default(), key);
        client.send(Direction::Request, seq, request).unwrap();
        client.stream.output
    }

    fn server_over(bytes: Vec<u8>) -> Channel<Duplex> {
        Channel::new(
            Duplex {
                input: Cursor::new(bytes),
                output: Vec::new(),
            },
            KEY,
        )
    }

    #[test]
    fn authenticated_requests_round_trip() {
        let req = Request::HostsSetBlock {
            tld: "test".into(),
            entries: vec![HostEntry {
                name: "blog.test".into(),
                ip: "127.0.0.1".parse().unwrap(),
            }],
            remove_file_if_empty: true,
        };
        let mut server = server_over(request_frame(KEY, 1, &req));
        assert_eq!(server.next_request().unwrap(), (1, req));
    }

    #[test]
    fn a_wrong_key_is_rejected() {
        let mut server = server_over(request_frame([8u8; KEY_LEN], 1, &Request::Ping));
        assert!(matches!(server.next_request(), Err(ProtoError::BadMac)));
    }

    #[test]
    fn a_tampered_body_is_rejected_before_parsing() {
        let mut frame = request_frame(KEY, 1, &Request::Ping);
        let text = String::from_utf8(frame.split_off(4)).unwrap();
        let tampered = text.replace(r#"{\"op\":\"ping\"}"#, r#"{\"op\":\"shutdown\"}"#);
        assert_ne!(tampered, text);
        let mut forged = u32::try_from(tampered.len())
            .unwrap()
            .to_le_bytes()
            .to_vec();
        forged.extend_from_slice(tampered.as_bytes());
        assert!(matches!(
            server_over(forged).next_request(),
            Err(ProtoError::BadMac)
        ));
    }

    #[test]
    fn replayed_or_reordered_requests_are_rejected() {
        let mut bytes = request_frame(KEY, 5, &Request::Ping);
        bytes.extend(request_frame(KEY, 5, &Request::Ping));
        let mut server = server_over(bytes);
        assert_eq!(server.next_request().unwrap().0, 5);
        assert!(matches!(
            server.next_request(),
            Err(ProtoError::Replay { got: 5, last: 5 })
        ));
    }

    #[test]
    fn a_response_cannot_be_reflected_as_a_request() {
        let mut helper = Channel::new(Duplex::default(), KEY);
        helper.respond(1, &Response::Bye).unwrap();
        let mut server = server_over(helper.stream.output);
        assert!(matches!(server.next_request(), Err(ProtoError::BadMac)));
    }

    #[test]
    fn oversized_frames_and_unknown_operations_are_rejected() {
        let mut huge = u32::try_from(MAX_FRAME + 1).unwrap().to_le_bytes().to_vec();
        huge.extend([0u8; 16]);
        assert!(matches!(
            server_over(huge).next_request(),
            Err(ProtoError::FrameTooLarge(_))
        ));

        // Correctly signed, but not an operation the helper knows.
        let body = r#"{"op":"run_command","cmd":"calc.exe"}"#;
        let tag = mac(&KEY, Direction::Request, 1, body.as_bytes())
            .finalize()
            .into_bytes();
        let env = serde_json::to_vec(&Envelope {
            seq: 1,
            body: body.into(),
            mac: to_hex(&tag),
        })
        .unwrap();
        let mut frame = u32::try_from(env.len()).unwrap().to_le_bytes().to_vec();
        frame.extend(env);
        assert!(matches!(
            server_over(frame).next_request(),
            Err(ProtoError::Json(_))
        ));
    }

    #[test]
    fn eof_before_a_frame_is_a_clean_close() {
        assert!(matches!(
            server_over(Vec::new()).next_request(),
            Err(ProtoError::Closed)
        ));
    }

    #[test]
    fn hex_round_trips_and_rejects_garbage() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
        assert_ne!(new_session_key(), new_session_key());
        assert!(new_pipe_name().starts_with("kiln-helper-"));
        assert_eq!(new_pipe_name().len(), "kiln-helper-".len() + 32);
    }
}
