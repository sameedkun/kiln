//! Kiln's desktop app ([ADR 0050], [ADR 0054]): a tray app over `kilnd`'s API.
//!
//! - **The main window:** frameless, with Kiln's own title bar. Closing it hides it; the
//!   first time, the page explains that Kiln keeps running in the tray.
//! - **The tray icon:** left click opens the main window, right click the popup next to it.
//! - **Quit** ends the app only: `kilnd`, the web server and services keep running.
//!
//! The app holds no logic of Kiln's: every read and change is a call to `kilnd`
//! ([`bridge`]), which runs the CLI.
//!
//! [ADR 0050]: ../../../../docs/adr/0050-desktop-app.md
//! [ADR 0054]: ../../../../docs/adr/0054-desktop-app-shell.md

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod placement;
mod settings;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, RunEvent, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_opener::OpenerExt;

use crate::bridge::{Bridge, DaemonState};
use crate::settings::{Place, Settings, StartIn};

const MAIN: &str = "main";
const TRAY: &str = "tray";
const TRAY_ICON: &str = "kiln";
const KILN_EXE: &str = if cfg!(windows) { "kiln.exe" } else { "kiln" };
/// A tray click this soon after the popup hid is the click that hid it: don't reopen.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// What the app keeps while it runs.
#[derive(Debug)]
struct Shell {
    bridge: Arc<Bridge>,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    popup_hidden_at: Mutex<Option<Instant>>,
}

impl Shell {
    fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, change: impl FnOnce(&mut Settings)) {
        let mut settings = self.settings.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut settings);
        let _ = settings::save(&self.settings_path, &settings);
    }
}

// ---------- commands the pages call ----------

/// A call to `kilnd` (ADR 0054 §3), off the main thread, on a connection of its own.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
async fn api(
    method: String,
    params: Option<Value>,
    shell: State<'_, Shell>,
) -> Result<Value, String> {
    if method == "daemon.shutdown" {
        return Err("the app doesn't stop Kiln's daemon; `kiln daemon stop` does".into());
    }
    let socket = shell.bridge.socket.clone();
    tauri::async_runtime::spawn_blocking(move || {
        bridge::call(&socket, &method, params.unwrap_or(Value::Null))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn daemon_state(shell: State<'_, Shell>) -> DaemonState {
    shell.bridge.state()
}

/// Opens a site in the browser by its host name, at the URL `kilnd` gives for it: the page
/// never hands the app a URL to open.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
async fn open_site(host: String, app: AppHandle, shell: State<'_, Shell>) -> Result<(), String> {
    let socket = shell.bridge.socket.clone();
    let sites = tauri::async_runtime::spawn_blocking(move || {
        bridge::call(&socket, "sites.list", Value::Null)
    })
    .await
    .map_err(|e| e.to_string())??;
    let url = site_url(&sites, &host).ok_or_else(|| format!("no site called {host}"))?;
    // A name Windows can't resolve would open a dead page: the page offers `hosts sync`.
    if !in_hosts(&sites, &host) {
        return Err(format!("{NOT_IN_HOSTS}{host} isn't in the hosts file yet"));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Opens a site's folder by its host name, the folder `kilnd` gives for it (ADR 0055 §2).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
async fn open_folder(host: String, app: AppHandle, shell: State<'_, Shell>) -> Result<(), String> {
    let socket = shell.bridge.socket.clone();
    let sites = tauri::async_runtime::spawn_blocking(move || {
        bridge::call(&socket, "sites.list", Value::Null)
    })
    .await
    .map_err(|e| e.to_string())??;
    let folder = site_folder(&sites, &host).ok_or_else(|| format!("no site called {host}"))?;
    app.opener()
        .open_path(folder, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Kiln's releases page, for "a new Kiln is out" (ADR 0064): a fixed address, never one the
/// page gives.
const RELEASES_PAGE: &str = "https://github.com/sameedkun/kiln/releases";

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn open_releases(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(RELEASES_PAGE, None::<&str>)
        .map_err(|e| e.to_string())
}

/// The folder `sites.list` gives for `host`.
fn site_folder(sites: &Value, host: &str) -> Option<String> {
    sites["sites"]
        .as_array()?
        .iter()
        .find(|site| site["host"] == host)
        .and_then(|site| site["project"].as_str())
        .map(str::to_owned)
}

/// Windows' folder dialog, for linking or parking a folder: the folder chosen, or nothing.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
async fn pick_folder(title: String, app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_folder()
    })
    .await
    .ok()
    .flatten()?;
    chosen.into_path().ok().map(|p| p.display().to_string())
}

/// Windows' open dialog for a `.sql` file, to restore (ADR 0056 §3).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
async fn pick_sql(title: String, app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(title)
            .add_filter("SQL dump", &["sql"])
            .blocking_pick_file()
    })
    .await
    .ok()
    .flatten()?;
    chosen.into_path().ok().map(|p| p.display().to_string())
}

/// Windows' save dialog, for where a dump goes (ADR 0056 §3).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
async fn pick_save(title: String, name: String, app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(title)
            .set_file_name(name)
            .add_filter("SQL dump", &["sql"])
            .blocking_save_file()
    })
    .await
    .ok()
    .flatten()?;
    chosen.into_path().ok().map(|p| p.display().to_string())
}

/// How an `open_site` error starts when the site's name isn't in the hosts file.
const NOT_IN_HOSTS: &str = "not-in-hosts: ";

/// Whether `sites.list` says `host`'s name is in the hosts file.
fn in_hosts(sites: &Value, host: &str) -> bool {
    ["sites", "proxies"]
        .iter()
        .filter_map(|list| sites[list].as_array())
        .flatten()
        .find(|site| site["host"] == host)
        .is_some_and(|site| site["in_hosts"] != false)
}

/// The URL `sites.list` gives for `host`, among its sites and proxies.
fn site_url(sites: &Value, host: &str) -> Option<String> {
    ["sites", "proxies"]
        .iter()
        .filter_map(|list| sites[list].as_array())
        .flatten()
        .find(|site| site["host"] == host)
        .and_then(|site| site["url"].as_str())
        .map(str::to_owned)
}

/// The title bar's buttons.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn window_action(action: String, window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    let result = match action.as_str() {
        "minimise" => window.minimize(),
        "maximise" => {
            if window.is_maximized().unwrap_or(false) {
                window.unmaximize()
            } else {
                window.maximize()
            }
        }
        "close" => {
            close_main(&app);
            Ok(())
        }
        other => return Err(format!("no window action {other:?}")),
    };
    result.map_err(|e| e.to_string())
}

/// The first close's notice was read: from now on, closing just hides the window.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn close_notice_read(app: AppHandle, shell: State<'_, Shell>) {
    shell.update(|s| s.close_notice_shown = true);
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.hide();
    }
}

/// Esc in the popup.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn hide_tray_popup(app: AppHandle) {
    hide_popup(&app);
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn show_main(app: AppHandle) {
    open_main(&app);
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn settings_get(shell: State<'_, Shell>) -> Settings {
    shell.settings()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn set_start_in(start_in: StartIn, shell: State<'_, Shell>) {
    shell.update(|s| s.start_in = start_in);
}

/// Remembers that onboarding was dismissed, or done (ADR 0057 §3).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn set_onboarding_dismissed(dismissed: bool, shell: State<'_, Shell>) {
    shell.update(|s| s.onboarding_dismissed = dismissed);
}

/// The tray icon's look and tooltip, from what the pages know: `running`, `attention` or
/// `stopped`.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn set_tray(state: String, tooltip: String, app: AppHandle) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ICON) else {
        return Ok(());
    };
    let icon = match state.as_str() {
        "attention" => &include_bytes!("../icons/tray-attention.png")[..],
        "stopped" => &include_bytes!("../icons/tray-stopped.png")[..],
        _ => &include_bytes!("../icons/tray.png")[..],
    };
    tray.set_icon(Some(Image::from_bytes(icon).map_err(|e| e.to_string())?))
        .and_then(|()| tray.set_tooltip(Some(tooltip)))
        .map_err(|e| e.to_string())
}

/// The popup fits its content's height (in the page's pixels).
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn fit_popup(height: f64, app: AppHandle) -> Result<(), String> {
    let Some(popup) = app.get_webview_window(TRAY) else {
        return Ok(());
    };
    popup
        .set_size(tauri::LogicalSize::new(
            POPUP_WIDTH,
            height.clamp(200.0, 720.0),
        ))
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands commands their arguments by value
fn quit(app: AppHandle) {
    leave(&app);
}

/// What Kiln's login item starts (ADR 0054 §6): `app`, `daemon`, `other` or `none`.
/// Changing it is `kiln app enable|disable`, a job with a preview.
#[tauri::command]
fn login_item() -> &'static str {
    #[cfg(windows)]
    {
        let item = kiln_sys::win::read_user_reg_string(kiln_sys::win::USER_RUN_KEY, "Kiln")
            .ok()
            .flatten()
            .map(|v| v.value);
        let me = std::env::current_exe().ok();
        login_item_kind(item.as_deref(), me.as_deref())
    }
    #[cfg(not(windows))]
    {
        "none"
    }
}

/// What a login item's content starts, against this app's path.
fn login_item_kind(item: Option<&str>, me: Option<&std::path::Path>) -> &'static str {
    let Some(item) = item else { return "none" };
    let app = me.map(|me| format!("\"{}\" --background", me.display()));
    if Some(item) == app.as_deref() {
        "app"
    } else if item.to_ascii_lowercase().contains("kilnd") {
        "daemon"
    } else {
        "other"
    }
}

/// Ends the app: its place remembered, its PID file gone. Kiln itself keeps running.
fn leave(app: &AppHandle) {
    remember_place(app);
    if let Some(pid) = app.try_state::<Pid>() {
        let _ = pid.0.remove();
    }
    app.exit(0);
}

/// The PID file uninstall uses to stop the app before the daemon (ADR 0054 §2).
#[derive(Debug)]
struct Pid(kiln_sys::background::PidFile);

// ---------- windows ----------

const POPUP_WIDTH: f64 = 340.0;

fn open_main(app: &AppHandle) {
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
    hide_popup(app);
}

/// Closing the main window hides it. The first time, the page says why first.
fn close_main(app: &AppHandle) {
    remember_place(app);
    let shell = app.state::<Shell>();
    let Some(main) = app.get_webview_window(MAIN) else {
        return;
    };
    if shell.settings().close_notice_shown {
        let _ = main.hide();
    } else {
        let _ = main.emit("kiln-first-close", ());
    }
}

/// Keeps where the main window is, to open it there next time.
fn remember_place(app: &AppHandle) {
    let Some(main) = app.get_webview_window(MAIN) else {
        return;
    };
    if main.is_minimized().unwrap_or(false) || !main.is_visible().unwrap_or(false) {
        return;
    }
    if let (Ok(position), Ok(size)) = (main.outer_position(), main.outer_size()) {
        let maximized = main.is_maximized().unwrap_or(false);
        app.state::<Shell>().update(|s| {
            // A maximised window keeps the place it had before.
            let place = if maximized {
                s.window.map_or(
                    Place {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                        maximized: true,
                    },
                    |before| Place {
                        maximized: true,
                        ..before
                    },
                )
            } else {
                Place {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                    maximized: false,
                }
            };
            s.window = Some(place);
        });
    }
}

fn hide_popup(app: &AppHandle) {
    if let Some(popup) = app.get_webview_window(TRAY)
        && popup.is_visible().unwrap_or(false)
    {
        let _ = popup.hide();
        *app.state::<Shell>()
            .popup_hidden_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    }
}

/// Right click on the tray icon: the popup against it, or away if it was open.
fn toggle_popup(app: &AppHandle, icon: tauri::Rect) {
    let Some(popup) = app.get_webview_window(TRAY) else {
        return;
    };
    if popup.is_visible().unwrap_or(false) {
        hide_popup(app);
        return;
    }
    let just_hidden = app
        .state::<Shell>()
        .popup_hidden_at
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some_and(|at| at.elapsed() < REOPEN_GUARD);
    if just_hidden {
        return;
    }
    let position = icon.position.to_physical::<i32>(1.0);
    let size = icon.size.to_physical::<u32>(1.0);
    let icon = placement::Rect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let monitor = app
        .monitor_from_point(f64::from(icon.x), f64::from(icon.y))
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(popup_size)) = (monitor, popup.outer_size()) else {
        return;
    };
    let area = monitor.work_area();
    let work = placement::Rect {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    };
    let (x, y) = placement::popup_position(icon, work, (popup_size.width, popup_size.height));
    let _ = popup.set_position(PhysicalPosition::new(x, y));
    let _ = popup.show();
    let _ = popup.set_focus();
    let _ = popup.emit("kiln-popup-shown", ());
}

/// `WebView2`'s arguments when a debug build is driven end to end (P4-13): Tauri's own, plus a
/// debugging port from `KILN_DEBUG_PORT`. Release builds never open one.
fn browser_args() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let port: u16 = std::env::var("KILN_DEBUG_PORT").ok()?.parse().ok()?;
    Some(format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
    ))
}

fn build_windows(
    app: &tauri::App,
    webview_dir: &std::path::Path,
    settings: &Settings,
    in_tray: bool,
) -> tauri::Result<bool> {
    let args = browser_args();
    let mut main = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()));
    if let Some(args) = &args {
        main = main.additional_browser_args(args);
    }
    let main = main
        .title("Kiln")
        .decorations(false)
        .inner_size(1120.0, 720.0)
        .min_inner_size(880.0, 560.0)
        .data_directory(webview_dir.to_path_buf())
        .visible(false)
        .build()?;
    if let Some(place) = settings.window {
        // Only onto a monitor that is still there.
        let on_screen = main
            .monitor_from_point(f64::from(place.x) + 40.0, f64::from(place.y) + 20.0)
            .ok()
            .flatten()
            .is_some();
        if on_screen {
            let _ = main.set_position(PhysicalPosition::new(place.x, place.y));
            let _ = main.set_size(PhysicalSize::new(place.width, place.height));
        }
        if place.maximized {
            let _ = main.maximize();
        }
    }
    if !in_tray {
        let _ = main.show();
        let _ = main.set_focus();
    }
    // Both windows share one WebView2 environment, so they get the same arguments.
    let mut popup = WebviewWindowBuilder::new(app, TRAY, WebviewUrl::App("index.html#tray".into()));
    if let Some(args) = &args {
        popup = popup.additional_browser_args(args);
    }
    let popup = popup
        .title("Kiln")
        .decorations(false)
        .inner_size(POPUP_WIDTH, 520.0)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .data_directory(webview_dir.to_path_buf())
        .visible(false)
        .build();
    Ok(popup.is_ok())
}

fn build_tray(app: &tauri::App, popup: bool) -> tauri::Result<()> {
    let mut tray = TrayIconBuilder::with_id(TRAY_ICON)
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("Kiln")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                match button {
                    MouseButton::Left => open_main(tray.app_handle()),
                    MouseButton::Right => toggle_popup(tray.app_handle(), rect),
                    MouseButton::Middle => {}
                }
            }
        });
    if !popup {
        // ADR 0051 §6: without the popup, a native menu with the same way in and out.
        let open = MenuItem::with_id(app, "open", "Open Kiln", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
        tray = tray
            .menu(&Menu::with_items(app, &[&open, &quit])?)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "open" => open_main(app),
                "quit" => leave(app),
                _ => {}
            });
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // First: a second start shows the window already open.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            open_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            api,
            daemon_state,
            open_site,
            open_folder,
            open_releases,
            pick_folder,
            pick_sql,
            pick_save,
            window_action,
            close_notice_read,
            show_main,
            hide_tray_popup,
            settings_get,
            set_start_in,
            set_onboarding_dismissed,
            set_tray,
            fit_popup,
            quit,
            login_item,
        ])
        .setup(|app| {
            let paths = kiln_daemon::Paths::for_user().ok_or("cannot find Kiln's folders")?;
            let kiln = std::env::current_exe()?.with_file_name(KILN_EXE);
            let settings_path = paths.state.join("app.json");
            let settings = settings::load(&settings_path);
            let args: Vec<String> = std::env::args().collect();
            let in_tray = settings::starts_in_tray(&args, &settings);
            // WebView2's profile inside Kiln's folder, so uninstall removes it (ADR 0054 §2).
            let webview_dir = paths.local.join("app").join("webview");
            let bridge = Arc::new(Bridge::new(paths.socket.clone(), kiln));
            let pid = kiln_sys::background::PidFile::new(
                paths.local.join("app").join("kiln-desktop.pid"),
                std::env::current_exe()?,
            );
            if let Some(dir) = pid.path().parent() {
                std::fs::create_dir_all(dir)?;
            }
            pid.write(std::process::id())?;
            app.manage(Pid(pid));
            app.manage(Shell {
                bridge: Arc::clone(&bridge),
                settings_path,
                settings: Mutex::new(settings.clone()),
                popup_hidden_at: Mutex::new(None),
            });
            let popup = build_windows(app, &webview_dir, &settings, in_tray)?;
            build_tray(app, popup)?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                bridge::follow(
                    &bridge,
                    &|state| {
                        let _ = handle.emit("kiln-daemon", state);
                    },
                    &|| {
                        let _ = handle.emit("kiln-reconnected", ());
                    },
                    &|event| {
                        let _ = handle.emit("kiln-event", event);
                    },
                );
            });
            Ok(())
        })
        .on_window_event(|window, event| match (window.label(), event) {
            (MAIN, WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                close_main(window.app_handle());
            }
            (TRAY, WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                hide_popup(window.app_handle());
            }
            (TRAY, WindowEvent::Focused(false)) => hide_popup(window.app_handle()),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("Kiln's app couldn't start")
        .run(|_app, event| {
            // Hidden windows aren't closed ones, but never end because none is showing.
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_login_item_is_named_for_what_it_starts() {
        let me = std::path::Path::new(r"C:\Kiln\kiln-desktop.exe");
        assert_eq!(login_item_kind(None, Some(me)), "none");
        assert_eq!(
            login_item_kind(Some(r#""C:\Kiln\kiln-desktop.exe" --background"#), Some(me)),
            "app"
        );
        assert_eq!(
            login_item_kind(Some(r#""C:\Kiln\kilnd.exe""#), Some(me)),
            "daemon"
        );
        assert_eq!(
            login_item_kind(Some(r#""C:\Other\thing.exe""#), Some(me)),
            "other"
        );
    }

    #[test]
    fn a_site_not_in_the_hosts_file_says_so() {
        let sites = json!({
            "sites": [{"host": "shop.test", "in_hosts": true}, {"host": "new.test", "in_hosts": false}],
            "proxies": [{"host": "mailpit.test", "in_hosts": false}],
        });
        assert!(in_hosts(&sites, "shop.test"));
        assert!(!in_hosts(&sites, "new.test"));
        assert!(!in_hosts(&sites, "mailpit.test"));
    }

    #[test]
    fn a_sites_folder_is_the_one_kilnd_gives() {
        let sites = json!({
            "sites": [{"host": "shop.test", "project": r"D:\Code\shop"}],
            "proxies": [{"host": "api.test", "url": "http://api.test/"}],
        });
        assert_eq!(
            site_folder(&sites, "shop.test").as_deref(),
            Some(r"D:\Code\shop")
        );
        assert_eq!(site_folder(&sites, "api.test"), None);
        assert_eq!(site_folder(&sites, r"C:\Windows"), None);
    }

    #[test]
    fn a_site_opens_at_the_url_kilnd_gives_and_nothing_else() {
        let sites = json!({
            "sites": [{"host": "shop.test", "url": "https://shop.test:8443/"}],
            "proxies": [{"host": "api.test", "url": "http://api.test:8080/"}],
        });
        assert_eq!(
            site_url(&sites, "shop.test").as_deref(),
            Some("https://shop.test:8443/")
        );
        assert_eq!(
            site_url(&sites, "api.test").as_deref(),
            Some("http://api.test:8080/")
        );
        assert_eq!(site_url(&sites, "https://evil.example"), None);
    }
}
