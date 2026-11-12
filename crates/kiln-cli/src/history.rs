//! `kiln history` ([ADR 0018] §1): what Kiln changed on this machine, newest first.
//!
//! [ADR 0018]: ../../../docs/adr/0018-history-undo-uninstall.md

use kiln_ops::{Journal, OpState};

/// `2026-10-22 14:03 UTC` from Unix milliseconds.
fn when(ms: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(ms / 1000).map_or_else(
        |_| "?".to_owned(),
        |t| {
            format!(
                "{}-{:02}-{:02} {:02}:{:02} UTC",
                t.year(),
                u8::from(t.month()),
                t.day(),
                t.hour(),
                t.minute()
            )
        },
    )
}

/// Whether an operation belongs in the default (non-`--all`) list: still in effect, or
/// needing attention.
fn current(state: OpState) -> bool {
    matches!(state, OpState::Applied | OpState::Prepared | OpState::Stuck)
}

pub fn history(all: bool) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let entries = store.entries().map_err(|e| e.to_string())?;
    let times = store.operation_times().map_err(|e| e.to_string())?;
    let shown: Vec<_> = entries
        .iter()
        .rev()
        .filter(|e| all || current(e.state))
        .collect();
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "operations": shown.iter().map(|e| serde_json::json!({
                "id": e.id,
                "at_ms": times.get(&e.id).map(|(created, _)| *created),
                "state": crate::apply::state_word(e.state).trim(),
                "kind": e.kind,
                "description": kiln_platform::describe::describe(&e.kind, &e.params),
            })).collect::<Vec<_>>(),
            "hidden": entries.iter().filter(|e| !current(e.state)).count(),
        }));
        return Ok(());
    }
    if shown.is_empty() {
        if entries.is_empty() {
            say!("Kiln hasn't changed anything on this machine yet.");
        } else {
            say!(
                "Nothing Kiln changed is in effect any more ({} past operation(s): `kiln history --all`).",
                entries.len()
            );
        }
        return Ok(());
    }
    for entry in shown {
        let at = times
            .get(&entry.id)
            .map_or_else(|| "?".to_owned(), |(created, _)| when(*created));
        say!(
            "#{:<4} {at}  {:<14} {}",
            entry.id,
            crate::apply::state_word(entry.state),
            kiln_platform::describe::describe(&entry.kind, &entry.params)
        );
    }
    let hidden = entries.iter().filter(|e| !current(e.state)).count();
    if !all && hidden > 0 {
        say!("({hidden} undone or rolled-back operation(s) hidden: `kiln history --all`)");
    }
    say!(
        "Undo the newest with `kiln undo`, or one with `kiln undo <n>`; everything with `kiln uninstall`."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_utc_minutes() {
        // 1_792_678_239 s = 2026-10-22 14:10:39 UTC
        assert_eq!(when(1_792_678_239_000), "2026-10-22 14:10 UTC");
        assert!(current(OpState::Applied) && current(OpState::Stuck));
        assert!(!current(OpState::Reverted) && !current(OpState::RolledBack));
    }
}
