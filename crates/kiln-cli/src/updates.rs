//! `kiln updates` ([ADR 0064]): PHP patch releases for the versions Kiln has installed, and,
//! only when the user turned it on, a newer Kiln on GitHub.
//!
//! [ADR 0064]: ../../../docs/adr/0064-updates.md

use serde_json::json;

/// `on` when Kiln may ask GitHub for its newest release; off (unset) by default.
pub const CHECK_KILN_KEY: &str = "updates.check_kiln";

/// Kiln's releases on GitHub. Only read when [`CHECK_KILN_KEY`] is on.
const LATEST_URL: &str = "https://api.github.com/repos/sameedkun/kiln/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/sameedkun/kiln/releases";

/// A version's numbers, for comparing: `v1.2.10` and `1.2.10` are `[1, 2, 10]`.
#[must_use]
pub fn numbers(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map_while(|p| p.parse().ok())
        .collect()
}

/// Whether `latest` is newer than `current`.
#[must_use]
pub fn is_newer(latest: &str, current: &str) -> bool {
    numbers(latest) > numbers(current)
}

/// Whether Kiln may check GitHub.
pub fn check_kiln(store: &kiln_state::StateStore) -> bool {
    store
        .setting(CHECK_KILN_KEY)
        .ok()
        .flatten()
        .is_some_and(|v| v == "on")
}

/// The newest PHP patch for each installed version of Kiln's own: (installed, newest).
fn php_updates() -> Result<Vec<(String, String)>, String> {
    if !cfg!(windows) {
        return Ok(Vec::new());
    }
    let store = crate::web::open_store()?;
    let installed = store.php_versions().map_err(|e| e.to_string())?;
    let releases = crate::php::releases(&crate::php::fetcher()?)?;
    Ok(installed
        .iter()
        .filter(|r| r.archive != kiln_php::linux::SYSTEM_ARCHIVE)
        .filter_map(|r| {
            let minor: String = r.version.split('.').take(2).collect::<Vec<_>>().join(".");
            let newest = releases.iter().find(|n| n.minor == minor)?;
            Some((r.version.clone(), newest.version.clone()))
        })
        .collect())
}

/// Kiln's newest release on GitHub: `Ok(None)` when there is none yet.
fn kiln_latest() -> Result<Option<String>, String> {
    let text = match crate::php::fetcher()?.get_text(LATEST_URL, 1 << 20) {
        Ok(text) => text,
        Err(e) if e.to_string().contains("404") => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("GitHub's answer isn't JSON: {e}"))?;
    Ok(value["tag_name"].as_str().map(str::to_owned))
}

/// `kiln updates`: what can be updated. PHP's index is read every time; GitHub only when
/// checking for Kiln is on.
pub fn updates() -> Result<(), String> {
    let store = crate::web::open_store()?;
    let php = php_updates()?;
    let current = env!("CARGO_PKG_VERSION");
    let checking = check_kiln(&store);
    let latest = if checking { kiln_latest() } else { Ok(None) };
    if crate::out::json() {
        let (latest, error) = match &latest {
            Ok(v) => (v.clone(), None),
            Err(e) => (None, Some(e.clone())),
        };
        crate::out::data(json!({
            "php": php.iter().map(|(installed, newest)| json!({
                "version": installed,
                "newest": newest,
                "update": is_newer(newest, installed),
            })).collect::<Vec<_>>(),
            "kiln": {
                "version": current,
                "checking": checking,
                "latest": latest,
                "update": latest.as_deref().is_some_and(|l| is_newer(l, current)),
                "error": error,
                "page": RELEASES_PAGE,
            },
        }));
        return Ok(());
    }
    let mut any = false;
    for (installed, newest) in &php {
        if is_newer(newest, installed) {
            any = true;
            let minor: String = installed.split('.').take(2).collect::<Vec<_>>().join(".");
            say!("PHP {installed} → {newest}: `kiln php update {minor}`.");
        }
    }
    if !any {
        say!("Kiln's PHP is up to date.");
    }
    match (checking, latest) {
        (false, _) => say!(
            "Kiln itself isn't checked: `kiln updates check-kiln on` lets Kiln ask GitHub for its newest release."
        ),
        (true, Ok(Some(latest))) if is_newer(&latest, current) => {
            say!("Kiln {latest} is out (this is {current}): {RELEASES_PAGE}");
        }
        (true, Ok(_)) => say!("Kiln {current} is the newest."),
        (true, Err(e)) => say!("Couldn't ask GitHub for Kiln's newest release: {e}"),
    }
    Ok(())
}

/// `kiln updates check-kiln on|off`.
pub fn set_check_kiln(on: bool) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    store
        .set_setting(CHECK_KILN_KEY, if on { "on" } else { "off" })
        .map_err(|e| e.to_string())?;
    if on {
        say!(
            "Kiln asks GitHub for its newest release when you check for updates. It sends nothing about you or this PC."
        );
    } else {
        say!("Kiln doesn't ask GitHub for new releases.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(is_newer("8.4.28", "8.4.27"));
        assert!(is_newer("8.4.100", "8.4.99"));
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(!is_newer("8.4.27", "8.4.27"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert_eq!(numbers("1.2.3-rc1"), [1, 2, 3]);
    }
}
