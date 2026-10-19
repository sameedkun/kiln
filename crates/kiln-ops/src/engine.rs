use std::collections::HashMap;
use std::fmt;

use crate::journal::{Journal, JournalEntry, JournalError, OpState};
use crate::operation::{OpError, Operation, Preview};

type Builder = Box<dyn Fn(&serde_json::Value) -> Result<Box<dyn Operation>, OpError>>;

/// Rebuilds operations from their journaled `kind` and `params`, so they can be reverted by a
/// later process than the one that applied them.
#[derive(Default)]
pub struct Registry {
    builders: HashMap<&'static str, Builder>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut kinds: Vec<_> = self.builders.keys().collect();
        kinds.sort();
        f.debug_struct("Registry").field("kinds", &kinds).finish()
    }
}

impl Registry {
    pub fn register(
        &mut self,
        kind: &'static str,
        build: impl Fn(&serde_json::Value) -> Result<Box<dyn Operation>, OpError> + 'static,
    ) {
        self.builders.insert(kind, Box::new(build));
    }

    pub fn build(
        &self,
        kind: &str,
        params: &serde_json::Value,
    ) -> Result<Box<dyn Operation>, OpError> {
        let build = self
            .builders
            .get(kind)
            .ok_or_else(|| OpError::UnknownKind(kind.to_owned()))?;
        build(params)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    /// Operation `index` of the batch failed; the batch was rolled back. Any rollback that
    /// itself failed is listed in `stuck` and marked [`OpState::Stuck`] in the journal.
    #[error("operation {index} (`{kind}`) failed: {cause}")]
    Failed {
        index: usize,
        kind: &'static str,
        cause: OpError,
        stuck: Vec<(u64, OpError)>,
    },
    #[error("journal entry {0} not found")]
    NotFound(u64),
    #[error("journal entry {id} is {state:?}; only applied operations can be reverted")]
    NotRevertible { id: u64, state: OpState },
    #[error("reverting journal entry {id} failed: {cause}")]
    Revert { id: u64, cause: OpError },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    pub batch: u64,
    pub entry_ids: Vec<u64>,
}

#[derive(Debug, Default)]
pub struct RecoveryReport {
    /// Interrupted operations that were rolled back.
    pub rolled_back: Vec<u64>,
    /// Interrupted operations that could not be rolled back and need attention.
    pub stuck: Vec<(u64, OpError)>,
}

#[derive(Debug)]
pub struct Engine<J: Journal> {
    journal: J,
    registry: Registry,
}

impl<J: Journal> Engine<J> {
    pub fn new(journal: J, registry: Registry) -> Self {
        Self { journal, registry }
    }

    pub fn journal(&self) -> &J {
        &self.journal
    }

    pub fn preview(ops: &[Box<dyn Operation>]) -> Result<Vec<Preview>, OpError> {
        ops.iter().map(|op| op.preview()).collect()
    }

    fn next_ids(&self) -> Result<(u64, u64), JournalError> {
        let entries = self.journal.entries()?;
        let id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let batch = entries.iter().map(|e| e.batch).max().unwrap_or(0) + 1;
        Ok((id, batch))
    }

    fn set_state(
        &mut self,
        entry: &mut JournalEntry,
        state: OpState,
        note: Option<String>,
    ) -> Result<(), JournalError> {
        entry.state = state;
        entry.note = note;
        self.journal.record(entry)
    }

    /// Applies `ops` as one all-or-nothing batch.
    pub fn apply(&mut self, ops: &[Box<dyn Operation>]) -> Result<BatchReport, ApplyError> {
        let (mut next_id, batch) = self.next_ids()?;
        let mut done: Vec<(JournalEntry, &dyn Operation)> = Vec::new();

        for (index, op) in ops.iter().enumerate() {
            let receipt = match op.prepare() {
                Ok(receipt) => receipt,
                Err(cause) => return Err(self.fail_batch(index, op.kind(), cause, done)),
            };
            let mut entry = JournalEntry {
                id: next_id,
                batch,
                kind: op.kind().to_owned(),
                params: op.params(),
                scope: op.scope(),
                receipt,
                state: OpState::Prepared,
                note: None,
            };
            next_id += 1;
            self.journal.record(&entry)?;

            if let Err(cause) = op.apply(&entry.receipt).and_then(|()| op.verify()) {
                done.push((entry, op.as_ref()));
                return Err(self.fail_batch(index, op.kind(), cause, done));
            }
            self.set_state(&mut entry, OpState::Applied, None)?;
            done.push((entry, op.as_ref()));
        }

        Ok(BatchReport {
            batch,
            entry_ids: done.iter().map(|(entry, _)| entry.id).collect(),
        })
    }

    /// Rolls back everything touched so far in this batch, newest first.
    fn fail_batch(
        &mut self,
        index: usize,
        kind: &'static str,
        cause: OpError,
        done: Vec<(JournalEntry, &dyn Operation)>,
    ) -> ApplyError {
        let mut stuck = Vec::new();
        for (mut entry, op) in done.into_iter().rev() {
            let outcome = op.revert(&entry.receipt, false);
            let recorded = match outcome {
                Ok(()) => self.set_state(
                    &mut entry,
                    OpState::RolledBack,
                    Some(format!("batch failed: {cause}")),
                ),
                Err(err) => {
                    let recorded =
                        self.set_state(&mut entry, OpState::Stuck, Some(err.to_string()));
                    stuck.push((entry.id, err));
                    recorded
                }
            };
            if let Err(err) = recorded {
                return ApplyError::Journal(err);
            }
        }
        ApplyError::Failed {
            index,
            kind,
            cause,
            stuck,
        }
    }

    /// Undoes one applied operation, e.g. for `kiln undo <id>`.
    pub fn revert(&mut self, id: u64, force: bool) -> Result<(), ApplyError> {
        let mut entry = self
            .journal
            .entries()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or(ApplyError::NotFound(id))?;
        if entry.state != OpState::Applied {
            return Err(ApplyError::NotRevertible {
                id,
                state: entry.state,
            });
        }
        self.registry
            .build(&entry.kind, &entry.params)
            .and_then(|op| op.revert(&entry.receipt, force))
            .map_err(|cause| ApplyError::Revert { id, cause })?;
        self.set_state(&mut entry, OpState::Reverted, None)?;
        Ok(())
    }

    /// Undoes every applied operation, newest first. This is what uninstall runs.
    /// Stops at the first failure so later (older) changes are not reverted out of order.
    pub fn revert_all(&mut self, force: bool) -> Result<Vec<u64>, ApplyError> {
        let mut applied: Vec<u64> = self
            .journal
            .entries()?
            .into_iter()
            .filter(|e| e.state == OpState::Applied)
            .map(|e| e.id)
            .collect();
        applied.reverse();
        for &id in &applied {
            self.revert(id, force)?;
        }
        Ok(applied)
    }

    /// Rolls back operations left in [`OpState::Prepared`] by a crash. Run on daemon start.
    pub fn recover(&mut self) -> Result<RecoveryReport, JournalError> {
        let mut report = RecoveryReport::default();
        let mut interrupted: Vec<JournalEntry> = self
            .journal
            .entries()?
            .into_iter()
            .filter(|e| e.state == OpState::Prepared)
            .collect();
        interrupted.reverse();
        for mut entry in interrupted {
            let outcome = self
                .registry
                .build(&entry.kind, &entry.params)
                .and_then(|op| op.revert(&entry.receipt, false));
            match outcome {
                Ok(()) => {
                    self.set_state(
                        &mut entry,
                        OpState::RolledBack,
                        Some("interrupted; rolled back on recovery".into()),
                    )?;
                    report.rolled_back.push(entry.id);
                }
                Err(err) => {
                    self.set_state(&mut entry, OpState::Stuck, Some(err.to_string()))?;
                    report.stuck.push((entry.id, err));
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use proptest::prelude::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::journal::MemoryJournal;
    use crate::operation::{Change, Receipt, Scope};

    type Store = Rc<RefCell<BTreeMap<String, String>>>;

    /// Fake adapter: sets (or deletes, with `None`) one key in a shared map. `fail` makes
    /// apply change the key and then report failure, to exercise rollback of partial work.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct SetKey {
        key: String,
        value: Option<String>,
        #[serde(default)]
        fail: bool,
        #[serde(skip)]
        store: Store,
    }

    impl Operation for SetKey {
        fn kind(&self) -> &'static str {
            "test.set_key"
        }
        fn params(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap()
        }
        fn scope(&self) -> Scope {
            Scope::User
        }
        fn preview(&self) -> Result<Preview, OpError> {
            Ok(Preview {
                summary: format!("set {}", self.key),
                changes: vec![Change {
                    target: self.key.clone(),
                    before: self.store.borrow().get(&self.key).cloned(),
                    after: self.value.clone(),
                }],
            })
        }
        fn prepare(&self) -> Result<Receipt, OpError> {
            Ok(Receipt(
                json!({ "before": self.store.borrow().get(&self.key) }),
            ))
        }
        fn apply(&self, _: &Receipt) -> Result<(), OpError> {
            let mut store = self.store.borrow_mut();
            match &self.value {
                Some(v) => store.insert(self.key.clone(), v.clone()),
                None => store.remove(&self.key),
            };
            if self.fail {
                return Err(OpError::Failed("injected failure".into()));
            }
            Ok(())
        }
        fn verify(&self) -> Result<(), OpError> {
            if self.store.borrow().get(&self.key) == self.value.as_ref() {
                Ok(())
            } else {
                Err(OpError::VerifyFailed(self.key.clone()))
            }
        }
        fn revert(&self, receipt: &Receipt, force: bool) -> Result<(), OpError> {
            let before: Option<String> =
                serde_json::from_value(receipt.0["before"].clone()).unwrap();
            let mut store = self.store.borrow_mut();
            let current = store.get(&self.key).cloned();
            if current != self.value && current != before && !force {
                return Err(OpError::Drifted(self.key.clone()));
            }
            match before {
                Some(v) => store.insert(self.key.clone(), v),
                None => store.remove(&self.key),
            };
            Ok(())
        }
    }

    fn registry(store: &Store) -> Registry {
        let store = store.clone();
        let mut registry = Registry::default();
        registry.register("test.set_key", move |params| {
            let mut op: SetKey =
                serde_json::from_value(params.clone()).map_err(|source| OpError::BadParams {
                    kind: "test.set_key".into(),
                    source,
                })?;
            op.store = store.clone();
            Ok(Box::new(op))
        });
        registry
    }

    fn set(store: &Store, key: &str, value: Option<&str>, fail: bool) -> Box<dyn Operation> {
        Box::new(SetKey {
            key: key.into(),
            value: value.map(Into::into),
            fail,
            store: store.clone(),
        })
    }

    fn engine(store: &Store) -> Engine<MemoryJournal> {
        Engine::new(MemoryJournal::default(), registry(store))
    }

    fn states(engine: &Engine<MemoryJournal>) -> Vec<OpState> {
        engine
            .journal()
            .entries()
            .unwrap()
            .iter()
            .map(|e| e.state)
            .collect()
    }

    #[test]
    fn applies_a_batch_and_reverts_it_newest_first() {
        let store = Store::default();
        store.borrow_mut().insert("a".into(), "orig".into());
        let mut engine = engine(&store);

        let report = engine
            .apply(&[
                set(&store, "a", Some("1"), false),
                set(&store, "b", Some("2"), false),
            ])
            .unwrap();
        assert_eq!(report.entry_ids, vec![1, 2]);
        assert_eq!(store.borrow().get("a").map(String::as_str), Some("1"));

        assert_eq!(engine.revert_all(false).unwrap(), vec![2, 1]);
        assert_eq!(
            *store.borrow(),
            BTreeMap::from([("a".into(), "orig".into())])
        );
        assert_eq!(states(&engine), vec![OpState::Reverted, OpState::Reverted]);
    }

    #[test]
    fn failed_operation_rolls_back_the_whole_batch_including_partial_work() {
        let store = Store::default();
        let mut engine = engine(&store);
        let err = engine
            .apply(&[
                set(&store, "a", Some("1"), false),
                set(&store, "b", Some("2"), true),
            ])
            .unwrap_err();
        assert!(matches!(err, ApplyError::Failed { index: 1, ref stuck, .. } if stuck.is_empty()));
        assert!(store.borrow().is_empty());
        assert_eq!(
            states(&engine),
            vec![OpState::RolledBack, OpState::RolledBack]
        );
    }

    #[test]
    fn revert_refuses_drifted_targets_unless_forced() {
        let store = Store::default();
        let mut engine = engine(&store);
        engine.apply(&[set(&store, "a", Some("1"), false)]).unwrap();
        store
            .borrow_mut()
            .insert("a".into(), "edited by user".into());

        assert!(matches!(
            engine.revert(1, false),
            Err(ApplyError::Revert {
                cause: OpError::Drifted(_),
                ..
            })
        ));
        engine.revert(1, true).unwrap();
        assert!(store.borrow().is_empty());
        assert!(matches!(
            engine.revert(1, false),
            Err(ApplyError::NotRevertible { .. })
        ));
    }

    #[test]
    fn recovery_rolls_back_operations_interrupted_mid_apply() {
        let store = Store::default();
        let mut journal = MemoryJournal::default();
        // Simulate a crash: prepared and applied, but never marked applied.
        let op = set(&store, "a", Some("1"), false);
        let receipt = op.prepare().unwrap();
        journal
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

        let mut engine = Engine::new(journal, registry(&store));
        let report = engine.recover().unwrap();
        assert_eq!(report.rolled_back, vec![1]);
        assert!(store.borrow().is_empty());
        assert_eq!(states(&engine), vec![OpState::RolledBack]);
    }

    #[test]
    fn unknown_kinds_cannot_be_reverted_silently() {
        let store = Store::default();
        let mut engine = Engine::new(MemoryJournal::default(), Registry::default());
        engine.apply(&[set(&store, "a", Some("1"), false)]).unwrap();
        assert!(matches!(
            engine.revert(1, false),
            Err(ApplyError::Revert {
                cause: OpError::UnknownKind(_),
                ..
            })
        ));
    }

    fn arb_batch() -> impl Strategy<Value = Vec<(String, Option<String>, bool)>> {
        prop::collection::vec(
            (
                prop::sample::select(vec!["a", "b", "c"]).prop_map(String::from),
                prop::option::of("[xyz]{1,2}"),
                prop::bool::weighted(0.15),
            ),
            1..5,
        )
    }

    proptest! {
        /// Whatever mix of successful and failing batches runs, uninstall (`revert_all`)
        /// restores the original state exactly.
        #[test]
        fn revert_all_restores_the_original_state(
            initial in prop::collection::btree_map("[abc]", "[xyz]{1,2}", 0..3),
            batches in prop::collection::vec(arb_batch(), 1..6),
        ) {
            let store: Store = Rc::new(RefCell::new(initial.clone()));
            let mut engine = engine(&store);
            for batch in batches {
                let before = store.borrow().clone();
                let ops: Vec<_> = batch
                    .iter()
                    .map(|(k, v, fail)| set(&store, k, v.as_deref(), *fail))
                    .collect();
                if engine.apply(&ops).is_err() {
                    prop_assert_eq!(&*store.borrow(), &before, "failed batch must leave no trace");
                }
            }
            engine.revert_all(false).unwrap();
            prop_assert_eq!(&*store.borrow(), &initial);
        }
    }
}
