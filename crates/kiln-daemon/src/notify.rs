//! Notifications ([ADR 0087]): mail Mailpit caught, a program the services supervisor saw
//! fail, a long API job done. kilnd keeps the newest [`KEEP`], survives a restart with them,
//! and publishes each as the event `notification`; the app shows them as Windows toasts and
//! in its list.
//!
//! [ADR 0087]: ../../../docs/adr/0087-notifications.md

use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// How many notifications are kept.
pub const KEEP: usize = 50;
/// The file they're kept in, in kilnd's state folder.
pub const FILE: &str = "notifications.json";
/// A program's failures are told at most this often.
const QUIET_FOR: Duration = Duration::from_mins(10);

/// A Mailpit's message count, and its newest message: subject, from, to.
pub type Inbox = (u64, Option<(String, String, String)>);

/// The newest notifications, newest last in memory.
#[derive(Debug)]
pub struct Notices {
    file: PathBuf,
    inner: Mutex<(u64, VecDeque<Value>)>,
}

impl Notices {
    /// The ones kept in `file`, if any.
    #[must_use]
    pub fn load(file: PathBuf) -> Self {
        let kept: VecDeque<Value> = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
            .unwrap_or_default()
            .into_iter()
            .collect();
        let next = kept
            .iter()
            .filter_map(|n| n["id"].as_u64())
            .max()
            .unwrap_or(0);
        Self {
            file,
            inner: Mutex::new((next, kept)),
        }
    }

    /// Records one; returns it, ready to publish.
    pub fn push(&self, kind: &str, title: &str, body: &str) -> Value {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.0 += 1;
        let notice = json!({
            "id": inner.0,
            "at": time::OffsetDateTime::now_utc().unix_timestamp(),
            "kind": kind,
            "title": title,
            "body": body,
        });
        inner.1.push_back(notice.clone());
        while inner.1.len() > KEEP {
            inner.1.pop_front();
        }
        self.save(&inner.1);
        notice
    }

    fn save(&self, all: &VecDeque<Value>) {
        let text = serde_json::to_string(&all.iter().collect::<Vec<_>>()).unwrap_or_default();
        let temp = self.file.with_extension("tmp");
        if std::fs::write(&temp, text).is_ok() {
            let _ = kiln_sys::rename_replace(&temp, &self.file);
        }
    }

    /// Newest first.
    #[must_use]
    pub fn list(&self) -> Value {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        json!({ "notifications": inner.1.iter().rev().collect::<Vec<_>>() })
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.1.clear();
        self.save(&inner.1);
    }
}

/// Mailpit's message count and newest message (subject, from, to), from its API on
/// 127.0.0.1:`port`.
#[must_use]
pub fn mailpit(port: u16) -> Option<Inbox> {
    let mut stream = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(500),
    )
    .ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        stream,
        "GET /api/v1/messages?limit=1 HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\n\r\n"
    )
    .ok()?;
    let mut response = Vec::new();
    let _ = stream.take(256 * 1024).read_to_end(&mut response);
    let text = String::from_utf8_lossy(&response);
    let body = text.split_once("\r\n\r\n")?.1;
    parse_messages(body)
}

/// Mailpit's `GET /api/v1/messages` answer, read.
fn parse_messages(body: &str) -> Option<Inbox> {
    let v: Value = serde_json::from_str(body).ok()?;
    let total = v["total"].as_u64()?;
    let newest = v["messages"].get(0).map(|m| {
        let who = |p: &Value| p["Address"].as_str().map(str::to_owned).unwrap_or_default();
        let to = m["To"]
            .as_array()
            .map(|all| all.iter().map(who).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        (
            m["Subject"].as_str().unwrap_or("(no subject)").to_owned(),
            who(&m["From"]),
            to,
        )
    });
    Some((total, newest))
}

/// The programs the supervisor's log says failed, from its new lines
/// (`<secs> <name> :<port> failed (non-zero exit)…` or `… failed to start: …`).
#[must_use]
pub fn failed_programs(new_lines: &str) -> Vec<String> {
    new_lines
        .lines()
        .filter_map(|line| {
            let rest = line.split_once(' ')?.1;
            let (name, after) = rest.split_once(" :")?;
            let after = after.split_once(' ')?.1;
            (after.starts_with("failed (non-zero exit)") || after.starts_with("failed to start"))
                .then(|| name.to_owned())
        })
        .collect()
}

/// What the notifier remembers between passes.
#[derive(Debug, Default)]
pub struct Watch {
    /// Each Mailpit's last count.
    mail: BTreeMap<String, u64>,
    /// How far the supervisor's log was read.
    log_at: Option<u64>,
    /// When each program's failure was last told.
    told: BTreeMap<String, Instant>,
}

impl Watch {
    /// A Mailpit's count now: the notification to give, if it went up since the last look.
    pub fn mail(&mut self, instance: &str, now: Inbox) -> Option<(String, String)> {
        let (total, newest) = now;
        let before = self.mail.insert(instance.to_owned(), total)?;
        if total <= before {
            return None;
        }
        let (subject, from, to) = newest?;
        let more = total - before;
        let title = if more == 1 {
            format!("Mail caught: {subject}")
        } else {
            format!("{more} emails caught; the newest: {subject}")
        };
        Some((title, format!("From {from} to {to}, in {instance}")))
    }

    /// The supervisor log's new part: the failures to tell (each program once in
    /// [`QUIET_FOR`]). The first look only notes where the log ends.
    pub fn log(&mut self, log: &std::path::Path, now: Instant) -> Vec<String> {
        let Ok(mut file) = std::fs::File::open(log) else {
            return Vec::new();
        };
        let len = file.metadata().map_or(0, |m| m.len());
        let from = match self.log_at {
            None => {
                self.log_at = Some(len);
                return Vec::new();
            }
            // A new log (the supervisor started again): from its start.
            Some(at) if at > len => 0,
            Some(at) => at,
        };
        self.log_at = Some(len);
        let mut text = String::new();
        if std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(from)).is_err()
            || (&mut file)
                .take(len - from)
                .read_to_string(&mut text)
                .is_err()
        {
            return Vec::new();
        }
        let mut out = Vec::new();
        for name in failed_programs(&text) {
            let quiet = self
                .told
                .get(&name)
                .is_some_and(|at| now.duration_since(*at) < QUIET_FOR);
            if !quiet && !out.contains(&name) {
                self.told.insert(name.clone(), now);
                out.push(name);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_keep_the_newest_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let notices = Notices::load(file.clone());
        for i in 0..(KEEP + 5) {
            notices.push("job", &format!("job {i}"), "");
        }
        let list = notices.list();
        let all = list["notifications"].as_array().unwrap();
        assert_eq!(all.len(), KEEP);
        assert_eq!(all[0]["title"], format!("job {}", KEEP + 4));
        let again = Notices::load(file);
        assert_eq!(again.push("mail", "m", "")["id"], (KEEP + 6) as u64);
        again.clear();
        assert_eq!(again.list()["notifications"], json!([]));
    }

    #[test]
    fn mailpits_answer_is_read() {
        let body = r#"{"total":3,"messages":[{"Subject":"Welcome!","From":{"Name":"Shop","Address":"hi@shop.test"},"To":[{"Address":"a@x.test"},{"Address":"b@x.test"}]}]}"#;
        assert_eq!(
            parse_messages(body),
            Some((
                3,
                Some((
                    "Welcome!".into(),
                    "hi@shop.test".into(),
                    "a@x.test, b@x.test".into()
                ))
            ))
        );
        assert_eq!(
            parse_messages(r#"{"total":0,"messages":[]}"#),
            Some((0, None))
        );
        assert_eq!(parse_messages("not json"), None);
    }

    #[test]
    fn mail_is_told_when_the_count_goes_up_not_on_the_first_look() {
        let mut w = Watch::default();
        let mail = |n: u64| {
            (
                n,
                Some(("Hi".to_owned(), "a@x".to_owned(), "b@x".to_owned())),
            )
        };
        assert_eq!(w.mail("mailpit-1", mail(4)), None);
        assert_eq!(w.mail("mailpit-1", mail(4)), None);
        assert_eq!(
            w.mail("mailpit-1", mail(5)),
            Some((
                "Mail caught: Hi".into(),
                "From a@x to b@x, in mailpit-1".into()
            ))
        );
        assert_eq!(
            w.mail("mailpit-1", mail(8)).unwrap().0,
            "3 emails caught; the newest: Hi"
        );
        assert_eq!(w.mail("mailpit-1", mail(0)), None, "emptied: nothing new");
    }

    #[test]
    fn failures_are_read_from_the_supervisors_log_once_per_quiet_time() {
        assert_eq!(
            failed_programs(
                "1700000000 queue-shop :0 failed (non-zero exit); restarting in 1000 ms (see php-errors.log)\n1700000001 vite-shop :5173 recycled\n1700000002 reverb :8080 failed to start: no such file; retrying in 2000 ms\n1700000003 queue-shop :0 started\n"
            ),
            ["queue-shop", "reverb"]
        );
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("supervisor.log");
        std::fs::write(&log, "1 old :0 failed (non-zero exit)\n").unwrap();
        let mut w = Watch::default();
        let now = Instant::now();
        assert!(
            w.log(&log, now).is_empty(),
            "the first look only notes the end"
        );
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(f, "2 queue-shop :0 failed (non-zero exit); restarting").unwrap();
        writeln!(f, "3 queue-shop :0 failed (non-zero exit); restarting").unwrap();
        assert_eq!(w.log(&log, now), ["queue-shop"]);
        writeln!(f, "4 queue-shop :0 failed (non-zero exit); restarting").unwrap();
        assert!(
            w.log(&log, now + Duration::from_secs(60)).is_empty(),
            "quiet for a while"
        );
        writeln!(f, "5 queue-shop :0 failed (non-zero exit); restarting").unwrap();
        assert_eq!(w.log(&log, now + QUIET_FOR), ["queue-shop"]);
    }
}
