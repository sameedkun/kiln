use std::path::Path;

use kiln_ops::FileJournal;

use crate::store::{StateError, StateStore, now_ms, record_in};

/// Result of importing a JSON Lines journal into the state store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Events replayed, including superseded states.
    pub events: usize,
    /// Distinct operations now in the store.
    pub operations: u64,
}

const IMPORTED_FROM: &str = "journal.imported_from";

impl StateStore {
    /// Replays every event of the JSON Lines journal at `source` into this store, preserving
    /// order, so each operation's latest state *and* history carry over.
    ///
    /// All-or-nothing (one transaction). Refused if this store already holds operations (ids
    /// could collide) or already imported a journal. The source file is only read.
    pub fn import_jsonl(&mut self, source: &Path) -> Result<ImportReport, StateError> {
        if let Some(previous) = self.setting(IMPORTED_FROM)? {
            return Err(StateError::ImportRefused(format!(
                "a journal was already imported from {previous}"
            )));
        }
        if self.operation_count()? > 0 {
            return Err(StateError::ImportRefused(
                "the state store already holds operations; importing could collide ids".into(),
            ));
        }
        let events = FileJournal::open_read_only(source)?.events()?;
        let at = now_ms();
        let tx = self.conn.transaction()?;
        for event in &events {
            record_in(&tx, event, at)?;
        }
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            [IMPORTED_FROM, &source.display().to_string()],
        )?;
        tx.commit()?;
        Ok(ImportReport {
            events: events.len(),
            operations: self.operation_count()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_ops::testing::sample_entry;
    use kiln_ops::{Journal, OpState};

    /// A Phase 0 journal: two operations with superseded states.
    fn legacy_journal(path: &Path) {
        let mut j = FileJournal::open(path).unwrap();
        j.record(&sample_entry(1, OpState::Prepared)).unwrap();
        j.record(&sample_entry(2, OpState::Prepared)).unwrap();
        j.record(&sample_entry(1, OpState::Applied)).unwrap();
        j.record(&sample_entry(2, OpState::RolledBack)).unwrap();
    }

    #[test]
    fn imports_latest_states_and_full_history_without_touching_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("journal.jsonl");
        legacy_journal(&source);
        let before = std::fs::read(&source).unwrap();

        let mut store = StateStore::open_in_memory().unwrap();
        let report = store.import_jsonl(&source).unwrap();
        assert_eq!(
            report,
            ImportReport {
                events: 4,
                operations: 2
            }
        );
        assert_eq!(
            store.entries().unwrap(),
            FileJournal::open_read_only(&source)
                .unwrap()
                .entries()
                .unwrap()
        );
        let history: Vec<_> = store
            .history(1)
            .unwrap()
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(history, vec![OpState::Prepared, OpState::Applied]);
        assert_eq!(
            std::fs::read(&source).unwrap(),
            before,
            "source is read-only"
        );
    }

    #[test]
    fn a_torn_final_line_in_the_source_is_ignored_not_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("journal.jsonl");
        legacy_journal(&source);
        let mut bytes = std::fs::read(&source).unwrap();
        bytes.extend_from_slice(br#"{"id":3,"bat"#);
        std::fs::write(&source, &bytes).unwrap();

        let mut store = StateStore::open_in_memory().unwrap();
        assert_eq!(store.import_jsonl(&source).unwrap().events, 4);
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
    }

    #[test]
    fn refuses_a_second_import_and_imports_into_a_used_store() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("journal.jsonl");
        legacy_journal(&source);

        let mut store = StateStore::open_in_memory().unwrap();
        store.import_jsonl(&source).unwrap();
        assert!(matches!(
            store.import_jsonl(&source),
            Err(StateError::ImportRefused(_))
        ));

        let mut used = StateStore::open_in_memory().unwrap();
        used.record(&sample_entry(9, OpState::Applied)).unwrap();
        assert!(matches!(
            used.import_jsonl(&source),
            Err(StateError::ImportRefused(_))
        ));
        assert_eq!(
            used.operation_count().unwrap(),
            1,
            "nothing partially imported"
        );
    }

    #[test]
    fn a_corrupt_source_imports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("journal.jsonl");
        legacy_journal(&source);
        let mut bytes = std::fs::read(&source).unwrap();
        bytes.extend_from_slice(b"garbage\n");
        bytes.extend_from_slice(&std::fs::read(&source).unwrap());
        std::fs::write(&source, &bytes).unwrap();

        let mut store = StateStore::open_in_memory().unwrap();
        assert!(matches!(
            store.import_jsonl(&source),
            Err(StateError::Journal(_))
        ));
        assert_eq!(store.operation_count().unwrap(), 0);
        assert_eq!(store.setting(IMPORTED_FROM).unwrap(), None);
    }

    #[test]
    fn open_dir_migrates_a_legacy_journal_once_and_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        legacy_journal(&dir.path().join("journal.jsonl"));

        let (store, report) = StateStore::open_dir(dir.path()).unwrap();
        assert_eq!(report.imported.unwrap().operations, 2);
        assert_eq!(store.operation_count().unwrap(), 2);
        drop(store);
        assert!(!dir.path().join("journal.jsonl").exists());
        assert!(
            dir.path().join("journal.jsonl.imported").exists(),
            "kept, not deleted"
        );

        let (store, report) = StateStore::open_dir(dir.path()).unwrap();
        assert_eq!(report.imported, None);
        assert_eq!(store.operation_count().unwrap(), 2);
    }
}
