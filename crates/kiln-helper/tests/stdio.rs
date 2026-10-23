//! Helper sessions over stdin/stdout, as `pkexec kiln-helper --stdio` runs them on Linux
//! (ADR 0020 §4), on real OS pipes with a temporary hosts file. Runs unprivileged on every
//! platform: only the launch through `pkexec` needs root, and that is checked by hand.

use std::fs;
use std::io::{PipeReader, PipeWriter, Write, pipe};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use kiln_core::{HostName, Tld};
use kiln_helper::client::{ClientError, HelperClient};
use kiln_helper::proto::{Joined, ProtoError, new_session_key, write_key_line};
use kiln_helper::server::{Summary, serve_stdio};
use kiln_platform::DnsEntry;

const ORIGINAL: &[u8] = b"127.0.0.1 localhost\n::1 localhost\n";

type Server = JoinHandle<Result<Summary, ProtoError>>;

/// A helper serving `hosts` on fresh pipes; returns the client's ends.
fn helper(hosts: PathBuf) -> (PipeReader, PipeWriter, Server) {
    let (from_kiln, to_helper) = pipe().unwrap();
    let (from_helper, to_kiln) = pipe().unwrap();
    let server = std::thread::spawn(move || serve_stdio(from_kiln, to_kiln, &hosts, None));
    (from_helper, to_helper, server)
}

fn hosts_file(dir: &Path) -> PathBuf {
    let path = dir.join("hosts");
    fs::write(&path, ORIGINAL).unwrap();
    path
}

fn entry(name: &str) -> DnsEntry {
    DnsEntry {
        name: HostName::new(name, &Tld::default()).unwrap(),
        ip: "127.0.0.1".parse().unwrap(),
    }
}

#[test]
fn a_stdio_session_writes_the_block_and_undoes_it() {
    let dir = tempfile::tempdir().unwrap();
    let hosts = hosts_file(dir.path());
    let (reader, mut writer, server) = helper(hosts.clone());
    let key = new_session_key();
    write_key_line(&mut writer, &key).unwrap();
    let mut client = HelperClient::over(Joined::new(reader, writer), key).unwrap();

    let written = client
        .hosts_set_block("test", &[entry("blog.test")], false)
        .unwrap();
    assert!(!written.unchanged);
    assert!(
        fs::read_to_string(&hosts)
            .unwrap()
            .contains("127.0.0.1 blog.test")
    );
    client.hosts_set_block("test", &[], false).unwrap();
    assert_eq!(fs::read(&hosts).unwrap(), ORIGINAL, "byte-identical");

    client.shutdown().unwrap();
    let summary = server.join().unwrap().unwrap();
    assert_eq!(summary.hosts_writes, 2);
}

#[test]
fn a_wrong_key_gets_nothing_done() {
    let dir = tempfile::tempdir().unwrap();
    let hosts = hosts_file(dir.path());
    let (reader, mut writer, server) = helper(hosts.clone());
    write_key_line(&mut writer, &new_session_key()).unwrap();
    // Signed with another key: the helper rejects the first message and ends the session.
    let err = HelperClient::over(Joined::new(reader, writer), new_session_key()).unwrap_err();
    assert!(matches!(err, ClientError::Protocol(_)), "{err}");
    assert!(matches!(server.join().unwrap(), Err(ProtoError::BadMac)));
    assert_eq!(fs::read(&hosts).unwrap(), ORIGINAL);
}

#[test]
fn a_malformed_key_line_ends_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let (_reader, mut writer, server) = helper(hosts_file(dir.path()));
    writer.write_all(&[b'z'; 65]).unwrap();
    assert!(matches!(
        server.join().unwrap(),
        Err(ProtoError::BadKeyLine)
    ));

    // End of input before a key: a clean, empty session.
    let (_reader, writer, server) = helper(hosts_file(dir.path()));
    drop(writer);
    assert!(matches!(server.join().unwrap(), Err(ProtoError::Closed)));
}

#[test]
fn requests_are_validated_inside_the_helper() {
    let dir = tempfile::tempdir().unwrap();
    let hosts = hosts_file(dir.path());
    let (reader, mut writer, server) = helper(hosts.clone());
    let key = new_session_key();
    write_key_line(&mut writer, &key).unwrap();
    let mut client = HelperClient::over(Joined::new(reader, writer), key).unwrap();
    // A public TLD is refused by the helper itself, whatever the client sends.
    let err = client
        .hosts_set_block("com", &[entry("blog.test")], false)
        .unwrap_err();
    assert!(matches!(err, ClientError::Refused { .. }), "{err}");
    assert_eq!(fs::read(&hosts).unwrap(), ORIGINAL);
    client.shutdown().unwrap();
    server.join().unwrap().unwrap();
}
