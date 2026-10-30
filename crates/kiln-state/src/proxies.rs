//! Proxy sites ([ADR 0032]): a host name served from a local dev server.
//!
//! [ADR 0032]: ../../../docs/adr/0032-proxy-sites.md

use rusqlite::{OptionalExtension, params};

use crate::store::{StateError, StateStore, now_ms};

/// One proxy site. Plain HTTP unless `secure` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyRecord {
    /// Fully qualified host name, e.g. `app.test`.
    pub host: String,
    /// The loopback URL it forwards to, e.g. `http://localhost:5173`.
    pub target: String,
    pub secure: bool,
}

fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProxyRecord> {
    Ok(ProxyRecord {
        host: row.get(0)?,
        target: row.get(1)?,
        secure: row.get(2)?,
    })
}

impl StateStore {
    /// All proxies, ordered by host name.
    pub fn proxies(&self) -> Result<Vec<ProxyRecord>, StateError> {
        let mut stmt = self
            .conn
            .prepare("SELECT host, target, secure FROM proxies ORDER BY host")?;
        let rows = stmt.query_map([], record)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn proxy(&self, host: &str) -> Result<Option<ProxyRecord>, StateError> {
        Ok(self
            .conn
            .query_row(
                "SELECT host, target, secure FROM proxies WHERE host = ?1",
                [host],
                record,
            )
            .optional()?)
    }

    /// Adds `proxy`, or updates its target and `secure` flag.
    pub fn put_proxy(&mut self, proxy: &ProxyRecord) -> Result<(), StateError> {
        self.conn.execute(
            "INSERT INTO proxies (host, target, secure, created_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(host) DO UPDATE SET target = excluded.target, secure = excluded.secure",
            params![proxy.host, proxy.target, proxy.secure, now_ms()],
        )?;
        Ok(())
    }

    /// Turns HTTPS on or off. Returns `false` if no such proxy exists.
    pub fn set_proxy_secure(&mut self, host: &str, secure: bool) -> Result<bool, StateError> {
        let changed = self.conn.execute(
            "UPDATE proxies SET secure = ?2 WHERE host = ?1",
            params![host, secure],
        )?;
        Ok(changed > 0)
    }

    /// Returns `false` if no such proxy exists.
    pub fn remove_proxy(&mut self, host: &str) -> Result<bool, StateError> {
        Ok(self
            .conn
            .execute("DELETE FROM proxies WHERE host = ?1", [host])?
            > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxies_are_added_updated_secured_and_removed() {
        let mut store = StateStore::open_in_memory().unwrap();
        let vite = ProxyRecord {
            host: "app.test".into(),
            target: "http://localhost:5173".into(),
            secure: false,
        };
        store.put_proxy(&vite).unwrap();
        store
            .put_proxy(&ProxyRecord {
                target: "http://localhost:5174".into(),
                ..vite.clone()
            })
            .unwrap();
        assert_eq!(
            store.proxy("app.test").unwrap().unwrap().target,
            "http://localhost:5174"
        );
        assert!(store.set_proxy_secure("app.test", true).unwrap());
        assert!(!store.set_proxy_secure("nope.test", true).unwrap());
        assert!(store.proxies().unwrap()[0].secure);
        assert!(store.remove_proxy("app.test").unwrap());
        assert!(!store.remove_proxy("app.test").unwrap());
        assert_eq!(store.proxies().unwrap(), []);
    }
}
