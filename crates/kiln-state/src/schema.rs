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
    // v2: linked sites (ADR 0011). HTTP unless `secure` is set.
    "CREATE TABLE sites (
         host       TEXT    PRIMARY KEY,  -- e.g. blog.test
         root       TEXT    NOT NULL,     -- document root, absolute
         secure     INTEGER NOT NULL DEFAULT 0 CHECK (secure IN (0, 1)),
         created_ms INTEGER NOT NULL
     ) STRICT;",
    // v3: installed PHP runtimes (ADR 0013).
    "CREATE TABLE php_versions (
         version      TEXT    PRIMARY KEY,  -- e.g. 8.4.27
         dir          TEXT    NOT NULL,     -- absolute install folder
         archive      TEXT    NOT NULL,     -- file name it was installed from
         sha256       TEXT    NOT NULL,     -- verified digest of that archive
         installed_ms INTEGER NOT NULL
     ) STRICT;",
    // v4: parked folders; every subfolder is a site (ADR 0015).
    "CREATE TABLE parked (
         path       TEXT    PRIMARY KEY,  -- absolute folder
         created_ms INTEGER NOT NULL
     ) STRICT;",
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
