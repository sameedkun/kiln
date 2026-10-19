use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Receipt, Scope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpState {
    /// Prior state captured; the system may or may not have been changed yet.
    Prepared,
    /// Applied and verified. This is the only state that can be reverted on request.
    Applied,
    /// Undone automatically because its batch failed or the process crashed mid-apply.
    RolledBack,
    /// Undone on request (`kiln undo`, uninstall).
    Reverted,
    /// Rollback itself failed; needs attention. `note` says why.
    Stuck,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: u64,
    pub batch: u64,
    pub kind: String,
    pub params: serde_json::Value,
    pub scope: Scope,
    pub receipt: Receipt,
    pub state: OpState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("journal I/O error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("journal {path} is corrupt at line {line}: {source}")]
    Corrupt {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },
}

/// Durable record of operations. Writes are events; [`Journal::entries`] returns the latest
/// state of every operation in id order.
pub trait Journal {
    fn record(&mut self, entry: &JournalEntry) -> Result<(), JournalError>;
    fn entries(&self) -> Result<Vec<JournalEntry>, JournalError>;
}

fn latest_per_id(events: impl IntoIterator<Item = JournalEntry>) -> Vec<JournalEntry> {
    let mut by_id = BTreeMap::new();
    for event in events {
        by_id.insert(event.id, event);
    }
    by_id.into_values().collect()
}

/// In-memory journal for tests and dry runs.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    events: Vec<JournalEntry>,
}

impl Journal for MemoryJournal {
    fn record(&mut self, entry: &JournalEntry) -> Result<(), JournalError> {
        self.events.push(entry.clone());
        Ok(())
    }

    fn entries(&self) -> Result<Vec<JournalEntry>, JournalError> {
        Ok(latest_per_id(self.events.iter().cloned()))
    }
}

/// Append-only JSON Lines journal. Every event is flushed to disk before `record` returns.
///
/// A torn final line (crash mid-write) was never acknowledged to the caller, so [`open`]
/// truncates it before new events are appended; otherwise the next event would be glued onto
/// it and lost. Corruption anywhere else is an error, because silently dropping history could
/// hide changes that still need reverting.
///
/// [`open`]: FileJournal::open
#[derive(Debug)]
pub struct FileJournal {
    path: PathBuf,
}

impl FileJournal {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, JournalError> {
        let path = path.into();
        // Plain write access, not append: on Windows an append-only handle lacks the
        // FILE_WRITE_DATA right that truncation needs.
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| io_error(&path, source))?;
        let bytes = std::fs::read(&path).map_err(|source| io_error(&path, source))?;
        if bytes.last().is_some_and(|&b| b != b'\n') {
            let keep = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
            file.set_len(keep as u64)
                .and_then(|()| file.sync_data())
                .map_err(|source| io_error(&path, source))?;
        }
        Ok(Self { path })
    }
}

fn io_error(path: &Path, source: io::Error) -> JournalError {
    JournalError::Io {
        path: path.to_owned(),
        source,
    }
}

impl Journal for FileJournal {
    fn record(&mut self, entry: &JournalEntry) -> Result<(), JournalError> {
        let mut line = serde_json::to_vec(entry).expect("journal entries always serialize");
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|source| io_error(&self.path, source))?;
        file.write_all(&line)
            .and_then(|()| file.sync_data())
            .map_err(|source| io_error(&self.path, source))
    }

    fn entries(&self) -> Result<Vec<JournalEntry>, JournalError> {
        let file = File::open(&self.path).map_err(|source| io_error(&self.path, source))?;
        let lines: Vec<String> = BufReader::new(file)
            .lines()
            .collect::<Result<_, _>>()
            .map_err(|source| io_error(&self.path, source))?;
        let last = lines.len();
        let mut events = Vec::with_capacity(lines.len());
        for (index, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str(line) {
                Ok(event) => events.push(event),
                Err(_) if index + 1 == last => break,
                Err(source) => {
                    return Err(JournalError::Corrupt {
                        path: self.path.clone(),
                        line: index + 1,
                        source,
                    });
                }
            }
        }
        Ok(latest_per_id(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, state: OpState) -> JournalEntry {
        JournalEntry {
            id,
            batch: 1,
            kind: "test.kind".into(),
            params: serde_json::json!({ "k": id }),
            scope: Scope::User,
            receipt: Receipt(serde_json::json!(null)),
            state,
            note: None,
        }
    }

    #[test]
    fn file_journal_survives_reopen_and_keeps_latest_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let mut journal = FileJournal::open(&path).unwrap();
        journal.record(&entry(1, OpState::Prepared)).unwrap();
        journal.record(&entry(2, OpState::Prepared)).unwrap();
        journal.record(&entry(1, OpState::Applied)).unwrap();

        let reopened = FileJournal::open(&path).unwrap();
        let states: Vec<_> = reopened
            .entries()
            .unwrap()
            .iter()
            .map(|e| (e.id, e.state))
            .collect();
        assert_eq!(states, vec![(1, OpState::Applied), (2, OpState::Prepared)]);
    }

    fn append_raw(path: &Path, bytes: &[u8]) {
        OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }

    #[test]
    fn torn_final_line_is_dropped_on_open_so_later_events_are_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        FileJournal::open(&path)
            .unwrap()
            .record(&entry(1, OpState::Applied))
            .unwrap();
        append_raw(&path, br#"{"id":2,"bat"#);

        let mut journal = FileJournal::open(&path).unwrap();
        journal.record(&entry(3, OpState::Applied)).unwrap();
        let ids: Vec<_> = journal.entries().unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn corruption_before_the_last_line_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let mut journal = FileJournal::open(&path).unwrap();
        journal.record(&entry(1, OpState::Applied)).unwrap();
        append_raw(&path, b"not json\n");
        journal.record(&entry(2, OpState::Applied)).unwrap();
        assert!(matches!(
            journal.entries(),
            Err(JournalError::Corrupt { line: 2, .. })
        ));
    }
}
