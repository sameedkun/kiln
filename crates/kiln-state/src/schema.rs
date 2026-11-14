use rusqlite::{Connection, TransactionBehavior};

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
    // v5: per-site PHP versions (ADR 0016).
    "CREATE TABLE isolations (
         host       TEXT    PRIMARY KEY,  -- e.g. legacy.test
         version    TEXT    NOT NULL,     -- 8.3 (newest installed 8.3.x) or 8.3.36
         created_ms INTEGER NOT NULL
     ) STRICT;",
    // v6: proxy sites, served from a local dev server (ADR 0032).
    "CREATE TABLE proxies (
         host       TEXT    PRIMARY KEY,  -- e.g. app.test
         target     TEXT    NOT NULL,     -- loopback URL, e.g. http://localhost:5173
         secure     INTEGER NOT NULL DEFAULT 0 CHECK (secure IN (0, 1)),
         created_ms INTEGER NOT NULL
     ) STRICT;",
    // v7: service instances (ADR 0035, 0036).
    "CREATE TABLE services (
         name       TEXT    PRIMARY KEY,  -- e.g. mailpit-1
         kind       TEXT    NOT NULL,     -- e.g. mailpit
         version    TEXT    NOT NULL,     -- exact, e.g. 1.31.6
         ports      TEXT    NOT NULL,     -- JSON object, role -> port, e.g. {\"smtp\":1025}
         enabled    INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),  -- should run
         created_ms INTEGER NOT NULL
     ) STRICT;",
    // v8: a service that runs in a project, such as Reverb (ADR 0047): its folder.
    "ALTER TABLE services ADD COLUMN project TEXT;",
];

/// Schema version this build creates and understands.
#[allow(clippy::cast_possible_wrap)] // a handful of migrations; `try_from` is not const
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub(crate) fn migrate(conn: &mut Connection, location: &str) -> Result<(), StateError> {
    // One step at a time, each in a transaction that takes the write lock first and reads the
    // version under it: on a fresh PC the app starts several `kiln` processes at once,
    // and without the lock two of them both saw version 0 and both created the tables
    // ("table settings already exists").
    loop {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if current > SCHEMA_VERSION {
            return Err(StateError::TooNew {
                location: location.to_owned(),
                found: current,
                supported: SCHEMA_VERSION,
            });
        }
        if current == SCHEMA_VERSION {
            return Ok(());
        }
        tx.execute_batch(MIGRATIONS[usize::try_from(current).unwrap_or(0)])?;
        tx.pragma_update(None, "user_version", current + 1)?;
        tx.commit()?;
    }
}
