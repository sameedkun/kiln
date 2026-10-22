//! Putting a folder first on the current user's `PATH`, reversibly ([ADR 0016] §11).
//!
//! Only Kiln's own entry is ever removed on undo: changes the user made to `PATH` in the
//! meantime survive. The value keeps its registry type, so `%VARS%` in it keep working.
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`UserPathOp`].
pub const USER_PATH_KIND: &str = "env.user_path.prepend";

/// The user `PATH` as stored: its text and whether it may contain `%VARS%`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPath {
    pub value: String,
    pub expandable: bool,
}

/// Where the user `PATH` lives: the registry in production, memory in tests.
pub trait PathStore: std::fmt::Debug {
    fn read(&self) -> Result<Option<StoredPath>, OpError>;
    fn write(&self, path: &StoredPath) -> Result<(), OpError>;
}

/// Entries compare case-insensitively and ignore a trailing backslash, as Windows does.
fn same_entry(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_lowercase();
    !a.trim().is_empty() && norm(a) == norm(b)
}

fn entries(value: &str) -> Vec<&str> {
    value.split(';').filter(|e| !e.trim().is_empty()).collect()
}

/// Puts `entry` first on the user `PATH` (if it isn't there already).
#[derive(Debug, Clone)]
pub struct UserPathOp {
    entry: String,
    store: Rc<dyn PathStore>,
}

impl UserPathOp {
    #[must_use]
    pub fn new(entry: &Path, store: Rc<dyn PathStore>) -> Self {
        Self {
            entry: entry.display().to_string(),
            store,
        }
    }

    fn current(&self) -> Result<StoredPath, OpError> {
        Ok(self.store.read()?.unwrap_or(StoredPath {
            value: String::new(),
            expandable: true,
        }))
    }

    fn present(&self, path: &StoredPath) -> bool {
        entries(&path.value)
            .iter()
            .any(|e| same_entry(e, &self.entry))
    }

    /// The `PATH` with Kiln's entry first.
    fn with_entry(&self, path: &StoredPath) -> StoredPath {
        let mut value = self.entry.clone();
        for entry in entries(&path.value) {
            if !same_entry(entry, &self.entry) {
                value.push(';');
                value.push_str(entry);
            }
        }
        StoredPath {
            value,
            expandable: path.expandable,
        }
    }
}

fn shown(path: &StoredPath) -> String {
    entries(&path.value).join("\n")
}

impl Operation for UserPathOp {
    fn kind(&self) -> &'static str {
        USER_PATH_KIND
    }

    fn params(&self) -> Value {
        json!({ "entry": self.entry })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let current = self.current()?;
        let first = entries(&current.value)
            .first()
            .is_some_and(|e| same_entry(e, &self.entry));
        Ok(Preview {
            summary: if first {
                format!("{} is already first on your PATH", self.entry)
            } else {
                format!("Put {} first on your user PATH", self.entry)
            },
            changes: vec![Change {
                target: r"HKCU\Environment\Path (your user PATH)".into(),
                before: Some(shown(&current)).filter(|s| !s.is_empty()),
                after: Some(shown(&self.with_entry(&current))),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        Ok(Receipt(
            json!({ "was_present": self.present(&self.current()?) }),
        ))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        let current = self.current()?;
        let first = entries(&current.value)
            .first()
            .is_some_and(|e| same_entry(e, &self.entry));
        if first {
            return Ok(());
        }
        self.store.write(&self.with_entry(&current))
    }

    fn verify(&self) -> Result<(), OpError> {
        let current = self.current()?;
        if entries(&current.value)
            .first()
            .is_some_and(|e| same_entry(e, &self.entry))
        {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "{} isn't first on the user PATH",
                self.entry
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, _force: bool) -> Result<(), OpError> {
        if receipt.0["was_present"].as_bool().unwrap_or(true) {
            return Ok(()); // it was on PATH before Kiln: leave it
        }
        let current = self.current()?;
        let kept: Vec<&str> = entries(&current.value)
            .into_iter()
            .filter(|e| !same_entry(e, &self.entry))
            .collect();
        self.store.write(&StoredPath {
            value: kept.join(";"),
            expandable: current.expandable,
        })
    }
}

/// The real user `PATH` in `HKCU\Environment` (Windows).
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct RegistryPath;

#[cfg(windows)]
impl PathStore for RegistryPath {
    fn read(&self) -> Result<Option<StoredPath>, OpError> {
        kiln_sys::win::read_user_reg_string(kiln_sys::win::USER_ENVIRONMENT_KEY, "Path")
            .map(|v| {
                v.map(|v| StoredPath {
                    value: v.value,
                    expandable: v.expandable,
                })
            })
            .map_err(|e| OpError::Failed(format!("cannot read the user PATH: {e}")))
    }

    fn write(&self, path: &StoredPath) -> Result<(), OpError> {
        kiln_sys::win::write_user_reg_string(
            kiln_sys::win::USER_ENVIRONMENT_KEY,
            "Path",
            &kiln_sys::win::RegString {
                value: path.value.clone(),
                expandable: path.expandable,
            },
        )
        .map_err(|e| OpError::Failed(format!("cannot write the user PATH: {e}")))?;
        // New terminals and Explorer pick the change up; open terminals keep their PATH.
        kiln_sys::win::broadcast_environment_change();
        Ok(())
    }
}

/// An in-memory `PATH` for tests.
#[derive(Debug, Default)]
pub struct MemoryPath(pub RefCell<Option<StoredPath>>);

impl PathStore for MemoryPath {
    fn read(&self) -> Result<Option<StoredPath>, OpError> {
        Ok(self.0.borrow().clone())
    }

    fn write(&self, path: &StoredPath) -> Result<(), OpError> {
        *self.0.borrow_mut() = Some(path.clone());
        Ok(())
    }
}

/// Lets the engine rebuild [`UserPathOp`]s from the journal, against `store`.
pub fn register_user_path(registry: &mut Registry, store: Rc<dyn PathStore>) {
    registry.register(USER_PATH_KIND, move |params| {
        let entry = params["entry"]
            .as_str()
            .ok_or_else(|| OpError::Failed("params.entry missing".into()))?;
        Ok(Box::new(UserPathOp::new(
            &PathBuf::from(entry),
            Rc::clone(&store),
        )))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_ops::{Engine, MemoryJournal};

    fn setup(initial: Option<&str>) -> (Rc<MemoryPath>, Engine<MemoryJournal>) {
        let store = Rc::new(MemoryPath(RefCell::new(initial.map(|v| StoredPath {
            value: v.into(),
            expandable: true,
        }))));
        let mut registry = Registry::default();
        register_user_path(&mut registry, store.clone());
        (store, Engine::new(MemoryJournal::default(), registry))
    }

    fn value(store: &MemoryPath) -> String {
        store.0.borrow().as_ref().unwrap().value.clone()
    }

    const SHIMS: &str = r"C:\Users\dev\AppData\Local\Kiln\bin\shims";

    #[test]
    fn prepends_and_undo_removes_only_kiln_s_entry() {
        let (store, mut engine) = setup(Some(r"%USERPROFILE%\.herd\bin;C:\Tools"));
        let op: Box<dyn Operation> = Box::new(UserPathOp::new(Path::new(SHIMS), store.clone()));
        let preview = op.preview().unwrap();
        assert!(
            preview.changes[0]
                .after
                .as_deref()
                .unwrap()
                .starts_with(SHIMS)
        );
        let report = engine.apply(&[op]).unwrap();
        assert_eq!(
            value(&store),
            format!(r"{SHIMS};%USERPROFILE%\.herd\bin;C:\Tools")
        );
        assert!(store.0.borrow().as_ref().unwrap().expandable, "type kept");

        // The user edits PATH afterwards; undo keeps that edit.
        let edited = format!(r"{SHIMS};%USERPROFILE%\.herd\bin;C:\Tools;D:\Mine");
        store.0.borrow_mut().as_mut().unwrap().value = edited;
        engine.revert(report.entry_ids[0], false).unwrap();
        assert_eq!(value(&store), r"%USERPROFILE%\.herd\bin;C:\Tools;D:\Mine");
    }

    #[test]
    fn an_entry_already_on_path_moves_first_and_stays_on_undo() {
        let (store, mut engine) = setup(Some(&format!(r"C:\Tools;{SHIMS}\")));
        let op: Box<dyn Operation> = Box::new(UserPathOp::new(Path::new(SHIMS), store.clone()));
        let report = engine.apply(&[op]).unwrap();
        assert_eq!(
            value(&store),
            format!(r"{SHIMS};C:\Tools"),
            "moved, not duplicated"
        );
        engine.revert(report.entry_ids[0], false).unwrap();
        assert_eq!(
            value(&store),
            format!(r"{SHIMS};C:\Tools"),
            "was there before Kiln"
        );
    }

    #[test]
    fn works_without_any_user_path_and_matches_case_insensitively() {
        let (store, mut engine) = setup(None);
        let op: Box<dyn Operation> = Box::new(UserPathOp::new(Path::new(SHIMS), store.clone()));
        let report = engine.apply(&[op]).unwrap();
        assert_eq!(value(&store), SHIMS);
        engine.revert(report.entry_ids[0], false).unwrap();
        assert_eq!(value(&store), "");
        assert!(same_entry(
            r"c:\users\DEV\appdata\local\kiln\bin\shims\",
            SHIMS
        ));
        assert!(!same_entry("", ""));
    }
}
