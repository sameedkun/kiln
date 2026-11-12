//! The wire format ([ADR 0026] §3): JSON-RPC 2.0, one JSON message per line.
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The API version `kiln.hello` reports: 2 adds reads through `kiln --json` and jobs
/// ([ADR 0053]); version 1's methods stay as they were.
///
/// [ADR 0053]: ../../../docs/adr/0053-api-v2-and-json-output.md
pub const API_VERSION: u32 = 2;

/// Longest line either side accepts, so a stuck or hostile peer can't exhaust memory.
pub const MAX_LINE: usize = 1024 * 1024;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// A Kiln operation failed; `message` says why, in the words the CLI would use.
pub const KILN_ERROR: i64 = -32000;

/// A call. Without an `id` it is a notification and gets no response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

impl Request {
    #[must_use]
    pub fn new(id: u64, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id.into()),
            method: method.into(),
            params,
        }
    }
}

/// The answer to a call with an `id`: either `result` or `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    #[must_use]
    pub fn new(id: Value, outcome: Result<Value, RpcError>) -> Self {
        let (result, error) = match outcome {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            jsonrpc: "2.0".into(),
            id,
            result,
            error,
        }
    }
}

/// Something that happened, sent to clients that called `events.subscribe`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Notification {
    #[must_use]
    pub fn new(method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message} (code {code})")]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// A Kiln operation failed.
    #[must_use]
    pub fn kiln(message: impl Into<String>) -> Self {
        Self::new(KILN_ERROR, message)
    }

    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("no method {method:?}"))
    }
}

/// `kiln.hello`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub api: u32,
    /// The daemon's version (Kiln's).
    pub version: String,
    /// The methods it answers.
    pub methods: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_match_json_rpc_2() {
        let request = Request::new(7, "web.status", Value::Null);
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({"jsonrpc": "2.0", "id": 7, "method": "web.status"})
        );
        let ok = Response::new(json!(7), Ok(json!({"a": 1})));
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            json!({"jsonrpc": "2.0", "id": 7, "result": {"a": 1}})
        );
        let err = Response::new(json!(7), Err(RpcError::method_not_found("x.y")));
        assert_eq!(
            serde_json::to_value(&err).unwrap(),
            json!({"jsonrpc": "2.0", "id": 7, "error": {"code": -32601, "message": "no method \"x.y\""}})
        );
        let note: Request =
            serde_json::from_value(json!({"jsonrpc": "2.0", "method": "a.b"})).unwrap();
        assert_eq!(note.id, None, "no id: a notification");
    }
}
