//! `kilnd`: Kiln's optional per-user daemon ([ADR 0026]). Started by `kiln daemon start` (or
//! at login, P2-02); it has no console window and writes to its log.
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

// No console window on Windows, also when started at login.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::fs::{File, OpenOptions, TryLockError};
use std::process::ExitCode;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use kiln_daemon::daemon::{Backoff, Log};
use kiln_daemon::server::{self, Events, Handler};
use kiln_daemon::transport::Listener;
use kiln_daemon::{Daemon, Paths};

/// How often the watchdog checks the web stack.
const WATCH_EVERY: Duration = Duration::from_secs(15);

/// Exit code when another daemon already runs for this user.
const ALREADY_RUNNING: u8 = 3;

fn main() -> ExitCode {
    let Some(paths) = Paths::for_user() else {
        return ExitCode::FAILURE;
    };
    let log = Arc::new(Log::new(paths.log.clone()));
    let _lock = match lock(&paths) {
        Ok(Some(file)) => file,
        Ok(None) => {
            log.line("not started: another kilnd is already running for this user");
            return ExitCode::from(ALREADY_RUNNING);
        }
        Err(e) => {
            log.line(&format!(
                "not started: cannot take {}: {e}",
                paths.lock.display()
            ));
            return ExitCode::FAILURE;
        }
    };
    let listener = match Listener::bind(&paths.socket) {
        Ok(listener) => listener,
        Err(e) => {
            log.line(&format!(
                "not started: cannot listen on {}: {e}",
                paths.socket.display()
            ));
            return ExitCode::FAILURE;
        }
    };
    log.line(&format!(
        "started: kilnd {} (PID {}), API on {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        paths.socket.display()
    ));

    let events = Arc::new(Events::default());
    let (shutdown, stop) = mpsc::channel();
    let daemon = Arc::new(Daemon::new(
        paths.clone(),
        Arc::clone(&log),
        Arc::clone(&events),
        shutdown,
    ));

    let handler: Arc<dyn Handler> = daemon.clone();
    let server_log = Arc::clone(&log);
    std::thread::spawn(move || {
        let error = server::serve(&listener, &handler, &events);
        server_log.line(&format!("the API stopped accepting connections: {error}"));
    });
    let watcher = Arc::clone(&daemon);
    std::thread::spawn(move || {
        let mut backoff = Backoff::default();
        loop {
            watcher.watch(&mut backoff, Instant::now());
            std::thread::sleep(WATCH_EVERY);
        }
    });

    let _ = stop.recv();
    // Let the answer to `daemon.shutdown` reach its client.
    std::thread::sleep(Duration::from_millis(200));
    let _ = std::fs::remove_file(&paths.socket);
    log.line("stopped");
    ExitCode::SUCCESS
}

/// Takes the per-user daemon lock; `None` if another daemon holds it.
fn lock(paths: &Paths) -> std::io::Result<Option<File>> {
    if let Some(dir) = paths.lock.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&paths.lock)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}
