//! Shared conformance tests for [`Journal`] implementations. Every backend (memory, JSON
//! Lines, the SQLite state store) runs the same checks, so the engine can rely on one
//! contract. Enabled for other crates with the `testing` feature.

use serde_json::json;

use crate::{Journal, JournalEntry, OpState, Receipt, Scope};

/// A fully populated entry; `id` and `state` vary per check.
#[must_use]
pub fn sample_entry(id: u64, state: OpState) -> JournalEntry {
    JournalEntry {
        id,
        batch: id / 2 + 1,
        kind: "dns.hosts.block".into(),
        params: json!({ "tld": "test", "names": ["blog.test", "shop.test"], "nested": { "n": id } }),
        scope: if id.is_multiple_of(2) {
            Scope::Machine
        } else {
            Scope::User
        },
        receipt: Receipt(
            json!({ "before": "# hosts\r\n127.0.0.1 localhost\r\n", "absent": false }),
        ),
        state,
        note: id
            .is_multiple_of(3)
            .then(|| format!("note for {id} — ünïcödé ✓")),
    }
}

/// Behaviour every journal must have, starting from an empty one.
///
/// # Panics
/// On any contract violation.
pub fn check_journal_contract<J: Journal>(mut journal: J) {
    assert!(
        journal.entries().unwrap().is_empty(),
        "a new journal is empty"
    );

    // Ids come back sorted regardless of write order, with every field intact.
    let three = sample_entry(3, OpState::Prepared);
    let one = sample_entry(1, OpState::Prepared);
    let two = sample_entry(2, OpState::Applied);
    for e in [&three, &one, &two] {
        journal.record(e).unwrap();
    }
    assert_eq!(
        journal.entries().unwrap(),
        vec![one.clone(), two.clone(), three.clone()]
    );

    // Recording an existing id replaces its latest state (including clearing a note).
    let mut three_done = three.clone();
    three_done.state = OpState::Reverted;
    three_done.note = None;
    journal.record(&three_done).unwrap();
    let entries = journal.entries().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[2], three_done);

    // The largest id SQLite-backed stores can hold.
    let big = sample_entry(u64::try_from(i64::MAX).unwrap(), OpState::Stuck);
    journal.record(&big).unwrap();
    assert_eq!(journal.entries().unwrap().last(), Some(&big));
}

/// Durability: what was recorded is identical after reopening. `open` must open the *same*
/// backing store every time it is called.
///
/// # Panics
/// On any contract violation.
pub fn check_journal_persistence<J: Journal>(open: impl Fn() -> J) {
    let expected = {
        let mut journal = open();
        journal.record(&sample_entry(1, OpState::Prepared)).unwrap();
        journal.record(&sample_entry(2, OpState::Prepared)).unwrap();
        journal.record(&sample_entry(1, OpState::Applied)).unwrap();
        journal.entries().unwrap()
    };
    assert_eq!(expected.len(), 2);
    assert_eq!(expected[0].state, OpState::Applied);
    let reopened = open();
    assert_eq!(reopened.entries().unwrap(), expected);
}
