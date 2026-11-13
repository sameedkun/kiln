//! The app's own settings ([ADR 0054] §2): a small JSON file in Kiln's folder, so uninstall
//! removes it with everything else. Kiln's settings stay in Kiln's state; these are only
//! about the app's window.
//!
//! [ADR 0054]: ../../../../docs/adr/0054-desktop-app-shell.md

use std::path::Path;

use serde::{Deserialize, Serialize};

/// What happens when the app starts without `--background`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartIn {
    #[default]
    Window,
    Tray,
}

/// Where the main window was, to open it there again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub start_in: StartIn,
    /// The first close explains that Kiln keeps running in the tray; once.
    pub close_notice_shown: bool,
    /// "Not now" on first-run onboarding (ADR 0057 §3): it doesn't open on its own again.
    pub onboarding_dismissed: bool,
    pub window: Option<Place>,
}

/// The settings at `path`; the defaults if there are none or they can't be read.
#[must_use]
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes the settings. A torn write only costs the defaults next time.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// Whether the app stays in the tray when it starts: always from the login item
/// (`--background`), else as the user chose.
#[must_use]
pub fn starts_in_tray(args: &[String], settings: &Settings) -> bool {
    args.iter().any(|a| a == "--background") || settings.start_in == StartIn::Tray
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_broken_files_give_the_defaults() {
        let dir = std::env::temp_dir().join(format!("kiln-app-settings-{}", std::process::id()));
        let path = dir.join("app.json");
        assert_eq!(load(&path), Settings::default());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_and_ignore_unknown_fields() {
        let dir = std::env::temp_dir().join(format!("kiln-app-round-{}", std::process::id()));
        let path = dir.join("app.json");
        let settings = Settings {
            start_in: StartIn::Tray,
            close_notice_shown: true,
            onboarding_dismissed: false,
            window: Some(Place {
                x: 10,
                y: 20,
                width: 1120,
                height: 720,
                maximized: false,
            }),
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), settings);
        std::fs::write(&path, r#"{"start_in":"tray","later_setting":1}"#).unwrap();
        assert_eq!(load(&path).start_in, StartIn::Tray);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_login_item_always_starts_in_the_tray() {
        let window = Settings::default();
        let tray = Settings {
            start_in: StartIn::Tray,
            ..Settings::default()
        };
        assert!(!starts_in_tray(&[], &window));
        assert!(starts_in_tray(&["--background".into()], &window));
        assert!(starts_in_tray(&[], &tray));
    }
}
