//! End-to-end helper sessions over real Windows named pipes (unelevated; the hosts file is a
//! temporary file). Each test attacks one guarantee from ADR 0009.
#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::Duration;

use kiln_core::{HostName, Tld};
use kiln_helper::client::HelperClient;
use kiln_helper::proto::{
    Channel, ErrorCode, HostEntry, ProtoError, Request, Response, SessionKey, new_pipe_name,
    new_session_key, to_hex,
};
use kiln_helper::server::{ServeError, ServerConfig, Summary, serve};
use kiln_platform::DnsEntry;

const ORIGINAL: &[u8] = b"# Copyright (c) Microsoft Corp.\r\n127.0.0.1 localhost\r\n";
const TIMEOUT: Duration = Duration::from_secs(5);

struct Session {
    pipe: String,
    key: SessionKey,
    server: JoinHandle<Result<Summary, ServeError>>,
}

fn start(hosts_path: &Path, client_pid: u32) -> Session {
    let config = ServerConfig {
        pipe_name: new_pipe_name(),
        client_pid,
        key: new_session_key(),
        hosts_path: hosts_path.to_owned(),
    };
    let (pipe, key) = (config.pipe_name.clone(), config.key);
    let server = std::thread::spawn(move || serve(&config));
    Session { pipe, key, server }
}

fn hosts_file(dir: &Path) -> PathBuf {
    let path = dir.join("hosts");
    fs::write(&path, ORIGINAL).unwrap();
    path
}

fn entries(names: &[&str]) -> Vec<DnsEntry> {
    names
        .iter()
        .map(|n| DnsEntry {
            name: HostName::new(n, &Tld::default()).unwrap(),
            ip: "127.0.0.1".parse().unwrap(),
        })
        .collect()
}

#[test]
fn a_full_session_writes_and_removes_the_block() {
    let dir = tempfile::tempdir().unwrap();
    let hosts = hosts_file(dir.path());
    let s = start(&hosts, std::process::id());

    let mut client = HelperClient::connect(&s.pipe, s.key, TIMEOUT).unwrap();
    assert_eq!(
        client.ping().unwrap(),
        std::process::id(),
        "in-process server"
    );

    let written = client
        .hosts_set_block("test", &entries(&["blog.test", "shop.test"]), false)
        .unwrap();
    assert!(!written.created && !written.unchanged);
    let text = fs::read_to_string(&hosts).unwrap();
    assert!(text.contains("127.0.0.1 blog.test\r\n") && text.contains("127.0.0.1 shop.test\r\n"));

    client.hosts_set_block("test", &[], false).unwrap();
    assert_eq!(
        fs::read(&hosts).unwrap(),
        ORIGINAL,
        "byte-identical after removal"
    );

    client.shutdown().unwrap();
    let summary = s.server.join().unwrap().unwrap();
    assert_eq!(
        summary,
        Summary {
            requests: 5,
            hosts_writes: 2
        }
    );
}

/// The helper re-validates everything itself. These requests are correctly signed but go
/// straight onto the wire, bypassing the typed client, which could not even express them.
#[test]
fn invalid_requests_are_refused_and_change_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let hosts = hosts_file(dir.path());
    let s = start(&hosts, std::process::id());
    let pipe = kiln_sys::win::connect_pipe(&s.pipe, TIMEOUT).unwrap();
    let mut raw = Channel::new(pipe, s.key);

    let entry = |name: &str, ip: &str| HostEntry {
        name: name.into(),
        ip: ip.parse().unwrap(),
    };
    for (tld, e) in [
        ("com", entry("bank.com", "127.0.0.1")),     // public TLD
        ("test", entry("bank.com", "127.0.0.1")),    // name outside the TLD
        ("test", entry("nas.test", "192.168.1.20")), // not loopback
        ("test", entry("a.test\r\n10.0.0.1 evil", "::1")), // line injection
    ] {
        let response = raw
            .call(&Request::HostsSetBlock {
                tld: tld.into(),
                entries: vec![e.clone()],
                remove_file_if_empty: false,
            })
            .unwrap();
        assert!(
            matches!(
                response,
                Response::Error {
                    code: ErrorCode::InvalidRequest,
                    ..
                }
            ),
            "{tld} {e:?} -> {response:?}"
        );
    }
    assert_eq!(fs::read(&hosts).unwrap(), ORIGINAL);
    assert_eq!(raw.call(&Request::Shutdown).unwrap(), Response::Bye);
    assert_eq!(s.server.join().unwrap().unwrap().hosts_writes, 0);
}

#[test]
fn a_client_without_the_session_key_is_cut_off() {
    let dir = tempfile::tempdir().unwrap();
    let hosts = hosts_file(dir.path());
    let s = start(&hosts, std::process::id());

    let mut wrong_key = s.key;
    wrong_key[0] ^= 1;
    assert!(HelperClient::connect(&s.pipe, wrong_key, TIMEOUT).is_err());
    assert!(matches!(
        s.server.join().unwrap(),
        Err(ServeError::Protocol(ProtoError::BadMac))
    ));
    assert_eq!(fs::read(&hosts).unwrap(), ORIGINAL);
}

/// A process the helper was not started for (even the same user, even with the key) is
/// refused before any request is read.
#[test]
fn only_the_launching_process_may_connect() {
    let dir = tempfile::tempdir().unwrap();
    let hosts = hosts_file(dir.path());
    let mut other: Child = Command::new("cmd")
        .args(["/c", "ping", "-n", "5", "127.0.0.1"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let s = start(&hosts, other.id());

    assert!(HelperClient::connect(&s.pipe, s.key, TIMEOUT).is_err());
    match s.server.join().unwrap() {
        Err(ServeError::WrongClient { expected, actual }) => {
            assert_eq!(expected, other.id());
            assert_eq!(actual, std::process::id());
        }
        other => panic!("expected WrongClient, got {other:?}"),
    }
    let _ = other.kill();
    let _ = other.wait();
    assert_eq!(fs::read(&hosts).unwrap(), ORIGINAL);
}

/// The real binary: argument handling and a ping/shutdown session. It is not elevated here,
/// and no hosts write is sent (the binary always targets the real system hosts file).
#[test]
fn the_helper_binary_serves_one_session_and_exits() {
    let pipe = new_pipe_name();
    let key = new_session_key();
    let mut helper = Command::new(env!("CARGO_BIN_EXE_kiln-helper"))
        .args([
            "--pipe",
            &pipe,
            "--client-pid",
            &std::process::id().to_string(),
            "--key",
            &to_hex(&key),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut client = HelperClient::connect(&pipe, key, TIMEOUT).unwrap();
    assert_eq!(client.ping().unwrap(), helper.id());
    client.shutdown().unwrap();
    assert_eq!(helper.wait().unwrap().code(), Some(0));

    let bad = Command::new(env!("CARGO_BIN_EXE_kiln-helper"))
        .args(["--pipe", "kiln-helper-x", "--hosts", r"C:\Windows\win.ini"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(64), "unknown arguments are refused");
}
