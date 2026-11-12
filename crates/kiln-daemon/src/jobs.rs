//! Running `kiln --json` and keeping what it says ([ADR 0053] §4): the read methods' runs
//! and the jobs that make changes.
//!
//! [ADR 0053]: ../../../docs/adr/0053-api-v2-and-json-output.md

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};

use crate::allow::Mode;

/// A job keeps at most this many events; later ones are counted, not kept.
pub const MAX_EVENTS: usize = 1000;
/// Finished jobs kept for `jobs.get` and `jobs.list`.
pub const KEEP_JOBS: usize = 20;
/// What is kept of a run's standard error, for when it ends without a final line.
const STDERR_KEPT: u64 = 16 * 1024;

/// How a run of `kiln --json` ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub ok: bool,
    pub exit: Option<i32>,
    pub needs_confirmation: bool,
    /// The final `result` or `error` line, or an `error` made up for a run without one.
    pub last: Value,
}

impl Outcome {
    fn error(message: &str, exit: Option<i32>) -> Self {
        Self {
            ok: false,
            exit,
            needs_confirmation: false,
            last: json!({"type": "error", "message": message}),
        }
    }
}

/// Reads the CLI's JSON Lines: each line before the final one goes to `on_event`; returns
/// the final `result` or `error`, if one came. A line that isn't JSON becomes a note.
pub fn read_lines(reader: impl BufRead, on_event: &mut dyn FnMut(Value)) -> Option<Value> {
    let mut last = None;
    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(line)
            .unwrap_or_else(|_| json!({"type": "note", "text": line}));
        match value["type"].as_str() {
            Some("result" | "error") => last = Some(value),
            _ => on_event(value),
        }
    }
    last
}

/// The outcome from the final line; the final line decides, not the exit code. Without
/// one (a crash, a usage error), an error from what it wrote to standard error.
#[must_use]
pub fn outcome(last: Option<Value>, exit: Option<i32>, stderr: &str) -> Outcome {
    match last {
        Some(line) if line["type"] == "result" => Outcome {
            ok: line["ok"].as_bool().unwrap_or(false),
            exit,
            needs_confirmation: line["needs_confirmation"].as_bool().unwrap_or(false),
            last: line,
        },
        Some(line) => Outcome {
            ok: false,
            exit,
            needs_confirmation: false,
            last: line,
        },
        None => {
            let how =
                exit.map_or_else(|| "it was stopped".to_owned(), |c| format!("exit code {c}"));
            let said = stderr.trim();
            Outcome::error(
                &if said.is_empty() {
                    format!("kiln ended without an answer ({how})")
                } else {
                    format!("{said} ({how})")
                },
                exit,
            )
        }
    }
}

/// Runs `kiln --json [--confirmed] <args>` without a window. `on_event` sees each line
/// before the final one; after `limit` the run is stopped.
pub fn run(
    kiln: &Path,
    args: &[String],
    confirmed: bool,
    limit: Duration,
    on_event: &mut dyn FnMut(Value),
) -> Outcome {
    let mut command = Command::new(kiln);
    command.arg("--json");
    if confirmed {
        command.arg("--confirmed");
    }
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return Outcome::error(&format!("cannot run {}: {e}", kiln.display()), None),
    };
    let stdout = child.stdout.take();
    // Drained on its own thread, so a chatty standard error can't stall standard output.
    let stderr = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = (&mut err).take(STDERR_KEPT).read_to_end(&mut kept);
            let _ = std::io::copy(&mut err, &mut std::io::sink());
            String::from_utf8_lossy(&kept).into_owned()
        })
    });
    let child = Arc::new(Mutex::new(child));
    let (done, finished) = mpsc::channel::<()>();
    let watchdog = {
        let child = Arc::clone(&child);
        std::thread::spawn(move || {
            if finished.recv_timeout(limit) == Err(RecvTimeoutError::Timeout) {
                let _ = child.lock().unwrap_or_else(PoisonError::into_inner).kill();
                true
            } else {
                false
            }
        })
    };
    let last = stdout.and_then(|out| read_lines(BufReader::new(out), on_event));
    let _ = done.send(());
    let timed_out = watchdog.join().unwrap_or(false);
    let status = child.lock().unwrap_or_else(PoisonError::into_inner).wait();
    let said = stderr.and_then(|t| t.join().ok()).unwrap_or_default();
    if timed_out {
        return Outcome::error(
            &format!(
                "kiln took longer than {} s and was stopped",
                limit.as_secs()
            ),
            None,
        );
    }
    outcome(last, status.ok().and_then(|s| s.code()), &said)
}

#[derive(Debug, Clone)]
struct Job {
    id: u64,
    args: Vec<String>,
    mode: Mode,
    started: String,
    events: Vec<Value>,
    dropped: usize,
    finished: Option<Value>,
}

impl Job {
    fn summary(&self) -> Value {
        json!({
            "job": self.id,
            "args": self.args,
            "mode": self.mode.name(),
            "started": self.started,
            "running": self.finished.is_none(),
            "finished": self.finished,
        })
    }
}

/// The jobs started through the API: running ones, and the newest finished ones.
#[derive(Debug, Default)]
pub struct Jobs {
    inner: Mutex<(u64, VecDeque<Job>)>,
}

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, (u64, VecDeque<Job>)> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a new job; returns its id.
    pub fn add(&self, args: Vec<String>, mode: Mode, started: String) -> u64 {
        let mut inner = self.lock();
        inner.0 += 1;
        let id = inner.0;
        inner.1.push_back(Job {
            id,
            args,
            mode,
            started,
            events: Vec::new(),
            dropped: 0,
            finished: None,
        });
        id
    }

    pub fn event(&self, id: u64, event: Value) {
        if let Some(job) = self.lock().1.iter_mut().find(|j| j.id == id) {
            if job.events.len() < MAX_EVENTS {
                job.events.push(event);
            } else {
                job.dropped += 1;
            }
        }
    }

    pub fn finish(&self, id: u64, finished: Value) {
        let mut inner = self.lock();
        if let Some(job) = inner.1.iter_mut().find(|j| j.id == id) {
            job.finished = Some(finished);
        }
        // Forget the oldest finished jobs beyond the limit; running ones always stay.
        while inner.1.iter().filter(|j| j.finished.is_some()).count() > KEEP_JOBS {
            if let Some(at) = inner.1.iter().position(|j| j.finished.is_some()) {
                inner.1.remove(at);
            }
        }
    }

    /// A job with its events so far.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<Value> {
        self.lock().1.iter().find(|j| j.id == id).map(|job| {
            let mut value = job.summary();
            value["events"] = json!(job.events);
            value["events_dropped"] = json!(job.dropped);
            value
        })
    }

    /// The newest jobs first, without their events.
    #[must_use]
    pub fn list(&self) -> Value {
        json!(
            self.lock()
                .1
                .iter()
                .rev()
                .map(Job::summary)
                .collect::<Vec<_>>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_before_the_last_are_events() {
        let text = concat!(
            r#"{"type":"preview","summary":"Point shop at redis-8","changes":[]}"#,
            "\n\n",
            "Composer says hello\n",
            r#"{"type":"confirm","question":"Point this project at redis-8?"}"#,
            "\n",
            r#"{"type":"result","ok":false,"needs_confirmation":true}"#,
            "\n"
        );
        let mut events = Vec::new();
        let last = read_lines(text.as_bytes(), &mut |e| events.push(e));
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[1],
            json!({"type": "note", "text": "Composer says hello"})
        );
        let outcome = outcome(last, Some(3), "");
        assert!(!outcome.ok && outcome.needs_confirmation);
    }

    #[test]
    fn the_final_line_decides_not_the_exit_code() {
        // `kiln doctor` exits 1 when a check fails, but its read worked.
        let done = outcome(
            Some(json!({"type": "result", "ok": true, "data": {}})),
            Some(1),
            "",
        );
        assert!(done.ok);
        let failed = outcome(
            Some(json!({"type": "error", "message": "no such site"})),
            Some(1),
            "",
        );
        assert!(!failed.ok);
        assert_eq!(failed.last["message"], "no such site");
    }

    #[test]
    fn a_run_without_a_final_line_is_an_error_from_its_stderr() {
        let usage = outcome(None, Some(2), "error: unrecognized subcommand 'nosuch'\n");
        assert!(!usage.ok);
        assert_eq!(
            usage.last["message"],
            "error: unrecognized subcommand 'nosuch' (exit code 2)"
        );
        let silent = outcome(None, None, "");
        assert_eq!(
            silent.last["message"],
            "kiln ended without an answer (it was stopped)"
        );
    }

    #[test]
    fn jobs_keep_a_bounded_history() {
        let jobs = Jobs::default();
        let first = jobs.add(vec!["link".into()], Mode::Apply, "now".into());
        for i in 0..MAX_EVENTS + 5 {
            jobs.event(first, json!({"type": "note", "text": i}));
        }
        let got = jobs.get(first).unwrap();
        assert_eq!(got["events"].as_array().unwrap().len(), MAX_EVENTS);
        assert_eq!(got["events_dropped"], 5);
        assert_eq!(got["running"], true);
        jobs.finish(first, json!({"ok": true}));
        for _ in 0..KEEP_JOBS + 3 {
            let id = jobs.add(vec!["unlink".into()], Mode::Apply, "now".into());
            jobs.finish(id, json!({"ok": true}));
        }
        let running = jobs.add(vec!["php".into()], Mode::Apply, "now".into());
        for _ in 0..3 {
            let id = jobs.add(vec!["web".into()], Mode::Apply, "now".into());
            jobs.finish(id, json!({"ok": true}));
        }
        assert!(
            jobs.get(first).is_none(),
            "the oldest finished job is forgotten"
        );
        assert!(
            jobs.get(running).is_some(),
            "a running job is never forgotten"
        );
        let listed = jobs.list();
        let listed = listed.as_array().unwrap();
        assert_eq!(listed.len(), KEEP_JOBS + 1);
        assert_eq!(listed[0]["job"], running + 3, "newest first");
    }
}
