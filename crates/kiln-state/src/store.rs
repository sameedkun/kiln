use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kiln_ops::{Journal, JournalEntry, JournalError, OpState, Receipt, Scope};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::import::ImportReport;
use crate::paths::{DB_FILE, LEGACY_JOURNAL_FILE};
use crate::schema;

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("state database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "state database {location} has schema version {found}, but this Kiln supports up to \
         {supported}; it was written by a newer Kiln"
    )]
    TooNew {
        location: String,
        found: i64,
        supported: i64,
    },
    #[error("state database holds an invalid value in {column}: {detail}")]
    Invalid {
        column: &'static str,
        detail: String,
    },
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("journal import refused: {0}")]
    ImportRefused(String),
    #[error("cannot prepare state directory {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// What [`StateStore::open_dir`] did besides opening.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// Set when a legacy JSON Lines journal was found and imported.
    pub imported: Option<ImportReport>,
}

/// The per-user state database. Implements [`Journal`], so the operation engine can use it
/// directly.
#[derive(Debug)]
pub struct StateStore {
    pub(crate) conn: Connection,
    location: String,
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn to_text<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => unreachable!("unit enums serialize to strings, got {other:?}"),
    }
}

fn from_text<T: serde::de::DeserializeOwned>(
    column: &'static str,
    text: &str,
) -> Result<T, StateError> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).map_err(|e| {
        StateError::Invalid {
            column,
            detail: format!("{text:?}: {e}"),
        }
    })
}

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> JournalError {
    JournalError::Storage(Box::new(e))
}

impl StateStore {
    /// Opens (creating if needed) the database at `path` and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Self, StateError> {
        Self::init(Connection::open(path)?, path.display().to_string())
    }

    /// A private in-memory database, for tests and dry runs.
    pub fn open_in_memory() -> Result<Self, StateError> {
        Self::init(Connection::open_in_memory()?, ":memory:".into())
    }

    /// Opens `dir/state.db`, creating the directory if needed. If a legacy
    /// `dir/journal.jsonl` exists and the store's journal is still empty, its history is
    /// imported once and the file is renamed to `journal.jsonl.imported` (kept, not deleted).
    pub fn open_dir(dir: &Path) -> Result<(Self, OpenReport), StateError> {
        std::fs::create_dir_all(dir).map_err(|source| StateError::Io {
            path: dir.to_owned(),
            source,
        })?;
        let mut store = Self::open(&dir.join(DB_FILE))?;
        let mut report = OpenReport::default();
        let legacy = dir.join(LEGACY_JOURNAL_FILE);
        if legacy.is_file() && store.operation_count()? == 0 {
            report.imported = Some(store.import_jsonl(&legacy)?);
            let archived = dir.join(format!("{LEGACY_JOURNAL_FILE}.imported"));
            kiln_sys::rename_replace(&legacy, &archived).map_err(|source| StateError::Io {
                path: legacy.clone(),
                source,
            })?;
        }
        Ok((store, report))
    }

    fn init(mut conn: Connection, location: String) -> Result<Self, StateError> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // WAL keeps readers (CLI) and the writer (daemon) from blocking each other; FULL
        // fsyncs every commit, so an acknowledged journal entry is durable.
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate(&mut conn, &location)?;
        Ok(Self { conn, location })
    }

    /// Where the database lives (a file path, or `:memory:`).
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    pub fn schema_version(&self) -> Result<i64, StateError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, StateError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn operation_count(&self) -> Result<u64, StateError> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Number of operations in each state, for `kiln state info` and `kiln doctor`.
    pub fn counts_by_state(&self) -> Result<Vec<(OpState, u64)>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT state, COUNT(*) FROM operations GROUP BY state ORDER BY state")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.map(|row| {
            let (state, n) = row?;
            Ok((
                from_text("operations.state", &state)?,
                u64::try_from(n).unwrap_or(0),
            ))
        })
        .collect()
    }

    /// Every state an operation went through, oldest first, with notes.
    pub fn history(&self, id: u64) -> Result<Vec<(OpState, Option<String>)>, StateError> {
        let id = i64::try_from(id).map_err(|e| StateError::Invalid {
            column: "operations.id",
            detail: e.to_string(),
        })?;
        let mut stmt = self
            .conn
            .prepare("SELECT state, note FROM operation_events WHERE op_id = ?1 ORDER BY seq")?;
        let rows = stmt.query_map([id], |row| Ok((row.get::<_, String>(0)?, row.get(1)?)))?;
        rows.map(|row| {
            let (state, note) = row?;
            Ok((from_text("operation_events.state", &state)?, note))
        })
        .collect()
    }
}

/// Upserts the operation's latest state and appends a history event, inside `tx`.
pub(crate) fn record_in(
    tx: &Transaction<'_>,
    entry: &JournalEntry,
    at_ms: i64,
) -> Result<(), JournalError> {
    let id = i64::try_from(entry.id).map_err(storage)?;
    let batch = i64::try_from(entry.batch).map_err(storage)?;
    let params_json = serde_json::to_string(&entry.params).map_err(storage)?;
    let receipt_json = serde_json::to_string(&entry.receipt.0).map_err(storage)?;
    let state = to_text(&entry.state);
    tx.execute(
        "INSERT INTO operations (id, batch, kind, params, scope, receipt, state, note, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(id) DO UPDATE SET
             batch = excluded.batch, kind = excluded.kind, params = excluded.params,
             scope = excluded.scope, receipt = excluded.receipt, state = excluded.state,
             note = excluded.note, updated_ms = excluded.updated_ms",
        params![
            id,
            batch,
            entry.kind,
            params_json,
            to_text(&entry.scope),
            receipt_json,
            state,
            entry.note,
            at_ms
        ],
    )
    .map_err(storage)?;
    tx.execute(
        "INSERT INTO operation_events (op_id, state, note, at_ms) VALUES (?1, ?2, ?3, ?4)",
        params![id, state, entry.note, at_ms],
    )
    .map_err(storage)?;
    Ok(())
}

impl Journal for StateStore {
    fn record(&mut self, entry: &JournalEntry) -> Result<(), JournalError> {
        let tx = self.conn.transaction().map_err(storage)?;
        record_in(&tx, entry, now_ms())?;
        tx.commit().map_err(storage)
    }

    fn entries(&self) -> Result<Vec<JournalEntry>, JournalError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, batch, kind, params, scope, receipt, state, note
                 FROM operations ORDER BY id",
            )
            .map_err(storage)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })
            .map_err(storage)?;
        rows.map(|row| {
            let (id, batch, kind, params_json, scope, receipt_json, state, note) =
                row.map_err(storage)?;
            Ok(JournalEntry {
                id: u64::try_from(id).map_err(storage)?,
                batch: u64::try_from(batch).map_err(storage)?,
                kind,
                params: serde_json::from_str(&params_json).map_err(storage)?,
                scope: from_text::<Scope>("operations.scope", &scope).map_err(storage)?,
                receipt: Receipt(serde_json::from_str(&receipt_json).map_err(storage)?),
                state: from_text::<OpState>("operations.state", &state).map_err(storage)?,
                note,
            })
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_ops::testing::{check_journal_contract, check_journal_persistence, sample_entry};

    #[test]
    fn meets_the_journal_contract_in_memory_and_on_disk() {
        check_journal_contract(StateStore::open_in_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        check_journal_contract(StateStore::open(&dir.path().join("fresh.db")).unwrap());
        let path = dir.path().join("persist.db");
        check_journal_persistence(|| StateStore::open(&path).unwrap());
    }

    #[test]
    fn new_databases_are_at_the_current_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        assert_eq!(
            StateStore::open(&path).unwrap().schema_version().unwrap(),
            crate::SCHEMA_VERSION
        );
        // Reopening does not re-run migrations.
        assert_eq!(
            StateStore::open(&path).unwrap().schema_version().unwrap(),
            crate::SCHEMA_VERSION
        );
    }

    #[test]
    fn refuses_databases_from_a_newer_kiln() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        StateStore::open(&path)
            .unwrap()
            .conn
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(matches!(
            StateStore::open(&path),
            Err(StateError::TooNew { found: 99, .. })
        ));
    }

    #[test]
    fn refuses_files_that_are_not_databases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        std::fs::write(&path, b"this is not sqlite").unwrap();
        assert!(matches!(
            StateStore::open(&path),
            Err(StateError::Sqlite(_))
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"this is not sqlite",
            "left untouched"
        );
    }

    #[test]
    fn keeps_the_full_history_of_each_operation() {
        let mut store = StateStore::open_in_memory().unwrap();
        let mut e = sample_entry(7, OpState::Prepared);
        store.record(&e).unwrap();
        e.state = OpState::Applied;
        store.record(&e).unwrap();
        e.state = OpState::Reverted;
        e.note = Some("undo requested".into());
        store.record(&e).unwrap();
        let states: Vec<_> = store
            .history(7)
            .unwrap()
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(
            states,
            vec![OpState::Prepared, OpState::Applied, OpState::Reverted]
        );
        assert_eq!(
            store.history(7).unwrap()[2].1.as_deref(),
            Some("undo requested")
        );
        assert_eq!(
            store.counts_by_state().unwrap(),
            vec![(OpState::Reverted, 1)]
        );
    }

    #[test]
    fn settings_round_trip() {
        let mut store = StateStore::open_in_memory().unwrap();
        assert_eq!(store.setting("tld").unwrap(), None);
        store.set_setting("tld", "test").unwrap();
        store.set_setting("tld", "wip").unwrap();
        assert_eq!(store.setting("tld").unwrap().as_deref(), Some("wip"));
    }

    #[test]
    fn rejects_unknown_states_written_by_hand() {
        let mut store = StateStore::open_in_memory().unwrap();
        store.record(&sample_entry(1, OpState::Applied)).unwrap();
        store
            .conn
            .execute("UPDATE operations SET state = 'exploded'", [])
            .unwrap();
        assert!(matches!(store.entries(), Err(JournalError::Storage(_))));
    }
}
