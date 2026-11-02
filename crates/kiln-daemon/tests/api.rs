//! The API over a real per-user socket: calls, errors, oversized lines and events.

use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use std::time::Duration;

use kiln_daemon::proto::{METHOD_NOT_FOUND, PARSE_ERROR, RpcError};
use kiln_daemon::server::{self, Events, Handler};
use kiln_daemon::transport::{self, Listener};
use kiln_daemon::{Client, ClientError};
use serde_json::{Value, json};

struct Echo;

impl Handler for Echo {
    fn call(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "kiln.hello" => Ok(json!({"api": 1, "version": "test", "methods": ["test.echo"]})),
            "test.echo" => Ok(params.clone()),
            "test.fail" => Err(RpcError::kiln("it failed")),
            other => Err(RpcError::method_not_found(other)),
        }
    }
}

/// A server on a socket in a fresh folder; the folder lives as long as the returned guard.
fn start() -> (tempfile::TempDir, std::path::PathBuf, Arc<Events>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kilnd.sock");
    let listener = Listener::bind(&path).unwrap();
    let events = Arc::new(Events::default());
    let handler: Arc<dyn Handler> = Arc::new(Echo);
    let served = Arc::clone(&events);
    std::thread::spawn(move || server::serve(&listener, &handler, &served));
    (dir, path, events)
}

#[test]
fn calls_errors_and_events_round_trip() {
    let (_dir, path, events) = start();
    let mut client = Client::connect(&path).unwrap();
    let hello = client.hello().unwrap();
    assert_eq!(hello.api, 1);
    assert_eq!(
        client.call("test.echo", json!({"a": [1, 2]})).unwrap(),
        json!({"a": [1, 2]})
    );
    match client.call("test.fail", Value::Null) {
        Err(ClientError::Rpc(error)) => assert_eq!(error.message, "it failed"),
        other => panic!("{other:?}"),
    }
    match client.call("no.such", Value::Null) {
        Err(ClientError::Rpc(error)) => assert_eq!(error.code, METHOD_NOT_FOUND),
        other => panic!("{other:?}"),
    }

    let mut watcher = Client::connect(&path).unwrap();
    watcher.subscribe().unwrap();
    events.publish("web.repaired", json!({"ok": true}));
    let event = watcher.next_event(Duration::from_secs(5)).unwrap();
    assert_eq!(event.method, "web.repaired");
    assert_eq!(event.params, json!({"ok": true}));
    // The subscribed connection still answers calls, skipping events in between.
    events.publish("web.repaired", json!({"ok": false}));
    assert_eq!(watcher.call("test.echo", json!(5)).unwrap(), json!(5));
}

#[test]
fn bad_input_gets_an_error_not_a_hang() {
    let (_dir, path, _events) = start();
    let stream = transport::connect(&path).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let mut answer = String::new();

    writeln!(writer, "not json").unwrap();
    reader.read_line(&mut answer).unwrap();
    let value: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(value["error"]["code"], json!(PARSE_ERROR));

    // A notification (no id) gets no answer; the next call's answer comes first.
    writeln!(
        writer,
        r#"{{"jsonrpc":"2.0","method":"test.echo","params":1}}"#
    )
    .unwrap();
    writeln!(
        writer,
        r#"{{"jsonrpc":"2.0","id":9,"method":"test.echo","params":2}}"#
    )
    .unwrap();
    answer.clear();
    reader.read_line(&mut answer).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&answer).unwrap()["id"],
        json!(9)
    );

    // A line over the limit is refused and the connection closed.
    let long = "x".repeat(kiln_daemon::proto::MAX_LINE + 10);
    // The daemon may answer and close before the whole line is written: a broken pipe here is
    // the refusal arriving early, and the answer is still there to read.
    let _ = writeln!(writer, "{long}");
    answer.clear();
    reader.read_line(&mut answer).unwrap();
    assert!(answer.contains("line too long"), "{answer}");
}

#[test]
fn no_socket_means_not_running() {
    let dir = tempfile::tempdir().unwrap();
    match Client::connect(&dir.path().join("kilnd.sock")) {
        Err(ClientError::NotRunning) => {}
        other => panic!("{other:?}"),
    }
}
