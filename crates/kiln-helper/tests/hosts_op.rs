//! Full stack on a real pipe: SQLite journal → engine → `HostsBlockOp` → `HelperSession`
//! → helper server → hosts file. Undo runs in a fresh engine and helper session, the way a
//! later `kiln undo` process would. The helper is not elevated here and the hosts file is a
//! temporary file.
#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread::JoinHandle;
use std::time::Duration;

use kiln_core::{HostName, Tld};
use kiln_helper::client::HelperClient;
use kiln_helper::proto::{new_pipe_name, new_session_key};
use kiln_helper::server::{ServeError, ServerConfig, Summary, serve};
use kiln_helper::session::HelperSession;
use kiln_ops::{Engine, OpState, Operation, Registry};
use kiln_platform::DnsEntry;
use kiln_platform::hosts_op::{HostsBlockOp, PrivilegedHosts, register_hosts_block};
use kiln_state::StateStore;

const ORIGINAL: &[u8] = b"# Copyright (c) Microsoft Corp.\r\n127.0.0.1 localhost\r\n";

/// A helper server on its own thread plus a connected session to it.
fn helper(hosts: &Path) -> (Rc<HelperSession>, JoinHandle<Result<Summary, ServeError>>) {
    let config = ServerConfig {
        pipe_name: new_pipe_name(),
        client_pid: std::process::id(),
        key: new_session_key(),
        hosts_path: hosts.to_owned(),
    };
    let (pipe, key) = (config.pipe_name.clone(), config.key);
    let server = std::thread::spawn(move || serve(&config));
    let client = HelperClient::connect(&pipe, key, Duration::from_secs(5)).unwrap();
    (Rc::new(HelperSession::connected(client)), server)
}

fn engine(db: &Path, hosts: &Path, session: &Rc<HelperSession>) -> Engine<StateStore> {
    let mut registry = Registry::default();
    let writer: Rc<dyn PrivilegedHosts> = session.clone();
    register_hosts_block(&mut registry, hosts.to_owned(), writer);
    Engine::new(StateStore::open(db).unwrap(), registry)
}

fn op(hosts: &Path, session: &Rc<HelperSession>, names: &[&str]) -> Box<dyn Operation> {
    let tld = Tld::default();
    let entries = names
        .iter()
        .map(|n| DnsEntry {
            name: HostName::new(n, &tld).unwrap(),
            ip: "127.0.0.1".parse().unwrap(),
        })
        .collect();
    let writer: Rc<dyn PrivilegedHosts> = session.clone();
    Box::new(HostsBlockOp::new(hosts.to_owned(), writer, tld, entries))
}

fn paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("state.db"), dir.join("hosts"))
}

#[test]
fn apply_through_the_helper_and_undo_from_a_later_process() {
    let dir = tempfile::tempdir().unwrap();
    let (db, hosts) = paths(dir.path());
    fs::write(&hosts, ORIGINAL).unwrap();

    // Process 1: apply.
    let id = {
        let (session, server) = helper(&hosts);
        let mut engine = engine(&db, &hosts, &session);
        let report = engine
            .apply(&[op(&hosts, &session, &["blog.test", "api.blog.test"])])
            .unwrap();
        assert_eq!(session.writes(), 1);
        session.finish().unwrap();
        assert_eq!(server.join().unwrap().unwrap().hosts_writes, 1);
        report.entry_ids[0]
    };
    let text = fs::read_to_string(&hosts).unwrap();
    assert!(text.contains("127.0.0.1 api.blog.test\r\n127.0.0.1 blog.test\r\n"));

    // Process 2: undo using only the journal.
    let (session, server) = helper(&hosts);
    let mut engine = engine(&db, &hosts, &session);
    engine.revert(id, false).unwrap();
    session.finish().unwrap();
    assert_eq!(server.join().unwrap().unwrap().hosts_writes, 1);
    assert_eq!(fs::read(&hosts).unwrap(), ORIGINAL, "byte-identical");

    let history: Vec<_> = engine
        .journal()
        .history(id)
        .unwrap()
        .into_iter()
        .map(|(s, _)| s)
        .collect();
    assert_eq!(
        history,
        vec![OpState::Prepared, OpState::Applied, OpState::Reverted]
    );
}

#[test]
fn a_missing_hosts_file_is_created_and_deleted_again() {
    let dir = tempfile::tempdir().unwrap();
    let (db, hosts) = paths(dir.path());

    let (session, server) = helper(&hosts);
    let mut engine = engine(&db, &hosts, &session);
    engine.apply(&[op(&hosts, &session, &["a.test"])]).unwrap();
    assert!(hosts.exists());
    engine.revert_all(false).unwrap();
    assert!(!hosts.exists(), "undo restores 'no hosts file'");
    session.finish().unwrap();
    assert_eq!(server.join().unwrap().unwrap().hosts_writes, 2);
}

#[test]
fn work_that_needs_no_change_never_touches_the_helper() {
    let dir = tempfile::tempdir().unwrap();
    let (db, hosts) = paths(dir.path());
    fs::write(&hosts, ORIGINAL).unwrap();

    let (session, server) = helper(&hosts);
    let mut engine = engine(&db, &hosts, &session);
    engine.apply(&[op(&hosts, &session, &[])]).unwrap(); // nothing to remove
    engine.apply(&[op(&hosts, &session, &["a.test"])]).unwrap();
    engine.apply(&[op(&hosts, &session, &["a.test"])]).unwrap(); // already there
    assert_eq!(session.writes(), 1);
    session.finish().unwrap();
    assert_eq!(
        server.join().unwrap().unwrap(),
        Summary {
            requests: 3, // connect ping, one write, shutdown
            hosts_writes: 1
        }
    );
}
