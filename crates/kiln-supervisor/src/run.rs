//! The supervisor's main loop ([ADR 0014] §2).
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::pools::Pools;
use crate::reconcile::{Event, Launcher, Supervisor};

/// How often the supervisor checks its workers and `pools.json`.
pub const TICK: Duration = Duration::from_millis(100);
const RELOAD_EVERY: Duration = Duration::from_millis(250);

/// Runs until `pools_file` is deleted or `stop` is set, then stops every worker.
pub fn run<L: Launcher>(
    pools_file: &Path,
    log_file: &Path,
    launcher: L,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;
    let mut supervisor = Supervisor::new(launcher);
    let mut desired = Pools::default();
    let mut seen: Option<SystemTime> = None;
    let mut last_check = Instant::now()
        .checked_sub(RELOAD_EVERY)
        .unwrap_or_else(Instant::now);
    write_line(
        &mut log,
        &format!("supervisor {} started", std::process::id()),
    );

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if last_check.elapsed() >= RELOAD_EVERY {
            last_check = Instant::now();
            let modified = std::fs::metadata(pools_file).and_then(|m| m.modified());
            match modified {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    write_line(&mut log, "pools.json removed: stopping");
                    break;
                }
                Err(e) => write_line(&mut log, &format!("cannot read pools.json: {e}")),
                Ok(modified) if seen != Some(modified) => match Pools::read(pools_file) {
                    Ok(Some(pools)) => {
                        seen = Some(modified);
                        let workers: usize = pools.pools.iter().map(|p| p.workers.len()).sum();
                        write_line(
                            &mut log,
                            &format!(
                                "pools.json: {} pool(s), {workers} worker(s)",
                                pools.pools.len()
                            ),
                        );
                        desired = pools;
                    }
                    Ok(None) => {}
                    // Keep the previous pools rather than stopping on a bad file.
                    Err(e) => write_line(&mut log, &format!("ignoring invalid pools.json: {e}")),
                },
                Ok(_) => {}
            }
        }
        for event in supervisor.reconcile(&desired, Instant::now()) {
            log_event(&mut log, &event);
        }
        std::thread::sleep(TICK);
    }
    for event in supervisor.stop_all() {
        log_event(&mut log, &event);
    }
    write_line(&mut log, "supervisor stopped");
    Ok(())
}

fn log_event(log: &mut impl Write, event: &Event) {
    let line = match event {
        // A program on no port (a queue worker, ADR 0060) is never recycled, and Kiln waits
        // for this line to know it started; every other start is routine.
        Event::Started { pool, port: 0 } => format!("{pool} :0 started"),
        Event::Started { .. } => return,
        Event::Exited {
            pool,
            port,
            crashed: false,
            ..
        } => format!("{pool} :{port} recycled"),
        Event::Exited {
            pool,
            port,
            crashed: true,
            retry_in,
        } => format!(
            "{pool} :{port} failed (non-zero exit); restarting in {} ms (see php-errors.log)",
            retry_in.as_millis()
        ),
        Event::StartFailed {
            pool,
            port,
            error,
            retry_in,
        } => format!(
            "{pool} :{port} failed to start: {error}; retrying in {} ms",
            retry_in.as_millis()
        ),
        Event::Stopped { pool, port } => format!("{pool} :{port} stopped"),
    };
    write_line(log, &line);
}

fn write_line(log: &mut impl Write, line: &str) {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let _ = writeln!(log, "{secs} {line}");
}
