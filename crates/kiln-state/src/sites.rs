//! Linked sites ([ADR 0011]): a host name, a document root and whether HTTPS is on.
//!
//! [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md

use std::path::PathBuf;

use rusqlite::{OptionalExtension, params};

use crate::store::{StateError, StateStore, now_ms};

/// One linked site. Sites are plain HTTP unless `secure` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRecord {
    /// Fully qualified host name, e.g. `blog.test`.
    pub host: String,
    /// Absolute document root.
    pub root: PathBuf,
    pub secure: bool,
}

fn root_text(root: &std::path::Path) -> Result<&str, StateError> {
    root.to_str().ok_or_else(|| StateError::Invalid {
        column: "sites.root",
        detail: format!("{} is not valid Unicode", root.display()),
    })
}

fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SiteRecord> {
    Ok(SiteRecord {
        host: row.get(0)?,
        root: PathBuf::from(row.get::<_, String>(1)?),
        secure: row.get(2)?,
    })
}

impl StateStore {
    /// All sites, ordered by host name.
    pub fn sites(&self) -> Result<Vec<SiteRecord>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT host, root, secure FROM sites ORDER BY host")?;
        let rows = stmt.query_map([], record)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn site(&self, host: &str) -> Result<Option<SiteRecord>, StateError> {
        Ok(self
            .conn
            .query_row(
                "SELECT host, root, secure FROM sites WHERE host = ?1",
                [host],
                record,
            )
            .optional()?)
    }

    /// Adds `site`, or updates its root and `secure` flag if the host is already linked.
    pub fn put_site(&mut self, site: &SiteRecord) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT INTO sites (host, root, secure, created_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(host) DO UPDATE SET root = excluded.root, secure = excluded.secure",
            params![site.host, root_text(&site.root)?, site.secure, now_ms()],
        )?;
        Ok(())
    }

    /// Turns HTTPS on or off. Returns `false` if no such site exists.
    pub fn set_site_secure(&mut self, host: &str, secure: bool) -> Result<bool, StateError> {
        let changed = self.conn.execute(
            "UPDATE sites SET secure = ?2 WHERE host = ?1",
            params![host, secure],
        )?;
        Ok(changed > 0)
    }

    /// Returns `false` if no such site exists.
    pub fn remove_site(&mut self, host: &str) -> Result<bool, StateError> {
        Ok(self
            .conn
            .execute("DELETE FROM sites WHERE host = ?1", [host])?
            > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(host: &str, secure: bool) -> SiteRecord {
        SiteRecord {
            host: host.into(),
            root: PathBuf::from(format!("/srv/{host}")),
            secure,
        }
    }

    #[test]
    fn sites_default_to_http_and_round_trip() {
        let mut store = StateStore::open_in_memory().unwrap();
        assert_eq!(store.sites().unwrap(), Vec::new());
        store.put_site(&site("shop.test", false)).unwrap();
        store.put_site(&site("blog.test", false)).unwrap();
        assert_eq!(
            store.sites().unwrap(),
            vec![site("blog.test", false), site("shop.test", false)]
        );
        // A column default of 0 means rows written without `secure` are HTTP too.
        store
            .conn
            .execute(
                "INSERT INTO sites (host, root, created_ms) VALUES ('raw.test', '/srv/raw', 0)",
                [],
            )
            .unwrap();
        assert!(!store.site("raw.test").unwrap().unwrap().secure);
    }

    #[test]
    fn secure_relink_and_remove() {
        let mut store = StateStore::open_in_memory().unwrap();
        store.put_site(&site("blog.test", false)).unwrap();
        assert!(store.set_site_secure("blog.test", true).unwrap());
        assert!(store.site("blog.test").unwrap().unwrap().secure);
        assert!(!store.set_site_secure("nope.test", true).unwrap());

        let moved = SiteRecord {
            root: PathBuf::from("/elsewhere"),
            ..site("blog.test", true)
        };
        store.put_site(&moved).unwrap();
        assert_eq!(store.sites().unwrap(), vec![moved]);

        assert!(store.remove_site("blog.test").unwrap());
        assert!(!store.remove_site("blog.test").unwrap());
        assert!(store.site("blog.test").unwrap().is_none());
    }
}
