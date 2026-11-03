//! Service instances ([ADR 0035], [ADR 0036] §2): what runs, on which ports, and whether it
//! should be running.
//!
//! [ADR 0035]: ../../../docs/adr/0035-services.md
//! [ADR 0036]: ../../../docs/adr/0036-service-framework.md

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};

use crate::store::{StateError, StateStore, now_ms};

/// One service instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecord {
    /// Unique name, e.g. `mailpit-1` or `mysql-8.4`.
    pub name: String,
    /// The provider's kind, e.g. `mailpit`.
    pub kind: String,
    /// Exact version, e.g. `1.31.6`.
    pub version: String,
    /// Its ports by role, e.g. `smtp` → 1025.
    pub ports: BTreeMap<String, u16>,
    /// Whether it should be running (`kiln service start` / `stop`).
    pub enabled: bool,
    /// The project folder, for a service that runs in one (Reverb, [ADR 0047]).
    ///
    /// [ADR 0047]: ../../../docs/adr/0047-reverb.md
    pub project: Option<std::path::PathBuf>,
}

fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ServiceRecord> {
    let ports: String = row.get(3)?;
    Ok(ServiceRecord {
        name: row.get(0)?,
        kind: row.get(1)?,
        version: row.get(2)?,
        ports: serde_json::from_str(&ports).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
        })?,
        enabled: row.get(4)?,
        project: row
            .get::<_, Option<String>>(5)?
            .map(std::path::PathBuf::from),
    })
}

const COLUMNS: &str = "name, kind, version, ports, enabled, project";

impl StateStore {
    /// All instances, ordered by name.
    pub fn services(&self) -> Result<Vec<ServiceRecord>, StateError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM services ORDER BY name"))?;
        let rows = stmt.query_map([], record)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn service(&self, name: &str) -> Result<Option<ServiceRecord>, StateError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM services WHERE name = ?1"),
                [name],
                record,
            )
            .optional()?)
    }

    /// Adds `service`, or replaces the one with its name.
    pub fn put_service(&mut self, service: &ServiceRecord) -> Result<(), StateError> {
        let ports = serde_json::to_string(&service.ports).map_err(|e| {
            StateError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })?;
        self.conn.execute(
            "INSERT INTO services (name, kind, version, ports, enabled, created_ms, project)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(name) DO UPDATE SET kind = excluded.kind, version = excluded.version,
                 ports = excluded.ports, enabled = excluded.enabled, project = excluded.project",
            params![
                service.name,
                service.kind,
                service.version,
                ports,
                service.enabled,
                now_ms(),
                service
                    .project
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
            ],
        )?;
        Ok(())
    }

    /// Marks it as one that should run, or not. Returns `false` if no such instance exists.
    pub fn set_service_enabled(&mut self, name: &str, enabled: bool) -> Result<bool, StateError> {
        let changed = self.conn.execute(
            "UPDATE services SET enabled = ?2 WHERE name = ?1",
            params![name, enabled],
        )?;
        Ok(changed > 0)
    }

    /// Returns `false` if no such instance exists.
    pub fn remove_service(&mut self, name: &str) -> Result<bool, StateError> {
        Ok(self
            .conn
            .execute("DELETE FROM services WHERE name = ?1", [name])?
            > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_are_added_listed_toggled_and_removed() {
        let mut store = StateStore::open_in_memory().unwrap();
        let mail = ServiceRecord {
            name: "mailpit-1".into(),
            kind: "mailpit".into(),
            version: "1.31.6".into(),
            ports: BTreeMap::from([("smtp".into(), 1025), ("http".into(), 8025)]),
            enabled: true,
            project: None,
        };
        store.put_service(&mail).unwrap();
        assert_eq!(store.service("mailpit-1").unwrap(), Some(mail.clone()));
        assert_eq!(store.service("nope").unwrap(), None);

        assert!(store.set_service_enabled("mailpit-1", false).unwrap());
        assert!(!store.set_service_enabled("nope", false).unwrap());
        assert!(!store.services().unwrap()[0].enabled);

        // Replacing keeps one row per name.
        store
            .put_service(&ServiceRecord {
                version: "1.32.0".into(),
                ..mail.clone()
            })
            .unwrap();
        assert_eq!(store.services().unwrap().len(), 1);
        assert_eq!(store.services().unwrap()[0].version, "1.32.0");

        // A service that runs in a project keeps its folder.
        let reverb = ServiceRecord {
            name: "reverb-shop".into(),
            kind: "reverb".into(),
            project: Some(std::path::PathBuf::from("/sites/shop")),
            ..mail.clone()
        };
        store.put_service(&reverb).unwrap();
        assert_eq!(store.service("reverb-shop").unwrap(), Some(reverb));
        assert!(store.remove_service("reverb-shop").unwrap());

        assert!(store.remove_service("mailpit-1").unwrap());
        assert!(!store.remove_service("mailpit-1").unwrap());
        assert_eq!(store.services().unwrap(), []);
    }
}
