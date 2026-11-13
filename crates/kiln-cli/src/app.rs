//! `kiln app enable|disable`: the desktop app's login item ([ADR 0054] §6), and stopping the
//! app for uninstall.
//!
//! The login item is Kiln's one `Kiln` Run value, the same journaled item `kiln daemon
//! enable` writes. Here it starts the app in the tray, and the app makes sure `kilnd` runs.
//!
//! [ADR 0054]: ../../../docs/adr/0054-desktop-app-shell.md

use std::path::{Path, PathBuf};
use std::time::Duration;

use kiln_sys::background::PidFile;

/// The app's program file, next to `kiln`.
pub const APP_EXE: &str = if cfg!(windows) {
    "kiln-desktop.exe"
} else {
    "kiln-desktop"
};

/// The app next to this `kiln`: never a path from a caller.
fn exe() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = me.with_file_name(APP_EXE);
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!(
            "{} is missing: Kiln's desktop app goes next to kiln (build it in apps/desktop)",
            exe.display()
        ))
    }
}

/// The login item's content for the app: in the tray, as at every sign-in.
#[must_use]
pub fn content(exe: &Path) -> String {
    format!("\"{}\" --background", exe.display())
}

/// The PID file the app writes while it runs, in Kiln's local folder.
fn pid_file(local: &Path, exe: PathBuf) -> PidFile {
    PidFile::new(local.join("app").join("kiln-desktop.pid"), exe)
}

pub fn enable(yes: bool) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Kiln's desktop app is Windows-only for now".into());
    }
    let exe = exe()?;
    crate::apply::login_item_enable(
        &content(&exe),
        &crate::apply::LoginWords {
            already: "Kiln's app already starts when you sign in.",
            question: "Start Kiln's app in the tray when you sign in? It starts Kiln's daemon too. (`kiln app disable` turns it off)",
            done: "Kiln's app now starts in the tray when you sign in.",
        },
        yes,
    )
    .map(|_| ())
}

/// Nothing of Kiln's starts at sign-in any more: the app's item and the daemon's are undone.
pub fn disable(yes: bool) -> Result<(), String> {
    crate::apply::autostart_disable(yes)
}

/// Stops the app if it runs (for uninstall, before the daemon: the app would start `kilnd`
/// again). Returns its PID if it was running.
pub fn stop_quietly() -> Result<Option<u32>, String> {
    let Some(local) = kiln_state::default_local_dir() else {
        return Ok(None);
    };
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    pid_file(&local, me.with_file_name(APP_EXE))
        .terminate(Duration::from_secs(5))
        .map_err(|e| format!("cannot stop Kiln's app: {e}"))
}

/// What Kiln's login item starts now: `app`, `daemon`, `other` or `none` (ADR 0057).
#[must_use]
pub fn login_item_kind() -> &'static str {
    let item =
        kiln_platform::autostart::platform_item().and_then(|item| item.read().ok().flatten());
    let Some(item) = item else { return "none" };
    let me = std::env::current_exe().ok();
    let app = me.as_ref().map(|m| content(&m.with_file_name(APP_EXE)));
    let daemon = me
        .as_ref()
        .map(|m| kiln_platform::autostart::content_for(&m.with_file_name(kiln_daemon::EXE_NAME)));
    if Some(&item) == app.as_ref() {
        "app"
    } else if Some(&item) == daemon.as_ref() {
        "daemon"
    } else {
        "other"
    }
}

/// What the login item starts, in words for `kiln daemon status`.
#[must_use]
pub fn login_item_says(item: Option<&str>, kilnd: Option<&Path>) -> String {
    let me = std::env::current_exe().ok();
    let app = me.as_ref().map(|m| content(&m.with_file_name(APP_EXE)));
    let daemon = kilnd.map(kiln_platform::autostart::content_for);
    match item {
        None => "no (`kiln app enable` or `kiln daemon enable` turns it on)".into(),
        Some(c) if Some(c) == app.as_deref() => {
            "Kiln's app, in the tray, which starts the daemon (`kiln app disable` turns it off)"
                .into()
        }
        Some(c) if Some(c) == daemon.as_deref() => {
            "Kiln's daemon (`kiln daemon disable` turns it off)".into()
        }
        Some(c) => format!("something else: {c}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_starts_in_the_tray_at_sign_in() {
        let exe = Path::new("C:\\Kiln\\kiln-desktop.exe");
        assert_eq!(content(exe), "\"C:\\Kiln\\kiln-desktop.exe\" --background");
    }

    #[test]
    fn the_login_item_is_named_for_what_it_starts() {
        assert!(login_item_says(None, None).starts_with("no"));
        let me = std::env::current_exe().unwrap();
        let app = content(&me.with_file_name(APP_EXE));
        assert!(login_item_says(Some(&app), None).starts_with("Kiln's app"));
        let kilnd = me.with_file_name("kilnd.exe");
        let daemon = kiln_platform::autostart::content_for(&kilnd);
        assert!(login_item_says(Some(&daemon), Some(&kilnd)).starts_with("Kiln's daemon"));
        assert!(login_item_says(Some("\"x.exe\""), Some(&kilnd)).starts_with("something else"));
    }
}
