//! The hosts-block DNS operation (P1-03; ADR 0004, 0006, 0009).
//!
//! `HostsBlockOp` makes Kiln's block in the hosts file contain exactly a given set of names.
//! Reading the file needs no privileges; the write goes through a [`PrivilegedHosts`]
//! implementation (on Windows, the elevated `kiln-helper`). The operation only asks for a
//! privileged write when something actually has to change, so re-applying, verifying, and
//! recovering after a crash never prompt for elevation.

use std::fmt;
use std::fs;
use std::io;
use std::net::IpAddr;
use std::path::PathBuf;
use std::rc::Rc;

use kiln_core::{HostName, Tld};
use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

use crate::{DnsEntry, hosts};

/// Journal kind of [`HostsBlockOp`].
pub const HOSTS_BLOCK_KIND: &str = "dns.hosts.block";

/// Performs the privileged part: replacing Kiln's block (empty removes it), optionally
/// deleting a file that would be left with zero bytes. Implementations re-validate input.
pub trait PrivilegedHosts {
    fn set_block(
        &self,
        tld: &Tld,
        entries: &[DnsEntry],
        remove_file_if_empty: bool,
    ) -> Result<(), OpError>;
}

/// `(ip, name)` text pairs, sorted: the comparable form of a block.
type Lines = Vec<(String, String)>;

fn lines(entries: &[DnsEntry]) -> Lines {
    let mut out: Lines = entries
        .iter()
        .map(|e| (e.ip.to_string(), e.name.to_string()))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn failed(message: impl Into<String>) -> OpError {
    OpError::Failed(message.into())
}

fn parse_lines(value: &Value, what: &str) -> Result<Lines, OpError> {
    let pairs = value
        .as_array()
        .ok_or_else(|| failed(format!("{what} is not a list")))?;
    let mut out = Lines::with_capacity(pairs.len());
    for pair in pairs {
        match pair.as_array().map(Vec::as_slice) {
            Some([Value::String(ip), Value::String(name)]) => out.push((ip.clone(), name.clone())),
            _ => return Err(failed(format!("{what} holds a malformed entry: {pair}"))),
        }
    }
    out.sort();
    Ok(out)
}

fn to_entries(lines: &Lines, tld: &Tld) -> Result<Vec<DnsEntry>, OpError> {
    lines
        .iter()
        .map(|(ip, name)| {
            Ok(DnsEntry {
                name: HostName::new(name, tld).map_err(|e| failed(format!("{name}: {e}")))?,
                ip: ip
                    .parse::<IpAddr>()
                    .map_err(|e| failed(format!("{ip}: {e}")))?,
            })
        })
        .collect()
}

/// Makes Kiln's hosts block contain exactly `entries` (an empty list removes the block).
pub struct HostsBlockOp {
    hosts_path: PathBuf,
    tld: Tld,
    entries: Vec<DnsEntry>,
    writer: Rc<dyn PrivilegedHosts>,
}

impl fmt::Debug for HostsBlockOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostsBlockOp")
            .field("hosts_path", &self.hosts_path)
            .field("tld", &self.tld)
            .field("entries", &self.entries)
            .finish_non_exhaustive()
    }
}

impl HostsBlockOp {
    #[must_use]
    pub fn new(
        hosts_path: PathBuf,
        writer: Rc<dyn PrivilegedHosts>,
        tld: Tld,
        mut entries: Vec<DnsEntry>,
    ) -> Self {
        entries.sort();
        entries.dedup();
        Self {
            hosts_path,
            tld,
            entries,
            writer,
        }
    }

    /// Rebuilds the operation from its journaled parameters (for undo in a later process).
    pub fn from_params(
        hosts_path: PathBuf,
        writer: Rc<dyn PrivilegedHosts>,
        params: &Value,
    ) -> Result<Self, OpError> {
        let tld = params["tld"]
            .as_str()
            .ok_or_else(|| failed("params.tld missing"))
            .and_then(|t| Tld::recorded(t).map_err(|e| failed(e.to_string())))?;
        let entries = to_entries(&parse_lines(&params["entries"], "params.entries")?, &tld)?;
        Ok(Self::new(hosts_path, writer, tld, entries))
    }

    fn read(&self) -> Result<Option<Vec<u8>>, OpError> {
        match fs::read(&self.hosts_path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(failed(format!(
                "cannot read {}: {e}",
                self.hosts_path.display()
            ))),
        }
    }

    fn block_of(bytes: Option<&[u8]>) -> Result<Lines, OpError> {
        let mut out = match bytes {
            Some(b) => hosts::read_block(b).map_err(|e| failed(e.to_string()))?,
            None => Vec::new(),
        };
        out.sort();
        Ok(out)
    }

    fn desired(&self) -> Lines {
        lines(&self.entries)
    }

    fn render(lines: &Lines) -> Option<String> {
        (!lines.is_empty()).then(|| {
            lines
                .iter()
                .map(|(ip, name)| format!("{ip} {name}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}

impl Operation for HostsBlockOp {
    fn kind(&self) -> &'static str {
        HOSTS_BLOCK_KIND
    }

    fn params(&self) -> Value {
        let entries: Vec<[String; 2]> = self
            .desired()
            .into_iter()
            .map(|(ip, name)| [ip, name])
            .collect();
        json!({ "tld": self.tld.as_str(), "entries": entries })
    }

    fn scope(&self) -> Scope {
        Scope::Machine
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let current = Self::block_of(self.read()?.as_deref())?;
        let summary = if self.entries.is_empty() {
            format!("Remove Kiln's block from {}", self.hosts_path.display())
        } else {
            format!(
                "Point {} name(s) under .{} at this computer in {}",
                self.entries.len(),
                self.tld,
                self.hosts_path.display()
            )
        };
        Ok(Preview {
            summary,
            changes: vec![Change {
                target: self.hosts_path.display().to_string(),
                before: Self::render(&current),
                after: Self::render(&self.desired()),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        let bytes = self.read()?;
        let previous: Vec<[String; 2]> = Self::block_of(bytes.as_deref())?
            .into_iter()
            .map(|(ip, name)| [ip, name])
            .collect();
        Ok(Receipt(
            json!({ "absent": bytes.is_none(), "previous": previous }),
        ))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        if Self::block_of(self.read()?.as_deref())? == self.desired() {
            return Ok(()); // already in place: no elevation needed
        }
        self.writer.set_block(&self.tld, &self.entries, false)
    }

    fn verify(&self) -> Result<(), OpError> {
        let current = Self::block_of(self.read()?.as_deref())?;
        if current == self.desired() {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "Kiln's block in {} holds {} entries, expected {}",
                self.hosts_path.display(),
                current.len(),
                self.entries.len()
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, force: bool) -> Result<(), OpError> {
        let previous = parse_lines(&receipt.0["previous"], "receipt.previous")?;
        let was_absent = receipt.0["absent"].as_bool().unwrap_or(false);
        let bytes = self.read()?;
        let current = Self::block_of(bytes.as_deref())?;

        if was_absent {
            // Target state: no hosts file. Kiln only ever deletes a file that would be empty.
            let Some(bytes) = bytes else { return Ok(()) };
            if current.is_empty() && !bytes.is_empty() {
                return Ok(()); // someone else's file now; nothing of ours in it
            }
            if current != self.desired() && !current.is_empty() && !force {
                return Err(OpError::Drifted(format!(
                    "Kiln's block in {} was edited after it was written",
                    self.hosts_path.display()
                )));
            }
            return self.writer.set_block(&self.tld, &[], true);
        }

        if current == previous {
            return Ok(()); // never applied, or already reverted: nothing to do, no elevation
        }
        if current != self.desired() && !force {
            return Err(OpError::Drifted(format!(
                "Kiln's block in {} was edited after it was written",
                self.hosts_path.display()
            )));
        }
        // The previous block was written by Kiln under one TLD; recover it from the names.
        let tld = match previous.first() {
            Some((_, name)) => {
                let label = name.rsplit('.').next().unwrap_or_default();
                Tld::recorded(label).map_err(|e| failed(format!("previous block: {e}")))?
            }
            None => self.tld.clone(),
        };
        self.writer
            .set_block(&tld, &to_entries(&previous, &tld)?, false)
    }
}

/// Lets the engine rebuild [`HostsBlockOp`]s from the journal (undo, crash recovery).
pub fn register_hosts_block(
    registry: &mut Registry,
    hosts_path: PathBuf,
    writer: Rc<dyn PrivilegedHosts>,
) {
    registry.register(HOSTS_BLOCK_KIND, move |params| {
        Ok(Box::new(HostsBlockOp::from_params(
            hosts_path.clone(),
            writer.clone(),
            params,
        )?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_ops::{ApplyError, Engine, Journal, MemoryJournal, OpState};
    use std::cell::Cell;
    use std::path::Path;

    /// Writes directly (no elevation) with the same block editor, and counts calls so tests
    /// can assert when elevation would have been requested.
    struct DirectWriter {
        path: PathBuf,
        calls: Cell<u32>,
    }

    impl PrivilegedHosts for DirectWriter {
        fn set_block(
            &self,
            _tld: &Tld,
            entries: &[DnsEntry],
            remove_file_if_empty: bool,
        ) -> Result<(), OpError> {
            self.calls.set(self.calls.get() + 1);
            let current = fs::read(&self.path).unwrap_or_default();
            let updated =
                hosts::upsert_block(&current, entries).map_err(|e| failed(e.to_string()))?;
            if remove_file_if_empty && updated.is_empty() {
                fs::remove_file(&self.path).map_err(|e| failed(e.to_string()))
            } else {
                fs::write(&self.path, updated).map_err(|e| failed(e.to_string()))
            }
        }
    }

    const ORIGINAL: &[u8] = b"# hosts\r\n127.0.0.1 localhost\r\n";

    fn setup(original: Option<&[u8]>) -> (tempfile::TempDir, PathBuf, Rc<DirectWriter>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        if let Some(bytes) = original {
            fs::write(&path, bytes).unwrap();
        }
        let writer = Rc::new(DirectWriter {
            path: path.clone(),
            calls: Cell::new(0),
        });
        (dir, path, writer)
    }

    fn op(path: &Path, writer: &Rc<DirectWriter>, names: &[&str]) -> Box<dyn Operation> {
        let tld = Tld::default();
        let entries = names
            .iter()
            .map(|n| DnsEntry {
                name: HostName::new(n, &tld).unwrap(),
                ip: "127.0.0.1".parse().unwrap(),
            })
            .collect();
        Box::new(HostsBlockOp::new(
            path.to_owned(),
            writer.clone(),
            tld,
            entries,
        ))
    }

    fn engine(path: &Path, writer: &Rc<DirectWriter>) -> Engine<MemoryJournal> {
        let mut registry = Registry::default();
        register_hosts_block(&mut registry, path.to_owned(), writer.clone());
        Engine::new(MemoryJournal::default(), registry)
    }

    #[test]
    fn apply_then_undo_restores_the_exact_bytes() {
        let (_d, path, writer) = setup(Some(ORIGINAL));
        let mut engine = engine(&path, &writer);
        engine
            .apply(&[op(&path, &writer, &["blog.test", "shop.test"])])
            .unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("127.0.0.1 shop.test")
        );
        engine.revert_all(false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), ORIGINAL);
        assert_eq!(writer.calls.get(), 2, "one write to apply, one to undo");
    }

    #[test]
    fn nothing_to_change_means_no_privileged_call() {
        let (_d, path, writer) = setup(Some(ORIGINAL));
        let mut engine = engine(&path, &writer);
        engine.apply(&[op(&path, &writer, &["a.test"])]).unwrap();
        engine.apply(&[op(&path, &writer, &["a.test"])]).unwrap();
        assert_eq!(writer.calls.get(), 1, "re-applying the same state is free");

        // A crash after `prepare` but before the write: recovery finds nothing to undo.
        let fresh = op(&path, &writer, &["new.test"]);
        let receipt = fresh.prepare().unwrap();
        fresh.revert(&receipt, false).unwrap();
        assert_eq!(writer.calls.get(), 1);
    }

    #[test]
    fn a_hand_edited_block_is_not_overwritten_unless_forced() {
        let (_d, path, writer) = setup(Some(ORIGINAL));
        let mut engine = engine(&path, &writer);
        let report = engine.apply(&[op(&path, &writer, &["a.test"])]).unwrap();
        // The user edits Kiln's block by hand.
        let edited = String::from_utf8(fs::read(&path).unwrap())
            .unwrap()
            .replace("127.0.0.1 a.test", "127.0.0.1 mine.test");
        fs::write(&path, edited).unwrap();

        let id = report.entry_ids[0];
        assert!(matches!(
            engine.revert(id, false),
            Err(ApplyError::Revert {
                cause: OpError::Drifted(_),
                ..
            })
        ));
        engine.revert(id, true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), ORIGINAL);
    }

    #[test]
    fn foreign_edits_after_apply_survive_undo() {
        let (_d, path, writer) = setup(Some(ORIGINAL));
        let mut engine = engine(&path, &writer);
        engine.apply(&[op(&path, &writer, &["a.test"])]).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"10.0.0.5 nas.lan\r\n");
        fs::write(&path, &bytes).unwrap();

        engine.revert_all(false).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("10.0.0.5 nas.lan"),
            "other tools' later edits stay"
        );
        assert!(!after.contains("a.test"));
    }

    #[test]
    fn a_created_file_is_deleted_on_undo() {
        let (_d, path, writer) = setup(None);
        let mut engine = engine(&path, &writer);
        engine.apply(&[op(&path, &writer, &["a.test"])]).unwrap();
        assert!(path.exists());
        engine.revert_all(false).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn undo_restores_the_previous_block_not_just_removes_it() {
        let (_d, path, writer) = setup(Some(ORIGINAL));
        let mut engine = engine(&path, &writer);
        engine.apply(&[op(&path, &writer, &["one.test"])]).unwrap();
        let second = engine.apply(&[op(&path, &writer, &["two.test"])]).unwrap();
        engine.revert(second.entry_ids[0], false).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("one.test") && !text.contains("two.test"));
        let states: Vec<_> = engine
            .journal()
            .entries()
            .unwrap()
            .iter()
            .map(|e| e.state)
            .collect();
        assert_eq!(states, vec![OpState::Applied, OpState::Reverted]);
    }

    #[test]
    fn params_round_trip_through_the_registry() {
        let (_d, path, writer) = setup(Some(ORIGINAL));
        let original = op(&path, &writer, &["b.test", "a.test", "a.test"]);
        let rebuilt =
            HostsBlockOp::from_params(path.clone(), writer.clone(), &original.params()).unwrap();
        assert_eq!(rebuilt.params(), original.params());
        assert_eq!(
            original.params()["entries"].as_array().unwrap().len(),
            2,
            "deduplicated"
        );
        assert!(
            HostsBlockOp::from_params(path, writer, &json!({ "tld": "com", "entries": [] }))
                .is_err()
        );
    }

    #[test]
    fn preview_shows_before_and_after() {
        let (_d, path, writer) = setup(Some(ORIGINAL));
        let p = op(&path, &writer, &["blog.test"]).preview().unwrap();
        assert_eq!(p.changes[0].before, None);
        assert_eq!(p.changes[0].after.as_deref(), Some("127.0.0.1 blog.test"));
        assert_eq!(writer.calls.get(), 0, "preview never writes");
    }
}
