//! What `kilnd` does ([ADR 0026] §2, §5): answers the API, keeps the web stack running, and
//! logs to a rotating file. Changes are made by running the `kiln` program next to it, so
//! there is one implementation of each operation and the CLI's change lock applies.
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use kiln_sys::background::ProcStatus;
use serde_json::{Value, json};

use crate::allow::{self, Mode};
use crate::jobs::{self, Jobs};
use crate::proto::{API_VERSION, Hello, RpcError};
use crate::server::{Events, Handler};

/// Rotate the log at this size, keeping this many old files.
const LOG_LIMIT: u64 = 5 * 1024 * 1024;
const LOG_KEEP: u32 = 3;

/// How long a read method's `kiln --json` may take ([ADR 0053] §4).
///
/// [ADR 0053]: ../../../docs/adr/0053-api-v2-and-json-output.md
const READ_LIMIT: Duration = Duration::from_secs(120);
/// How long a job may take: long enough for the largest download on a slow line.
const JOB_LIMIT: Duration = Duration::from_hours(2);

/// Where the daemon's files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Kiln's local data folder (`%LOCALAPPDATA%\Kiln`, `~/.local/share/kiln`).
    pub local: PathBuf,
    /// The API socket.
    pub socket: PathBuf,
    /// Held while a daemon runs, so there is only ever one.
    pub lock: PathBuf,
    pub log: PathBuf,
    /// Kiln's CA (`<state>/ca`).
    pub ca: PathBuf,
    /// Kiln's state folder, with its database.
    pub state: PathBuf,
}

impl Paths {
    #[must_use]
    pub fn new(local: &Path, state: &Path) -> Self {
        Self {
            local: local.to_path_buf(),
            socket: local.join("kilnd.sock"),
            lock: state.join("kilnd.lock"),
            log: local.join("logs").join("kilnd.log"),
            ca: state.join("ca"),
            state: state.to_path_buf(),
        }
    }

    /// The user's real folders, or `None` without a home directory.
    #[must_use]
    pub fn for_user() -> Option<Self> {
        Some(Self::new(
            &kiln_state::default_local_dir()?,
            &kiln_state::default_state_dir()?,
        ))
    }
}

/// The daemon's program file next to `kiln` (`kilnd.exe` on Windows).
pub const EXE_NAME: &str = if cfg!(windows) { "kilnd.exe" } else { "kilnd" };
const KILN_EXE: &str = if cfg!(windows) { "kiln.exe" } else { "kiln" };

/// An append-only log that rotates at [`LOG_LIMIT`].
#[derive(Debug)]
pub struct Log {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl Log {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            file: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends a timestamped line. Logging never fails the daemon.
    pub fn line(&self, text: &str) {
        let mut slot = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() || fs::metadata(&self.path).is_ok_and(|m| m.len() > LOG_LIMIT) {
            *slot = None;
            self.rotate();
            *slot = self.open().ok();
        }
        if let Some(file) = slot.as_mut() {
            let now = time::OffsetDateTime::now_utc();
            let _ = writeln!(file, "{now} {text}");
        }
    }

    fn open(&self) -> io::Result<File> {
        if let Some(dir) = self.path.parent() {
            kiln_sys::background::create_private_dir(dir)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
    }

    fn rotate(&self) {
        if !fs::metadata(&self.path).is_ok_and(|m| m.len() > LOG_LIMIT) {
            return;
        }
        let numbered = |n: u32| self.path.with_extension(format!("log.{n}"));
        let _ = fs::remove_file(numbered(LOG_KEEP));
        for n in (1..LOG_KEEP).rev() {
            let _ = kiln_sys::rename_replace(&numbered(n), &numbered(n + 1));
        }
        let _ = kiln_sys::rename_replace(&self.path, &numbered(1));
    }
}

/// Renew the intermediate when fewer days than this are left: the rule `kiln ca renew` uses.
pub const RENEW_BEFORE_DAYS: i64 = 10;
/// How often the daemon checks the intermediate (and once when it starts).
pub const CA_CHECK_EVERY: Duration = Duration::from_hours(6);

/// Whether an intermediate with `days_left` is due for renewal ([ADR 0026] §2, P2-03).
/// Without one there is nothing to renew: `kiln web start` issues the first.
///
/// [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md
#[must_use]
pub fn renewal_due(days_left: Option<i64>) -> bool {
    days_left.is_some_and(|days| days < RENEW_BEFORE_DAYS)
}

/// What the watchdog does about the web stack's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repair {
    Nothing,
    /// Caddy exited without `kiln web stop` (a crash or a reboot): `kiln web start`.
    Start,
    /// Caddy runs but the PHP supervisor exited: `kiln web restart`.
    Restart,
}

/// A PID file whose process has gone means it didn't stop through Kiln; a clean stop removes
/// the PID file. Only those are repaired, so a stopped server stays stopped.
#[must_use]
pub fn repair_for(caddy: &ProcStatus, supervisor: &ProcStatus) -> Repair {
    match (caddy, supervisor) {
        (ProcStatus::Stale { .. }, _) => Repair::Start,
        (ProcStatus::Running { .. }, ProcStatus::Stale { .. }) => Repair::Restart,
        _ => Repair::Nothing,
    }
}

/// Whether the services supervisor exited without being stopped through Kiln (a crash or a
/// reboot): a clean stop removes its PID file, so a stale one is the only case to repair.
#[must_use]
pub fn services_need_repair(state: &ProcStatus) -> bool {
    matches!(state, ProcStatus::Stale { .. })
}

/// Waits between repairs: none for the first, then longer each time, reset after a healthy
/// stretch, so a server that crashes at once doesn't restart in a tight loop.
#[derive(Debug, Default)]
pub struct Backoff {
    attempts: usize,
    next: Option<Instant>,
    healthy_since: Option<Instant>,
}

const DELAYS: [u64; 6] = [0, 30, 60, 120, 300, 600];
const HEALTHY_RESET: Duration = Duration::from_mins(10);

impl Backoff {
    /// Whether a repair may run at `now`; if so, it counts as an attempt.
    pub fn try_repair(&mut self, now: Instant) -> bool {
        if self.next.is_some_and(|next| now < next) {
            return false;
        }
        self.healthy_since = None;
        let delay = DELAYS[(self.attempts + 1).min(DELAYS.len() - 1)];
        self.attempts += 1;
        self.next = Some(now + Duration::from_secs(delay));
        true
    }

    /// Records a healthy check; after [`HEALTHY_RESET`] of them the delays start over.
    pub fn healthy(&mut self, now: Instant) {
        let since = *self.healthy_since.get_or_insert(now);
        if now.duration_since(since) >= HEALTHY_RESET {
            self.attempts = 0;
            self.next = None;
        }
    }

    #[must_use]
    pub fn attempts(&self) -> usize {
        self.attempts
    }
}

/// The running daemon: its API methods and its watchdog.
#[derive(Debug)]
pub struct Daemon {
    paths: Paths,
    log: Arc<Log>,
    events: Arc<Events>,
    started: SystemTime,
    shutdown: Mutex<Option<Sender<()>>>,
    repairs: Mutex<Vec<Value>>,
    /// Restarts of the services supervisor ([ADR 0048]).
    ///
    /// [ADR 0048]: ../../../docs/adr/0048-kilnd-and-services.md
    service_repairs: Mutex<Vec<Value>>,
    /// When the intermediate was last checked, and the renewals made since the daemon started.
    ca_checked: Mutex<Option<Instant>>,
    renewals: Mutex<Vec<Value>>,
    /// Jobs started through the API ([ADR 0053] §4).
    ///
    /// [ADR 0053]: ../../../docs/adr/0053-api-v2-and-json-output.md
    jobs: Arc<Jobs>,
    /// The `kiln` program next to the daemon.
    kiln: PathBuf,
}

impl Daemon {
    #[must_use]
    pub fn new(paths: Paths, log: Arc<Log>, events: Arc<Events>, shutdown: Sender<()>) -> Self {
        Self {
            paths,
            log,
            events,
            started: SystemTime::now(),
            shutdown: Mutex::new(Some(shutdown)),
            repairs: Mutex::new(Vec::new()),
            service_repairs: Mutex::new(Vec::new()),
            ca_checked: Mutex::new(None),
            renewals: Mutex::new(Vec::new()),
            jobs: Arc::new(Jobs::default()),
            kiln: std::env::current_exe().map_or_else(
                |_| PathBuf::from(KILN_EXE),
                |me| me.with_file_name(KILN_EXE),
            ),
        }
    }

    /// The methods [`Handler::call`] answers, besides `events.subscribe`.
    pub const METHODS: &[&str] = &[
        "kiln.hello",
        "daemon.status",
        "daemon.shutdown",
        "web.status",
        "services.status",
        "service.start",
        "service.stop",
        "service.restart",
        "ca.status",
        "ca.renew",
        "events.subscribe",
        // Version 2 (ADR 0053).
        "sites.list",
        "php.list",
        "php.available",
        "php.extensions",
        "node.list",
        "node.available",
        "services.list",
        "services.catalog",
        "history.list",
        "doctor.report",
        "jobs.start",
        "jobs.get",
        "jobs.list",
        "services.logs",
        "env.show",
        "setup.status",
        "settings.get",
    ];

    /// Days the web server's intermediate certificate has left, or `None` without one.
    /// Reads the certificate only; the CA's key stays locked.
    pub fn intermediate_days_left(&self) -> Result<Option<i64>, String> {
        let ca = kiln_pki::CaStore::new(self.paths.ca.clone(), kiln_pki::default_protector());
        Ok(ca
            .intermediate()
            .map_err(|e| e.to_string())?
            .map(|i| (i.not_after - time::OffsetDateTime::now_utc()).whole_days()))
    }

    /// The renewal timer: every [`CA_CHECK_EVERY`] (and on the first pass), renews the
    /// intermediate through `kiln ca renew` if it is due. A failure, such as a locked keyring,
    /// is logged and published, and tried again at the next check.
    pub fn check_ca(&self, now: Instant) {
        {
            let mut checked = self
                .ca_checked
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if checked.is_some_and(|at| now.duration_since(at) < CA_CHECK_EVERY) {
                return;
            }
            *checked = Some(now);
        }
        match self.intermediate_days_left() {
            Ok(days) if renewal_due(days) => {
                self.log.line(&format!(
                    "renewal: the intermediate certificate has {} day(s) left; running `kiln ca renew`",
                    days.unwrap_or_default()
                ));
                // Logged and published inside; the next check tries again if it failed.
                let _ = self.renew(false);
            }
            Ok(_) => {}
            Err(e) => self.log.line(&format!(
                "renewal: cannot read the intermediate certificate: {e}"
            )),
        }
    }

    /// Runs `kiln ca renew` (with `--force` if asked), logs and publishes the outcome.
    fn renew(&self, force: bool) -> Result<String, String> {
        let args: &[&str] = if force {
            &["ca", "renew", "--force"]
        } else {
            &["ca", "renew"]
        };
        let outcome = run_kiln(args);
        match &outcome {
            Ok(output) | Err(output) => {
                for line in output.lines() {
                    self.log.line(&format!("  {line}"));
                }
            }
        }
        let record = json!({
            "at": time::OffsetDateTime::now_utc().to_string(),
            "ok": outcome.is_ok(),
            "days_left": self.intermediate_days_left().ok().flatten(),
        });
        self.renewals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(record.clone());
        let event = if outcome.is_ok() {
            "ca.renewed"
        } else {
            "ca.renewal_failed"
        };
        self.events.publish(event, record);
        outcome
    }

    fn ca_status(&self) -> Value {
        let renewals = self
            .renewals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        json!({
            "intermediate_days_left": self.intermediate_days_left().ok().flatten(),
            "renew_before_days": RENEW_BEFORE_DAYS,
            "check_every_hours": CA_CHECK_EVERY.as_secs() / 3600,
            "renewals": renewals,
        })
    }

    fn web_server(&self) -> kiln_web::WebServer {
        kiln_web::WebServer::new(
            self.paths.local.join("bin").join(kiln_web::EXE_NAME),
            self.paths.local.join("caddy"),
        )
    }

    fn supervisor(&self) -> io::Result<kiln_supervisor::SupervisorHandle> {
        Ok(kiln_supervisor::SupervisorHandle::new(
            kiln_supervisor::handle::default_exe()?,
            self.paths.local.join("supervisor"),
        ))
    }

    /// The web server's and the PHP supervisor's state.
    pub fn web_states(&self) -> Result<(ProcStatus, ProcStatus), String> {
        let caddy = self.web_server().status().map_err(|e| e.to_string())?;
        let supervisor = self
            .supervisor()
            .and_then(|s| s.status())
            .map_err(|e| e.to_string())?;
        Ok((caddy, supervisor))
    }

    /// One watchdog pass: repairs the web stack if it exited on its own.
    pub fn watch(&self, backoff: &mut Backoff, now: Instant) {
        let (caddy, supervisor) = match self.web_states() {
            Ok(states) => states,
            Err(e) => {
                self.log.line(&format!(
                    "watchdog: cannot read the web server's state: {e}"
                ));
                return;
            }
        };
        let repair = repair_for(&caddy, &supervisor);
        if repair == Repair::Nothing {
            backoff.healthy(now);
            return;
        }
        if !backoff.try_repair(now) {
            return;
        }
        let args: &[&str] = match repair {
            Repair::Start => &["web", "start"],
            Repair::Restart => &["web", "restart"],
            Repair::Nothing => return,
        };
        self.log.line(&format!(
            "watchdog: {} (web server {caddy:?}, PHP supervisor {supervisor:?}); running `kiln {}` (attempt {})",
            match repair {
                Repair::Start => "the web server exited on its own",
                _ => "the PHP supervisor exited on its own",
            },
            args.join(" "),
            backoff.attempts()
        ));
        let outcome = run_kiln(args);
        let ok = outcome.is_ok();
        match &outcome {
            Ok(output) | Err(output) => {
                for line in output.lines() {
                    self.log.line(&format!("  {line}"));
                }
            }
        }
        let record = json!({
            "at": time::OffsetDateTime::now_utc().to_string(),
            "command": format!("kiln {}", args.join(" ")),
            "ok": ok,
        });
        self.repairs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(record.clone());
        self.events.publish("web.repaired", record);
    }

    fn services_supervisor(&self) -> io::Result<kiln_supervisor::SupervisorHandle> {
        Ok(kiln_supervisor::SupervisorHandle::for_programs(
            kiln_supervisor::handle::default_exe()?,
            self.paths.local.join("services").join("supervisor"),
            "services",
            Duration::from_mins(1),
        ))
    }

    /// The services supervisor's state ([ADR 0048] §1).
    ///
    /// [ADR 0048]: ../../../docs/adr/0048-kilnd-and-services.md
    pub fn services_state(&self) -> Result<ProcStatus, String> {
        self.services_supervisor()
            .and_then(|s| s.status())
            .map_err(|e| e.to_string())
    }

    /// One watchdog pass for services: if their supervisor exited on its own (a crash or a
    /// reboot), `kiln services resume` brings back what was running. One that was stopped
    /// through Kiln, or never started, stays as it is.
    pub fn watch_services(&self, backoff: &mut Backoff, now: Instant) {
        let state = match self.services_state() {
            Ok(state) => state,
            Err(e) => {
                self.log.line(&format!(
                    "watchdog: cannot read the services supervisor's state: {e}"
                ));
                return;
            }
        };
        if !services_need_repair(&state) {
            backoff.healthy(now);
            return;
        }
        if !backoff.try_repair(now) {
            return;
        }
        self.log.line(&format!(
            "watchdog: the services supervisor exited on its own ({state:?}); running `kiln services resume` (attempt {})",
            backoff.attempts()
        ));
        let record = self.run_and_record(&["services", "resume"]);
        self.service_repairs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(record.clone());
        self.events.publish("services.repaired", record);
    }

    /// Runs `kiln <args>`, logs what it printed, and returns a record of the run.
    fn run_and_record(&self, args: &[&str]) -> Value {
        let outcome = run_kiln(args);
        match &outcome {
            Ok(output) | Err(output) => {
                for line in output.lines() {
                    self.log.line(&format!("  {line}"));
                }
            }
        }
        json!({
            "at": time::OffsetDateTime::now_utc().to_string(),
            "command": format!("kiln {}", args.join(" ")),
            "ok": outcome.is_ok(),
        })
    }

    /// The supervisor's state and every instance, from Kiln's state: name, kind, version,
    /// ports, whether it should run and whether its main port answers.
    fn services_status(&self) -> Result<Value, String> {
        let supervisor = self.services_state()?;
        let (store, _) =
            kiln_state::StateStore::open_dir(&self.paths.state).map_err(|e| e.to_string())?;
        let instances: Vec<Value> = store
            .services()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|record| {
                let main = kiln_services::provider(&record.kind)
                    .and_then(|p| p.ports().first())
                    .and_then(|role| record.ports.get(role.role).copied());
                let answers = main.is_some_and(|port| {
                    std::net::TcpStream::connect_timeout(
                        &(std::net::Ipv4Addr::LOCALHOST, port).into(),
                        Duration::from_millis(300),
                    )
                    .is_ok()
                });
                json!({
                    "name": record.name,
                    "kind": record.kind,
                    "version": record.version,
                    "ports": record.ports,
                    "enabled": record.enabled,
                    "project": record.project,
                    "answers": record.enabled && answers,
                })
            })
            .collect();
        Ok(json!({"supervisor": state_json(&supervisor), "instances": instances}))
    }

    /// `service.start|stop|restart` through the CLI, for an instance that exists: the name is
    /// checked against Kiln's state first, so nothing else reaches the command line.
    fn service_command(&self, action: &str, params: &Value) -> Result<Value, RpcError> {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| RpcError::kiln("params.name (an instance) is missing"))?;
        let (store, _) = kiln_state::StateStore::open_dir(&self.paths.state)
            .map_err(|e| RpcError::kiln(e.to_string()))?;
        if store
            .service(name)
            .map_err(|e| RpcError::kiln(e.to_string()))?
            .is_none()
        {
            return Err(RpcError::kiln(format!("no service called {name}")));
        }
        drop(store);
        self.log.line(&format!(
            "services: `kiln service {action} {name}` asked through the API"
        ));
        run_kiln(&["service", action, name])
            .map(|output| json!({"ok": true, "output": output}))
            .map_err(RpcError::kiln)
    }

    /// A read method: `kiln --json <args>`, answered with its data ([ADR 0053] §4).
    ///
    /// [ADR 0053]: ../../../docs/adr/0053-api-v2-and-json-output.md
    fn read(&self, args: &[&str]) -> Result<Value, RpcError> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let outcome = jobs::run(&self.kiln, &args, false, READ_LIMIT, &mut |_| {});
        if outcome.ok {
            Ok(outcome.last.get("data").cloned().unwrap_or(Value::Null))
        } else {
            Err(RpcError::kiln(
                outcome.last["message"].as_str().unwrap_or("kiln failed"),
            ))
        }
    }

    /// `services.logs` (ADR 0056 §2): an instance's last lines, for an instance Kiln has.
    fn service_logs(&self, params: &Value) -> Result<Value, RpcError> {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| RpcError::kiln("params.name (an instance) is missing"))?;
        let (store, _) = kiln_state::StateStore::open_dir(&self.paths.state)
            .map_err(|e| RpcError::kiln(e.to_string()))?;
        if store
            .service(name)
            .map_err(|e| RpcError::kiln(e.to_string()))?
            .is_none()
        {
            return Err(RpcError::kiln(format!("no service called {name}")));
        }
        drop(store);
        let lines = params["lines"]
            .as_u64()
            .unwrap_or(200)
            .clamp(1, 5000)
            .to_string();
        self.read(&["service", "logs", name, "--lines", &lines])
    }

    /// `env.show` (ADR 0056 §2): a project's `.env`, only for the folder of one of Kiln's
    /// sites.
    fn env_show(&self, params: &Value) -> Result<Value, RpcError> {
        let project = params["project"]
            .as_str()
            .ok_or_else(|| RpcError::kiln("params.project (a site's folder) is missing"))?;
        let sites = self.read(&["sites"])?;
        let known = sites["sites"]
            .as_array()
            .is_some_and(|all| all.iter().any(|site| site["project"] == project));
        if !known {
            return Err(RpcError::kiln(format!(
                "{project} isn't the folder of one of Kiln's sites"
            )));
        }
        self.read(&["env", "show", "--project", project])
    }

    /// `jobs.start`: checks the command, answers with the job's id, and runs it on its own
    /// thread, sending each line as `job.event` and the end as `job.finished`.
    fn start_job(&self, params: &Value) -> Result<Value, RpcError> {
        let args: Vec<String> = params["args"]
            .as_array()
            .and_then(|list| list.iter().map(|a| a.as_str().map(str::to_owned)).collect())
            .ok_or_else(|| RpcError::kiln("params.args must be a list of strings"))?;
        let mode =
            Mode::parse(params["mode"].as_str().unwrap_or_default()).map_err(RpcError::kiln)?;
        allow::check(&args, mode).map_err(RpcError::kiln)?;
        let id = self.jobs.add(
            args.clone(),
            mode,
            time::OffsetDateTime::now_utc().to_string(),
        );
        self.log.line(&format!(
            "job {id}: kiln {} ({})",
            args.join(" "),
            mode.name()
        ));
        let (jobs, events, log, kiln) = (
            Arc::clone(&self.jobs),
            Arc::clone(&self.events),
            Arc::clone(&self.log),
            self.kiln.clone(),
        );
        std::thread::spawn(move || {
            let outcome = jobs::run(&kiln, &args, mode == Mode::Apply, JOB_LIMIT, &mut |event| {
                jobs.event(id, event.clone());
                events.publish("job.event", json!({"job": id, "event": event}));
            });
            let finished = json!({
                "job": id,
                "ok": outcome.ok,
                "exit": outcome.exit,
                "needs_confirmation": outcome.needs_confirmation,
                "last": outcome.last,
            });
            let how = if outcome.ok {
                "done"
            } else if outcome.needs_confirmation {
                "stopped at its confirmation"
            } else {
                "failed"
            };
            log.line(&format!("job {id}: {how}"));
            jobs.finish(id, finished.clone());
            events.publish("job.finished", finished);
            if mode == Mode::Apply {
                let command: Vec<_> = args.iter().take(2).cloned().collect();
                events.publish("state.changed", json!({"job": id, "command": command}));
            }
        });
        Ok(json!({"job": id}))
    }

    fn status(&self) -> Value {
        let uptime = self.started.elapsed().map_or(0, |d| d.as_secs());
        let started = time::OffsetDateTime::from(self.started).to_string();
        let repairs = self
            .repairs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        json!({
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
            "started": started,
            "uptime_secs": uptime,
            "log": self.log.path(),
            "socket": self.paths.socket,
            "repairs": repairs,
            "service_repairs": self
                .service_repairs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            "ca": self.ca_status(),
        })
    }
}

/// Runs `kiln <args>` from the daemon's own folder; `Ok` if it succeeded. Either way the text
/// is what it printed.
fn run_kiln(args: &[&str]) -> Result<String, String> {
    let exe = std::env::current_exe()
        .map(|me| me.with_file_name(KILN_EXE))
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(&exe);
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
    let output = command
        .output()
        .map_err(|e| format!("cannot run {}: {e}", exe.display()))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}

/// A PHP version as `php.extensions` takes it: 8.4 or 8.4.13, nothing else.
fn check_version(text: &str) -> Result<(), RpcError> {
    let fine = !text.is_empty()
        && text.len() <= 12
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if fine {
        Ok(())
    } else {
        Err(RpcError::kiln(format!(
            "{text:?} isn't a PHP version like 8.4"
        )))
    }
}

fn state_json(status: &ProcStatus) -> Value {
    match status {
        ProcStatus::Running { pid } => json!({"state": "running", "pid": pid}),
        ProcStatus::Stale { pid } => json!({"state": "exited", "pid": pid}),
        ProcStatus::Stopped => json!({"state": "stopped"}),
    }
}

impl Handler for Daemon {
    fn call(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "kiln.hello" => serde_json::to_value(Hello {
                api: API_VERSION,
                version: env!("CARGO_PKG_VERSION").into(),
                methods: Self::METHODS.iter().map(|m| (*m).to_owned()).collect(),
            })
            .map_err(|e| RpcError::kiln(e.to_string())),
            "daemon.status" => Ok(self.status()),
            "daemon.shutdown" => {
                self.log.line("stopping: asked through the API");
                if let Some(shutdown) = self
                    .shutdown
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
                {
                    let _ = shutdown.send(());
                }
                Ok(json!({"stopping": true}))
            }
            "web.status" => {
                let (caddy, supervisor) = self.web_states().map_err(RpcError::kiln)?;
                Ok(json!({"server": state_json(&caddy), "php_supervisor": state_json(&supervisor)}))
            }
            "services.status" => self.services_status().map_err(RpcError::kiln),
            "service.start" => self.service_command("start", params),
            "service.stop" => self.service_command("stop", params),
            "service.restart" => self.service_command("restart", params),
            "ca.status" => Ok(self.ca_status()),
            "ca.renew" => {
                let force = params["force"].as_bool().unwrap_or(false);
                self.log.line(&format!(
                    "renewal: asked through the API{}",
                    if force { " (forced)" } else { "" }
                ));
                self.renew(force)
                    .map(|output| json!({"ok": true, "output": output}))
                    .map_err(RpcError::kiln)
            }
            "sites.list" => self.read(&["sites"]),
            "php.list" => self.read(&["php", "list"]),
            "php.available" => self.read(&["php", "available"]),
            "php.extensions" => match params["php"].as_str() {
                Some(php) => {
                    check_version(php)?;
                    self.read(&["php", "ext", "list", "--php", php])
                }
                None => self.read(&["php", "ext", "list"]),
            },
            "node.list" => self.read(&["node", "list"]),
            "node.available" => self.read(&["node", "available"]),
            "services.list" => self.read(&["service", "list"]),
            "services.catalog" => self.read(&["service", "catalog"]),
            "history.list" => {
                if params["all"].as_bool().unwrap_or(false) {
                    self.read(&["history", "--all"])
                } else {
                    self.read(&["history"])
                }
            }
            "doctor.report" => self.read(&["doctor"]),
            "setup.status" => self.read(&["setup"]),
            "settings.get" => self.read(&["settings"]),
            "services.logs" => self.service_logs(params),
            "env.show" => self.env_show(params),
            "jobs.start" => self.start_job(params),
            "jobs.get" => {
                let id = params["job"]
                    .as_u64()
                    .ok_or_else(|| RpcError::kiln("params.job (a job id) is missing"))?;
                self.jobs
                    .get(id)
                    .ok_or_else(|| RpcError::kiln(format!("no job {id}")))
            }
            "jobs.list" => Ok(self.jobs.list()),
            other => Err(RpcError::method_not_found(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNNING: ProcStatus = ProcStatus::Running { pid: 1 };
    const EXITED: ProcStatus = ProcStatus::Stale { pid: 1 };
    const STOPPED: ProcStatus = ProcStatus::Stopped;

    #[test]
    fn only_a_stack_that_exited_on_its_own_is_repaired() {
        assert_eq!(
            repair_for(&EXITED, &EXITED),
            Repair::Start,
            "crash or reboot"
        );
        assert_eq!(repair_for(&EXITED, &STOPPED), Repair::Start);
        assert_eq!(repair_for(&RUNNING, &EXITED), Repair::Restart);
        assert_eq!(repair_for(&RUNNING, &RUNNING), Repair::Nothing);
        assert_eq!(
            repair_for(&RUNNING, &STOPPED),
            Repair::Nothing,
            "static sites only"
        );
        assert_eq!(
            repair_for(&STOPPED, &STOPPED),
            Repair::Nothing,
            "stopped by `kiln web stop`"
        );
        assert_eq!(repair_for(&STOPPED, &EXITED), Repair::Nothing);
    }

    #[test]
    fn only_a_services_supervisor_that_exited_on_its_own_is_restarted() {
        assert!(services_need_repair(&EXITED), "crash or reboot");
        assert!(!services_need_repair(&RUNNING));
        assert!(
            !services_need_repair(&STOPPED),
            "all stopped, or never started"
        );
    }

    #[test]
    fn the_intermediate_is_renewed_with_fewer_than_ten_days_left() {
        assert!(
            !renewal_due(None),
            "no intermediate yet: web start issues one"
        );
        assert!(!renewal_due(Some(30)));
        assert!(!renewal_due(Some(10)));
        assert!(renewal_due(Some(9)));
        assert!(renewal_due(Some(0)));
        assert!(renewal_due(Some(-3)), "already expired");
    }

    #[test]
    fn repairs_back_off_and_reset_after_a_healthy_stretch() {
        let start = Instant::now();
        let mut backoff = Backoff::default();
        assert!(backoff.try_repair(start), "the first repair runs at once");
        assert!(!backoff.try_repair(start + Duration::from_secs(10)));
        assert!(backoff.try_repair(start + Duration::from_secs(31)));
        assert!(
            !backoff.try_repair(start + Duration::from_secs(60)),
            "now 60 s apart"
        );
        assert!(backoff.try_repair(start + Duration::from_secs(92)));
        backoff.healthy(start + Duration::from_secs(100));
        backoff.healthy(start + Duration::from_secs(100) + HEALTHY_RESET);
        assert_eq!(backoff.attempts(), 0);
        assert!(backoff.try_repair(start + Duration::from_secs(101) + HEALTHY_RESET));
    }

    #[test]
    fn the_log_rotates_and_keeps_three_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("kilnd.log");
        let log = Log::new(path.clone());
        log.line("first");
        assert!(fs::read_to_string(&path).unwrap().contains("first"));
        for n in 0..5 {
            fs::write(&path, vec![b'x'; usize::try_from(LOG_LIMIT).unwrap() + 1]).unwrap();
            log.line(&format!("after rotation {n}"));
        }
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("after rotation 4")
        );
        for n in 1..=LOG_KEEP {
            assert!(path.with_extension(format!("log.{n}")).exists(), "log.{n}");
        }
        assert!(
            !path
                .with_extension(format!("log.{}", LOG_KEEP + 1))
                .exists()
        );
    }
}
