//! Service updates ([ADR 0090]): an instance on an older patch than the one Kiln pins for its
//! series is updated in place (its data stays, only its program changes), and the previous
//! version is kept to roll back to.
//!
//! [ADR 0090]: ../../../docs/adr/0090-service-updates.md

use kiln_services::ServiceProvider;
use kiln_state::{ServiceRecord, StateStore};

/// The setting naming an instance's previous version, for `kiln service rollback`.
fn previous_key(name: &str) -> String {
    format!("services.previous.{name}")
}

/// A version's numbers: `8.4.13` → `[8, 4, 13]`.
fn numbers(version: &str) -> Vec<u64> {
    version
        .split(['.', '-'])
        .map_while(|p| p.parse().ok())
        .collect()
}

/// The release to update `record` to: the one Kiln pins for its series, when newer.
fn newer(provider: &dyn ServiceProvider, record: &ServiceRecord) -> Option<&'static str> {
    if provider.runs_in_project() {
        return None; // its program is the project's (queue, vite…)
    }
    let series = provider.series(&record.version);
    let release = kiln_services::release_for_this_platform(provider, Some(&series))?;
    (numbers(release.version) > numbers(&record.version)).then_some(release.version)
}

/// Each instance with its version and the newer one, if any.
fn updates(store: &StateStore) -> Result<Vec<(ServiceRecord, Option<&'static str>)>, String> {
    Ok(store
        .services()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|record| {
            let newer = kiln_services::provider(&record.kind).and_then(|p| newer(p, &record));
            (record, newer)
        })
        .collect())
}

/// `kiln service updates`: which instances have a newer patch.
pub fn list() -> Result<(), String> {
    let store = crate::web::open_store()?;
    let all = updates(&store)?;
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "instances": all.iter().map(|(r, newer)| serde_json::json!({
                "name": r.name,
                "kind": r.kind,
                "version": r.version,
                "newer": newer,
                "previous": store.setting(&previous_key(&r.name)).ok().flatten(),
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    let due: Vec<_> = all.iter().filter(|(_, n)| n.is_some()).collect();
    if due.is_empty() {
        say!("Every service is on the patch Kiln pins for its series.");
    }
    for (r, newer) in due {
        say!(
            "{}: {} → {} (`kiln service update {}`)",
            r.name,
            r.version,
            newer.unwrap_or_default(),
            r.name
        );
    }
    Ok(())
}

/// Whether the instance's main port answers within `secs` seconds.
fn comes_up(record: &ServiceRecord, secs: u64) -> bool {
    let port = kiln_services::provider(&record.kind)
        .and_then(|p| p.ports().first())
        .and_then(|role| record.ports.get(role.role).copied());
    let Some(port) = port else { return true };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(
            &(std::net::Ipv4Addr::LOCALHOST, port).into(),
            std::time::Duration::from_millis(500),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    false
}

/// Moves `name` to `version`: stopped, its record changed, started again if it ran, checked.
fn switch(store: &mut StateStore, record: &ServiceRecord, version: &str) -> Result<bool, String> {
    let was_running = record.enabled;
    store
        .set_service_enabled(&record.name, false)
        .map_err(|e| e.to_string())?;
    crate::service::apply(store)?;
    let moved = ServiceRecord {
        version: version.to_owned(),
        enabled: was_running,
        ..record.clone()
    };
    store.put_service(&moved).map_err(|e| e.to_string())?;
    if !was_running {
        return Ok(true);
    }
    let started = crate::service::apply(store);
    Ok(started.is_ok() && comes_up(&moved, 60))
}

/// Updates one instance to `to`, keeping its previous version.
fn update_one(record: &ServiceRecord, to: &str) -> Result<(), String> {
    let provider = crate::service::provider_of(&record.kind)?;
    let release = kiln_services::release_for_this_platform(provider, Some(to))
        .ok_or_else(|| format!("Kiln has no {} {to} for this PC", provider.display_name()))?;
    let services = crate::service::services_dir()?;
    let new = kiln_services::InstanceDirs::new(&services, &record.kind, to, &record.name);
    crate::service::install_binaries(provider, release, &new.bin)?;
    crate::out::step("Checking it runs on this PC");
    provider.check(&new.bin)?;
    // A database's data first (ADR 0086), while it still runs on the old version.
    if record.enabled && provider.has_databases() {
        for database in crate::service::database_users(record).into_keys() {
            if let Some(Err(e)) = crate::snapshot::auto(
                &record.name,
                &database,
                crate::snapshot::Reason::BeforeUpdate,
            ) {
                return Err(format!(
                    "couldn't keep a snapshot of {database} first, so {} wasn't updated: {e}",
                    record.name
                ));
            }
        }
    }
    let mut store = crate::web::open_store()?;
    crate::out::step(&format!("Moving {} to {to}", record.name));
    if switch(&mut store, record, to)? {
        store
            .set_setting(&previous_key(&record.name), &record.version)
            .map_err(|e| e.to_string())?;
        say!(
            "{} runs {} {to} now (was {}; `kiln service rollback {}` goes back).",
            record.name,
            provider.display_name(),
            record.version,
            record.name
        );
        return Ok(());
    }
    // It didn't come up on the new version: back to the old one.
    let current = crate::service::record_of(&store, &record.name)?;
    let _ = switch(&mut store, &current, &record.version);
    Err(format!(
        "{} didn't start on {to}, so it's back on {}; `kiln service logs {}` says why",
        record.name, record.version, record.name
    ))
}

/// `kiln service update <name>… | --all [--yes]`.
pub fn update(names: &[String], all: bool, yes: bool) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let candidates = updates(&store)?;
    drop(store);
    let chosen: Vec<(ServiceRecord, &'static str)> = if all {
        candidates
            .into_iter()
            .filter_map(|(r, n)| n.map(|n| (r, n)))
            .collect()
    } else {
        let mut out = Vec::new();
        for name in names {
            let (record, newer) = candidates
                .iter()
                .find(|(r, _)| r.name == *name)
                .ok_or_else(|| format!("no service called {name} (see `kiln service list`)"))?;
            match newer {
                Some(n) => out.push((record.clone(), *n)),
                None => say!(
                    "{name} is already on {}, the patch Kiln pins.",
                    record.version
                ),
            }
        }
        out
    };
    if chosen.is_empty() {
        if all {
            say!("Every service is on the patch Kiln pins for its series.");
        }
        return Ok(());
    }
    let changes = chosen
        .iter()
        .map(|(r, to)| kiln_ops::Change {
            target: r.name.clone(),
            before: Some(r.version.clone()),
            after: Some((*to).to_owned()),
        })
        .collect();
    crate::apply::print_preview(
        &kiln_ops::Preview {
            summary: "Update services to the patch Kiln pins (data stays; a database is snapshotted first; the previous version is kept)".into(),
            changes,
        },
        "(none)",
    );
    if !yes && !crate::apply::confirm("Update them?") {
        say!("Nothing was changed.");
        return Ok(());
    }
    let mut failed = Vec::new();
    for (record, to) in &chosen {
        if let Err(e) = update_one(record, to) {
            crate::out::warning(&e);
            failed.push(record.name.clone());
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("not updated: {}", failed.join(", ")))
    }
}

/// `kiln service rollback <name> [--yes]`: back to the version before the last update.
pub fn rollback(name: &str, yes: bool) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let record = crate::service::record_of(&store, name)?;
    let previous = store
        .setting(&previous_key(name))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{name} has no earlier version to go back to"))?;
    let provider = crate::service::provider_of(&record.kind)?;
    let services = crate::service::services_dir()?;
    let old = kiln_services::InstanceDirs::new(&services, &record.kind, &previous, name);
    if !provider.installed(&old.bin) {
        return Err(format!(
            "{} {previous} isn't on this PC any more ({}), so {name} can't go back",
            provider.display_name(),
            old.bin.display()
        ));
    }
    crate::apply::print_preview(
        &kiln_ops::Preview {
            summary: format!("Put {name} back on {} {previous}", provider.display_name()),
            changes: vec![kiln_ops::Change {
                target: name.to_owned(),
                before: Some(record.version.clone()),
                after: Some(previous.clone()),
            }],
        },
        "(none)",
    );
    if !yes && !crate::apply::confirm(&format!("Put {name} back on {previous}?")) {
        say!("Nothing was changed.");
        return Ok(());
    }
    if !switch(&mut store, &record, &previous)? {
        let current = crate::service::record_of(&store, name)?;
        let _ = switch(&mut store, &current, &record.version);
        return Err(format!(
            "{name} didn't start on {previous}, so it stays on {}",
            record.version
        ));
    }
    store
        .remove_setting(&previous_key(name))
        .map_err(|e| e.to_string())?;
    say!("{name} runs {previous} again.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(numbers("8.4.13") > numbers("8.4.9"));
        assert!(numbers("18.6") > numbers("18.5"));
        assert!(numbers("2026.11.0") > numbers("2026.10.2"));
        assert_eq!(numbers("8.4.13"), [8, 4, 13]);
    }

    #[test]
    fn an_older_patch_of_a_pinned_series_has_an_update_and_the_pin_itself_hasnt() {
        let mysql = kiln_services::provider("mysql").unwrap();
        let Some(pin) = kiln_services::release_for_this_platform(mysql, Some("8.4")) else {
            return; // no MySQL 8.4 for this OS
        };
        let record = |version: &str| ServiceRecord {
            name: "mysql-8.4".into(),
            kind: "mysql".into(),
            version: version.into(),
            ports: [("mysql".to_owned(), 3306)].into(),
            enabled: true,
            project: None,
        };
        assert_eq!(newer(mysql, &record("8.4.0")), Some(pin.version));
        assert_eq!(newer(mysql, &record(pin.version)), None);
        assert_eq!(newer(mysql, &record("8.4.999")), None, "never a downgrade");
    }
}
