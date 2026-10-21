//! The operation engine running on the SQLite state store: changes applied by one "process"
//! are recovered or undone by the next, using only what was journaled.

use std::fs;
use std::path::{Path, PathBuf};

use kiln_ops::{
    Change, Engine, Journal, JournalEntry, OpError, OpState, Operation, Preview, Receipt, Registry,
    Scope,
};
use kiln_state::StateStore;
use serde_json::json;

/// Writes `contents` to a file; the receipt remembers what was there before.
#[derive(Debug)]
struct WriteFile {
    path: PathBuf,
    contents: String,
}

impl Operation for WriteFile {
    fn kind(&self) -> &'static str {
        "test.write_file"
    }
    fn params(&self) -> serde_json::Value {
        json!({ "path": self.path, "contents": self.contents })
    }
    fn scope(&self) -> Scope {
        Scope::User
    }
    fn preview(&self) -> Result<Preview, OpError> {
        Ok(Preview {
            summary: format!("write {}", self.path.display()),
            changes: vec![Change {
                target: self.path.display().to_string(),
                before: fs::read_to_string(&self.path).ok(),
                after: Some(self.contents.clone()),
            }],
        })
    }
    fn prepare(&self) -> Result<Receipt, OpError> {
        Ok(Receipt(
            json!({ "before": fs::read_to_string(&self.path).ok() }),
        ))
    }
    fn apply(&self, _: &Receipt) -> Result<(), OpError> {
        fs::write(&self.path, &self.contents).map_err(|e| OpError::Failed(e.to_string()))
    }
    fn verify(&self) -> Result<(), OpError> {
        match fs::read_to_string(&self.path) {
            Ok(c) if c == self.contents => Ok(()),
            _ => Err(OpError::VerifyFailed(self.path.display().to_string())),
        }
    }
    fn revert(&self, receipt: &Receipt, force: bool) -> Result<(), OpError> {
        let current = fs::read_to_string(&self.path).ok();
        let before = receipt.0["before"].as_str().map(str::to_owned);
        if current.as_deref() != Some(self.contents.as_str()) && current != before && !force {
            return Err(OpError::Drifted(self.path.display().to_string()));
        }
        match before {
            Some(b) => fs::write(&self.path, b),
            None => fs::remove_file(&self.path).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            }),
        }
        .map_err(|e| OpError::Failed(e.to_string()))
    }
}

fn registry() -> Registry {
    let mut r = Registry::default();
    r.register("test.write_file", |p| {
        Ok(Box::new(WriteFile {
            path: PathBuf::from(p["path"].as_str().unwrap_or_default()),
            contents: p["contents"].as_str().unwrap_or_default().to_owned(),
        }))
    });
    r
}

fn engine(db: &Path) -> Engine<StateStore> {
    Engine::new(StateStore::open(db).unwrap(), registry())
}

#[test]
fn a_later_process_recovers_an_interrupted_operation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let target = dir.path().join("hosts");
    fs::write(&target, "original").unwrap();

    // Process 1 journals "prepared", changes the file, then dies before "applied".
    {
        let mut store = StateStore::open(&db).unwrap();
        let op = WriteFile {
            path: target.clone(),
            contents: "half-done".into(),
        };
        let receipt = op.prepare().unwrap();
        store
            .record(&JournalEntry {
                id: 1,
                batch: 1,
                kind: op.kind().into(),
                params: op.params(),
                scope: op.scope(),
                receipt: receipt.clone(),
                state: OpState::Prepared,
                note: None,
            })
            .unwrap();
        op.apply(&receipt).unwrap();
    }

    // Process 2 starts, recovers, and the machine is back to how it was.
    let mut engine = engine(&db);
    let report = engine.recover().unwrap();
    assert_eq!(report.rolled_back, vec![1]);
    assert_eq!(fs::read_to_string(&target).unwrap(), "original");
    let history: Vec<_> = engine
        .journal()
        .history(1)
        .unwrap()
        .into_iter()
        .map(|(s, _)| s)
        .collect();
    assert_eq!(history, vec![OpState::Prepared, OpState::RolledBack]);
}

#[test]
fn a_later_process_undoes_everything_on_uninstall() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    fs::write(&a, "a0").unwrap();

    {
        let mut engine = engine(&db);
        let ops: Vec<Box<dyn Operation>> = vec![
            Box::new(WriteFile {
                path: a.clone(),
                contents: "a1".into(),
            }),
            Box::new(WriteFile {
                path: b.clone(),
                contents: "b1".into(),
            }),
        ];
        engine.apply(&ops).unwrap();
        engine
            .apply(&[Box::new(WriteFile {
                path: a.clone(),
                contents: "a2".into(),
            }) as Box<dyn Operation>])
            .unwrap();
    }
    assert_eq!(fs::read_to_string(&a).unwrap(), "a2");

    let mut engine = engine(&db);
    assert_eq!(engine.revert_all(false).unwrap(), vec![3, 2, 1]);
    assert_eq!(fs::read_to_string(&a).unwrap(), "a0");
    assert!(!b.exists(), "b did not exist before Kiln");
}
