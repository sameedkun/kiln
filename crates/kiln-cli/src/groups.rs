//! `kiln group <site> [<group>]` ([ADR 0074]): sites that belong together (an app, its admin,
//! its blog) shown together. A label Kiln keeps, nothing more: no file of the project's or the
//! system's changes, so it isn't journaled.
//!
//! [ADR 0074]: ../../../docs/adr/0074-site-groups.md

use std::collections::BTreeMap;

use kiln_state::StateStore;

/// The setting holding each site's group, as JSON: `{"shop.test": "Acme"}`.
pub const KEY: &str = "sites.groups";

/// A group's name: 1 to 40 letters, digits, spaces, `-`, `_` or `.`, not starting or ending
/// with a space.
pub fn check_name(name: &str) -> Result<(), String> {
    let ok = (1..=40).contains(&name.chars().count())
        && name.trim() == name
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{name:?} can't be a group's name: 1 to 40 letters, digits, spaces, '-', '_' or '.'"
        ))
    }
}

/// Every site's group, by host.
#[must_use]
pub fn groups(store: &StateStore) -> BTreeMap<String, String> {
    store
        .setting(KEY)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// `kiln group <site> [<group>]`: puts a site in a group, or takes it out of its group.
pub fn set(site: &str, group: Option<&str>) -> Result<(), String> {
    if let Some(name) = group {
        check_name(name)?;
    }
    let mut store = crate::web::open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let host = crate::web::host_name_under(site, &tld)?.as_str().to_owned();
    let served = crate::web::resolved(&store)?
        .sites
        .iter()
        .any(|s| s.host.as_str() == host)
        || crate::web::proxy_specs(&store)?
            .iter()
            .any(|p| p.host.as_str() == host);
    if !served {
        return Err(format!("Kiln doesn't serve {host}"));
    }
    let mut all = groups(&store);
    match group {
        Some(name) => {
            all.insert(host.clone(), name.to_owned());
            say!("{host} is in the group {name}.");
        }
        None => {
            if all.remove(&host).is_some() {
                say!("{host} is in no group now.");
            } else {
                say!("{host} wasn't in a group.");
            }
        }
    }
    let text = serde_json::to_string(&all).map_err(|e| e.to_string())?;
    store.set_setting(KEY, &text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_names_are_short_plain_labels() {
        assert!(check_name("Acme Corp").is_ok());
        assert!(check_name("orbit-labs_2.0").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name(" Acme").is_err());
        assert!(check_name("a/b").is_err());
        assert!(check_name(&"x".repeat(41)).is_err());
    }
}
