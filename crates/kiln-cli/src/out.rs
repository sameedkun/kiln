//! Everything `kiln` prints goes through here ([ADR 0053] §1-2).
//!
//! In text mode it prints as the CLI always has. With the global `--json` flag, standard
//! output is JSON Lines instead: `note`, `warning`, `progress`, `preview` and `confirm`
//! events, then exactly one `result` or `error`. The desktop app and `kilnd` read that.
//!
//! [ADR 0053]: ../../../docs/adr/0053-api-v2-and-json-output.md

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, ExitCode, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};

/// Exit code for "stopped at a confirmation; nothing was changed by it".
pub const NEEDS_CONFIRMATION: u8 = 3;

static JSON: AtomicBool = AtomicBool::new(false);
static CONFIRMED: AtomicBool = AtomicBool::new(false);
static DECLINED: AtomicBool = AtomicBool::new(false);
static DATA: Mutex<Option<Value>> = Mutex::new(None);

/// A line the command would have printed.
macro_rules! say {
    () => {
        $crate::out::note("")
    };
    ($($arg:tt)*) => {
        $crate::out::note(&format!($($arg)*))
    };
}

/// A line the command would have printed to standard error.
macro_rules! say_err {
    ($($arg:tt)*) => {
        $crate::out::warning(&format!($($arg)*))
    };
}

/// Turns JSON mode on, and with it `--confirmed`.
pub fn start(json: bool, confirmed: bool) {
    JSON.store(json, Ordering::SeqCst);
    CONFIRMED.store(json && confirmed, Ordering::SeqCst);
}

#[must_use]
pub fn json() -> bool {
    JSON.load(Ordering::SeqCst)
}

/// One JSON object on one line of standard output.
pub fn event(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

pub fn note(text: &str) {
    if json() {
        if !text.is_empty() {
            event(&json!({"type": "note", "text": text}));
        }
    } else {
        println!("{text}");
    }
}

pub fn warning(text: &str) {
    if json() {
        event(&json!({"type": "warning", "text": text}));
    } else {
        eprintln!("{text}");
    }
}

/// What a change will do, from its operations' own preview.
pub fn preview(preview: &kiln_ops::Preview) {
    event(&json!({
        "type": "preview",
        "summary": preview.summary,
        "changes": preview.changes.iter().map(|c| json!({
            "target": c.target,
            "before": c.before,
            "after": c.after,
        })).collect::<Vec<_>>(),
    }));
}

pub fn progress(label: &str, done: u64, total: Option<u64>) {
    event(&json!({"type": "progress", "label": label, "done": done, "total": total}));
}

/// The answer to a confirmation in JSON mode: never from the terminal. Yes only with
/// `--confirmed`; otherwise the decline is recorded, and the command ends with
/// `needs_confirmation`.
pub fn confirm(question: &str) -> bool {
    event(&json!({"type": "confirm", "question": question}));
    if CONFIRMED.load(Ordering::SeqCst) {
        true
    } else {
        DECLINED.store(true, Ordering::SeqCst);
        false
    }
}

/// The data a read command answers with, sent in its final `result`.
pub fn data(value: Value) {
    *DATA.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
}

/// The final line and the exit code.
pub fn finish(outcome: Result<(), String>) -> ExitCode {
    if !json() {
        return match outcome {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::FAILURE
            }
        };
    }
    let (line, code) = final_line(
        outcome,
        DECLINED.load(Ordering::SeqCst),
        DATA.lock().unwrap_or_else(PoisonError::into_inner).take(),
    );
    event(&line);
    ExitCode::from(code)
}

/// The last JSON line for an outcome, and its exit code.
fn final_line(outcome: Result<(), String>, declined: bool, data: Option<Value>) -> (Value, u8) {
    match outcome {
        Err(message) => (json!({"type": "error", "message": message}), 1),
        Ok(()) if declined => (
            json!({"type": "result", "ok": false, "needs_confirmation": true}),
            NEEDS_CONFIRMATION,
        ),
        Ok(()) => {
            let mut line = json!({"type": "result", "ok": true});
            if let Some(data) = data {
                line["data"] = data;
            }
            (line, 0)
        }
    }
}

/// Runs a program whose output the user should see (Composer, `php artisan`). In JSON mode
/// its lines are read and sent as notes, so nothing raw reaches the stream.
pub fn run_shown(command: &mut Command) -> std::io::Result<ExitStatus> {
    if !json() {
        return command.status();
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().map(|err| {
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                note(&line);
            }
        })
    });
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            note(&line);
        }
    }
    if let Some(thread) = stderr {
        let _ = thread.join();
    }
    child.wait()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_final_line_says_what_happened() {
        assert_eq!(
            final_line(Err("no such site".into()), false, None),
            (json!({"type": "error", "message": "no such site"}), 1)
        );
        assert_eq!(
            final_line(Ok(()), true, None),
            (
                json!({"type": "result", "ok": false, "needs_confirmation": true}),
                NEEDS_CONFIRMATION
            )
        );
        assert_eq!(
            final_line(Ok(()), false, Some(json!({"sites": []}))),
            (
                json!({"type": "result", "ok": true, "data": {"sites": []}}),
                0
            )
        );
        assert_eq!(
            final_line(Ok(()), false, None),
            (json!({"type": "result", "ok": true}), 0)
        );
    }

    #[test]
    fn an_error_wins_over_a_decline() {
        assert_eq!(final_line(Err("x".into()), true, None).1, 1);
    }
}
