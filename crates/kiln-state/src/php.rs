//! Installed PHP runtimes ([ADR 0013]).
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

use std::path::PathBuf;

use rusqlite::params;

use crate::store::{StateError, StateStore, now_ms};

/// One installed PHP build and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpRecord {
    /// e.g. `8.4.27`
    pub version: String,
    pub dir: PathBuf,
    /// Archive file name it was installed from.
    pub archive: String,
    /// Verified SHA-256 of that archive, lowercase hex.
    pub sha256: String,
}

fn numbers(version: &str) -> Vec<u32> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

impl StateStore {
    /// Installed versions, oldest first.
    pub fn php_versions(&self) -> Result<Vec<PhpRecord>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT version, dir, archive, sha256 FROM php_versions")?;
        let rows = stmt.query_map([], |row| {
            Ok(PhpRecord {
                version: row.get(0)?,
                dir: PathBuf::from(row.get::<_, String>(1)?),
                archive: row.get(2)?,
                sha256: row.get(3)?,
            })
        })?;
        let mut records = rows.collect::<Result<Vec<_>, _>>()?;
        records.sort_by_key(|r| numbers(&r.version));
        Ok(records)
    }

    pub fn put_php(&mut self, record: &PhpRecord) -> Result<(), StateError> {
        let dir = record.dir.to_str().ok_or_else(|| StateError::Invalid {
            column: "php_versions.dir",
            detail: format!("{} is not valid Unicode", record.dir.display()),
        })?;
        self.conn.execute(
            "INSERT INTO php_versions (version, dir, archive, sha256, installed_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(version) DO UPDATE SET
                 dir = excluded.dir, archive = excluded.archive, sha256 = excluded.sha256",
            params![record.version, dir, record.archive, record.sha256, now_ms()],
        )?;
        Ok(())
    }

    /// Returns `false` if that version isn't recorded.
    pub fn remove_php(&mut self, version: &str) -> Result<bool, StateError> {
        Ok(self
            .conn
            .execute("DELETE FROM php_versions WHERE version = ?1", [version])?
            > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(version: &str) -> PhpRecord {
        PhpRecord {
            version: version.into(),
            dir: PathBuf::from(format!("/kiln/php/{version}")),
            archive: format!("php-{version}-nts-Win32-vs17-x64.zip"),
            sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn records_round_trip_in_version_order() {
        let mut store = StateStore::open_in_memory().unwrap();
        store.put_php(&record("8.10.1")).unwrap();
        store.put_php(&record("8.4.27")).unwrap();
        store.put_php(&record("8.4.27")).unwrap();
        let versions: Vec<_> = store
            .php_versions()
            .unwrap()
            .into_iter()
            .map(|r| r.version)
            .collect();
        assert_eq!(
            versions,
            ["8.4.27", "8.10.1"],
            "numeric order, no duplicates"
        );
        assert!(store.remove_php("8.4.27").unwrap());
        assert!(!store.remove_php("8.4.27").unwrap());
        assert_eq!(store.php_versions().unwrap(), vec![record("8.10.1")]);
    }
}

impl StateStore {
    /// Per-site PHP version pins: host → version spec (ADR 0016 §1–2).
    pub fn isolations(&self) -> Result<std::collections::BTreeMap<String, String>, StateError> {
        let mut stmt = self.conn.prepare("SELECT host, version FROM isolations")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn isolate(&mut self, host: &str, version: &str) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT INTO isolations (host, version, created_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(host) DO UPDATE SET version = excluded.version",
            params![host, version, now_ms()],
        )?;
        Ok(())
    }

    /// Returns `false` if the site wasn't pinned.
    pub fn unisolate(&mut self, host: &str) -> Result<bool, StateError> {
        Ok(self
            .conn
            .execute("DELETE FROM isolations WHERE host = ?1", [host])?
            > 0)
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::*;

    #[test]
    fn pins_round_trip() {
        let mut store = StateStore::open_in_memory().unwrap();
        store.isolate("legacy.test", "8.1").unwrap();
        store.isolate("legacy.test", "8.3").unwrap();
        store.isolate("api.test", "8.4.27").unwrap();
        let pins = store.isolations().unwrap();
        assert_eq!(pins.get("legacy.test").map(String::as_str), Some("8.3"));
        assert_eq!(pins.len(), 2);
        assert!(store.unisolate("legacy.test").unwrap());
        assert!(!store.unisolate("legacy.test").unwrap());
        assert_eq!(store.isolations().unwrap().len(), 1);
    }
}
