//! `kiln daemon start | stop | status | call` ([ADR 0026] §5): the optional per-user daemon.
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kiln_daemon::{Client, ClientError, Paths};
use serde_json::Value;

/// How long `start` waits for the API to answer, and `stop` for the daemon to exit.
const WAIT: Duration = Duration::from_secs(10);

fn paths() -> Result<Paths, String> {
    Paths::for_user().ok_or_else(|| "cannot determine Kiln's data directories".to_owned())
}

fn exe() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = me.with_file_name(kiln_daemon::EXE_NAME);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!(
            "{} is missing; it ships next to kiln",
            exe.display()
        ))
    }
}

/// Connects and checks the API version, or `None` if the daemon isn't running.
fn connect(paths: &Paths) -> Result<Option<Client>, String> {
    match Client::connect(&paths.socket) {
        Ok(mut client) => {
            let hello = client.hello().map_err(|e| e.to_string())?;
            if hello.api != kiln_daemon::proto::API_VERSION {
                return Err(format!(
                    "Kiln's daemon speaks API {} but this kiln speaks {}; restart it with `kiln daemon stop` and `kiln daemon start`",
                    hello.api,
                    kiln_daemon::proto::API_VERSION
                ));
            }
            Ok(Some(client))
        }
        Err(ClientError::NotRunning) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn pid_of(client: &mut Client) -> Result<u64, String> {
    let status = client
        .call("daemon.status", Value::Null)
        .map_err(|e| e.to_string())?;
    Ok(status["pid"].as_u64().unwrap_or_default())
}

pub fn start() -> Result<(), String> {
    let paths = paths()?;
    if let Some(mut client) = connect(&paths)? {
        say!(
            "Kiln's daemon is already running (PID {}).",
            pid_of(&mut client)?
        );
        return Ok(());
    }
    let mut command = Command::new(exe()?);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = kiln_sys::background::spawn_background(&mut command)
        .map_err(|e| format!("cannot start Kiln's daemon: {e}"))?;
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(mut client) = connect(&paths)? {
            say!(
                "Kiln's daemon started (PID {}). It keeps the web server running and logs to {}.",
                pid_of(&mut client)?,
                paths.log.display()
            );
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "Kiln's daemon exited at once ({status}); see {}",
                paths.log.display()
            ));
        }
        if Instant::now() > deadline {
            return Err(format!(
                "Kiln's daemon didn't answer within {} s; see {}",
                WAIT.as_secs(),
                paths.log.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Stops the daemon if it runs; `Ok(true)` if it was running. Used by `uninstall` too.
pub fn stop_quietly() -> Result<bool, String> {
    let paths = paths()?;
    let Some(mut client) = connect(&paths)? else {
        return Ok(false);
    };
    client
        .call("daemon.shutdown", Value::Null)
        .map_err(|e| e.to_string())?;
    drop(client);
    let deadline = Instant::now() + WAIT;
    while connect(&paths)?.is_some() {
        if Instant::now() > deadline {
            return Err("Kiln's daemon didn't stop".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(true)
}

pub fn stop() -> Result<(), String> {
    if stop_quietly()? {
        // Only mention the web server when there's one to keep running.
        if crate::web::is_running().unwrap_or(false) {
            say!("Kiln's daemon stopped. The web server keeps running; `kiln web stop` stops it.");
        } else {
            say!("Kiln's daemon stopped.");
        }
    } else {
        say!("Kiln's daemon isn't running.");
    }
    Ok(())
}

/// `kiln daemon enable`: start at login (journaled), and now if it isn't running.
pub fn enable(yes: bool) -> Result<(), String> {
    let exe = exe()?;
    crate::apply::autostart_enable(&exe, yes)?;
    if autostart_on(&exe) {
        start()?;
    }
    Ok(())
}

/// `kiln daemon disable`: no longer start at login. A running daemon keeps running.
pub fn disable(yes: bool) -> Result<(), String> {
    crate::apply::autostart_disable(yes)
}

/// Whether this platform's login item starts the `kilnd` at `exe`.
fn autostart_on(exe: &std::path::Path) -> bool {
    kiln_platform::autostart::platform_item()
        .and_then(|item| item.read().ok().flatten())
        .is_some_and(|content| content == kiln_platform::autostart::content_for(exe))
}

/// What Kiln's login item starts: the app, the daemon, or nothing (ADR 0054 §6).
fn print_autostart() {
    let item =
        kiln_platform::autostart::platform_item().and_then(|item| item.read().ok().flatten());
    let kilnd = exe().ok();
    say!(
        "  Starts at login: {}",
        crate::app::login_item_says(item.as_deref(), kilnd.as_deref())
    );
}

pub fn status() -> Result<(), String> {
    let paths = paths()?;
    let Some(mut client) = connect(&paths)? else {
        say!("Kiln's daemon isn't running. `kiln daemon start` starts it.");
        print_autostart();
        return Ok(());
    };
    let call = |client: &mut Client, method: &str| {
        client.call(method, Value::Null).map_err(|e| e.to_string())
    };
    let status = call(&mut client, "daemon.status")?;
    let web = call(&mut client, "web.status")?;
    say!(
        "Kiln's daemon {} is running (PID {}, up {}).",
        status["version"].as_str().unwrap_or("?"),
        status["pid"],
        uptime(status["uptime_secs"].as_u64().unwrap_or_default())
    );
    for (label, key) in [
        ("Web server", "server"),
        ("PHP supervisor", "php_supervisor"),
    ] {
        let state = &web[key];
        match state["state"].as_str() {
            Some("running") => say!("  {label}: running (PID {})", state["pid"]),
            Some("exited") => say!("  {label}: exited on its own; the daemon restarts it"),
            _ => say!("  {label}: stopped"),
        }
    }
    let repairs = status["repairs"].as_array().map_or(0, Vec::len);
    if repairs > 0 {
        say!("  Restarts by the daemon: {repairs}");
    }
    // Services (ADR 0048): their supervisor, and how many instances answer.
    if let Ok(services) = call(&mut client, "services.status") {
        let instances = services["instances"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let wanted = instances.iter().filter(|i| i["enabled"] == true).count();
        let answering = instances.iter().filter(|i| i["answers"] == true).count();
        match services["supervisor"]["state"].as_str() {
            Some("running") => say!(
                "  Services: {answering} of {wanted} running (supervisor PID {})",
                services["supervisor"]["pid"]
            ),
            Some("exited") => {
                say!("  Services: their supervisor exited on its own; the daemon restarts it");
            }
            _ if instances.is_empty() => say!("  Services: none"),
            _ => say!("  Services: stopped"),
        }
    }
    let service_repairs = status["service_repairs"].as_array().map_or(0, Vec::len);
    if service_repairs > 0 {
        say!("  Services restarted by the daemon: {service_repairs}");
    }
    let ca = &status["ca"];
    if let Some(days) = ca["intermediate_days_left"].as_i64() {
        say!(
            "  Intermediate certificate: {days} day(s) left; renewed by the daemon when fewer than {} are left",
            ca["renew_before_days"]
        );
    }
    let renewals = ca["renewals"].as_array().map_or(0, Vec::len);
    if renewals > 0 {
        say!("  Renewals by the daemon: {renewals}");
    }
    print_autostart();
    say!("  Log: {}", paths.log.display());
    Ok(())
}

fn uptime(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs} s"),
        60..3600 => format!("{} min", secs / 60),
        _ => format!("{} h {} min", secs / 3600, secs % 3600 / 60),
    }
}

/// `kiln daemon call <method> [params]`: one raw API call, for scripts and debugging.
pub fn call(method: &str, params: Option<&str>) -> Result<(), String> {
    let params: Value = match params {
        Some(text) => serde_json::from_str(text).map_err(|e| format!("params aren't JSON: {e}"))?,
        None => Value::Null,
    };
    let paths = paths()?;
    let mut client = connect(&paths)?.ok_or_else(|| ClientError::NotRunning.to_string())?;
    let result = client.call(method, params).map_err(|e| e.to_string())?;
    say!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_reads_naturally() {
        assert_eq!(uptime(42), "42 s");
        assert_eq!(uptime(600), "10 min");
        assert_eq!(uptime(3 * 3600 + 5 * 60), "3 h 5 min");
    }
}
