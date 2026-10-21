use std::path::PathBuf;

/// Database file name inside the state directory.
pub const DB_FILE: &str = "state.db";

/// The Phase 0 JSON Lines journal. If it is found next to the database, it is imported once
/// and renamed to `journal.jsonl.imported`.
pub const LEGACY_JOURNAL_FILE: &str = "journal.jsonl";

/// Per-user state directory (architecture §8):
/// - Windows: `%APPDATA%\Kiln`
/// - macOS: `~/Library/Application Support/Kiln`
/// - other Unix: `$XDG_STATE_HOME/kiln`, else `~/.local/state/kiln`
///
/// Returns `None` if the relevant environment variables are missing.
#[must_use]
pub fn default_state_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        env("APPDATA").map(|p| p.join("Kiln"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|p| p.join("Library/Application Support/Kiln"))
    } else {
        env("XDG_STATE_HOME")
            .map(|p| p.join("kiln"))
            .or_else(|| env("HOME").map(|p| p.join(".local/state/kiln")))
    }
}

/// Per-user directory for machine-specific data that shouldn't roam: binaries, web server
/// config, logs and caches ([ADR 0011]).
/// - Windows: `%LOCALAPPDATA%\Kiln`
/// - macOS: `~/Library/Application Support/Kiln` (same as the state directory)
/// - other Unix: `$XDG_DATA_HOME/kiln`, else `~/.local/share/kiln`
///
/// Returns `None` if the relevant environment variables are missing.
///
/// [ADR 0011]: ../../../docs/adr/0011-caddy-integration.md
#[must_use]
pub fn default_local_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        env("LOCALAPPDATA").map(|p| p.join("Kiln"))
    } else if cfg!(target_os = "macos") {
        default_state_dir()
    } else {
        env("XDG_DATA_HOME")
            .map(|p| p.join("kiln"))
            .or_else(|| env("HOME").map(|p| p.join(".local/share/kiln")))
    }
}
