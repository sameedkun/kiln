use rusqlite::Connection;

use crate::store::StateError;

/// Schema migrations, applied in order. Index `n` upgrades `user_version` `n` → `n + 1`.
/// Never edit a migration that has shipped; append a new one.
const MIGRATIONS: &[&str] = &[
    // v1: settings + operation journal with full event history.
    "CREATE TABLE settings (
         key   TEXT PRIMARY KEY,
         value TEXT NOT NULL
     ) STRICT;

     CREATE TABLE operations (
         id         INTEGER PRIMARY KEY,
         batch      INTEGER NOT NULL,
         kind       TEXT    NOT NULL,
         params     TEXT    NOT NULL,  -- JSON
         scope      TEXT    NOT NULL,
         receipt    TEXT    NOT NULL,  -- JSON
         state      TEXT    NOT NULL,
         note       TEXT,
         updated_ms INTEGER NOT NULL
     ) STRICT;

     CREATE TABLE operation_events (
         seq   INTEGER PRIMARY KEY AUTOINCREMENT,
         op_id INTEGER NOT NULL REFERENCES operations(id),
         state TEXT    NOT NULL,
         note  TEXT,
         at_ms INTEGER NOT NULL
     ) STRICT;

     CREATE INDEX operation_events_by_op ON operation_events(op_id, seq);
     CREATE INDEX operations_by_state ON operations(state);",
];

/// Schema version this build creates and understands.
#[allow(clippy::cast_possible_wrap)] // a handful of migrations; `try_from` is not const
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub(crate) fn migrate(conn: &mut Connection, location: &str) -> Result<(), StateError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(StateError::TooNew {
            location: location.to_owned(),
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    for (index, sql) in MIGRATIONS
        .iter()
        .enumerate()
        .skip(usize::try_from(current).unwrap_or(0))
    {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(
            None,
            "user_version",
            i64::try_from(index + 1).unwrap_or(i64::MAX),
        )?;
        tx.commit()?;
    }
    Ok(())
}
