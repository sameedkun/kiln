//! `kiln service …` and `kiln services up|down` ([ADR 0035], [ADR 0036]): service instances,
//! recorded in the state database and run by a second `kiln-supervisor`.
//!
//! [ADR 0035]: ../../../docs/adr/0035-services.md
//! [ADR 0036]: ../../../docs/adr/0036-service-framework.md

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use kiln_services::{InstanceDirs, ServiceProvider};
use kiln_state::{ServiceRecord, StateStore};
use kiln_supervisor::handle::SupervisorHandle;
use kiln_supervisor::{PoolSpec, Pools};
use kiln_sys::background::ProcStatus;

/// How long services get to start listening: a first database start can be slow.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How far above its usual port Kiln looks for a free one.
const PORT_SEARCH: u16 = 100;

/// `<local data>/services`.
pub(crate) fn services_dir() -> Result<PathBuf, String> {
    Ok(kiln_state::default_local_dir()
        .ok_or("cannot determine Kiln's local data directory")?
        .join("services"))
}

/// The supervisor that runs services, separate from the PHP one ([ADR 0036] §1).
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
pub(crate) fn supervisor() -> Result<SupervisorHandle, String> {
    let exe = kiln_supervisor::handle::default_exe().map_err(|e| e.to_string())?;
    Ok(SupervisorHandle::for_programs(
        exe,
        services_dir()?.join("supervisor"),
        "services",
        READY_TIMEOUT,
    ))
}

fn provider_of(kind: &str) -> Result<&'static dyn ServiceProvider, String> {
    kiln_services::provider(kind).ok_or_else(|| {
        let known: Vec<_> = kiln_services::providers()
            .iter()
            .map(|p| p.kind())
            .collect();
        format!(
            "Kiln has no service called {kind}; it has: {}",
            known.join(", ")
        )
    })
}

fn dirs_of(record: &ServiceRecord) -> Result<InstanceDirs, String> {
    Ok(InstanceDirs::new(
        &services_dir()?,
        &record.kind,
        &record.version,
        &record.name,
    ))
}

fn record_of(store: &StateStore, name: &str) -> Result<ServiceRecord, String> {
    store
        .service(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no service called {name} (see `kiln service list`)"))
}

/// What the supervisor should run: every enabled instance.
fn desired(store: &StateStore) -> Result<Pools, String> {
    let mut pools = Vec::new();
    for record in store.services().map_err(|e| e.to_string())? {
        if !record.enabled {
            continue;
        }
        let provider = provider_of(&record.kind)?;
        let dirs = dirs_of(&record)?;
        let launch = provider.launch(&dirs, &record.ports);
        let main = provider
            .ports()
            .first()
            .and_then(|p| record.ports.get(p.role))
            .copied()
            .ok_or_else(|| format!("{} has no main port recorded", record.name))?;
        let also_listens = record
            .ports
            .values()
            .copied()
            .filter(|&p| p != main)
            .collect();
        pools.push(PoolSpec::program(
            record.name.clone(),
            launch.program,
            main,
            kiln_supervisor::pools::ProgramSpec {
                args: launch.args,
                env: launch.env,
                cwd: launch.cwd,
                log: dirs.log,
                stop_command: launch.stop_command,
                also_listens,
            },
        ));
    }
    Ok(Pools { pools })
}

/// Brings the running services in line with the enabled instances: starts or updates the
/// supervisor, or stops it when nothing should run.
fn apply(store: &StateStore) -> Result<(), String> {
    let pools = desired(store)?;
    let supervisor = supervisor()?;
    if pools.pools.is_empty() {
        supervisor.stop().map_err(|e| e.to_string())?;
        return Ok(());
    }
    supervisor.start(&pools).map_err(|e| e.to_string())?;
    Ok(())
}

fn is_free(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// Ports for a new instance ([ADR 0036] §7): what the user asked for, else the usual port if
/// it is free and no other instance has it, else the next free one above it.
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
fn choose_ports(
    provider: &dyn ServiceProvider,
    asked: &BTreeMap<String, u16>,
    taken_by_instances: &[u16],
) -> Result<BTreeMap<String, u16>, String> {
    let mut chosen = BTreeMap::new();
    for role in provider.ports() {
        if let Some(&port) = asked.get(role.role) {
            if !is_free(port) || taken_by_instances.contains(&port) {
                return Err(format!(
                    "port {port} for {} is in use; pick another with --port {}=<n>",
                    role.role, role.role
                ));
            }
            chosen.insert(role.role.to_owned(), port);
            continue;
        }
        let port = (role.usual..role.usual.saturating_add(PORT_SEARCH))
            .find(|&p| {
                is_free(p) && !taken_by_instances.contains(&p) && !chosen.values().any(|&c| c == p)
            })
            .ok_or_else(|| {
                format!(
                    "no free port between {} and {} for {}",
                    role.usual,
                    role.usual.saturating_add(PORT_SEARCH - 1),
                    role.role
                )
            })?;
        if port != role.usual {
            println!(
                "Port {} ({}) is taken; using {port} instead.",
                role.usual, role.what
            );
        }
        chosen.insert(role.role.to_owned(), port);
    }
    if let Some(unknown) = asked
        .keys()
        .find(|k| !provider.ports().iter().any(|p| p.role == k.as_str()))
    {
        let roles: Vec<_> = provider.ports().iter().map(|p| p.role).collect();
        return Err(format!(
            "{} has no port called {unknown}; its ports are: {}",
            provider.kind(),
            roles.join(", ")
        ));
    }
    Ok(chosen)
}

/// `--port role=N` values.
pub(crate) fn parse_ports(values: &[String]) -> Result<BTreeMap<String, u16>, String> {
    values
        .iter()
        .map(|v| {
            let (role, port) = v
                .split_once('=')
                .ok_or_else(|| format!("--port {v}: expected role=number, e.g. smtp=2525"))?;
            let port: u16 = port
                .parse()
                .ok()
                .filter(|&p| p > 0)
                .ok_or_else(|| format!("--port {v}: {port} isn't a port number"))?;
            Ok((role.to_owned(), port))
        })
        .collect()
}

fn describe_ports(provider: &dyn ServiceProvider, ports: &BTreeMap<String, u16>) -> String {
    provider
        .ports()
        .iter()
        .filter_map(|role| {
            ports
                .get(role.role)
                .map(|port| format!("{} 127.0.0.1:{port}", role.role))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The pinned release for `version` (or the newest), or why there is none.
fn release_of(
    provider: &dyn ServiceProvider,
    version: Option<&str>,
) -> Result<&'static kiln_services::Release, String> {
    kiln_services::release_for_this_platform(provider, version).ok_or_else(|| {
        let pinned: Vec<_> = provider
            .releases()
            .iter()
            .filter(|r| r.os == std::env::consts::OS && r.arch == std::env::consts::ARCH)
            .map(|r| r.version)
            .collect();
        if pinned.is_empty() {
            format!(
                "Kiln has no {} release for {}/{} yet",
                provider.display_name(),
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        } else {
            format!(
                "no pinned {} {}; Kiln has: {}",
                provider.display_name(),
                version.unwrap_or(""),
                pinned.join(", ")
            )
        }
    })
}

/// Downloads and installs `release` into `bin`, unless it is already there.
fn install_binaries(
    provider: &dyn ServiceProvider,
    release: &kiln_services::Release,
    bin: &Path,
) -> Result<(), String> {
    if provider.executable(bin).is_file() {
        println!(
            "{} {} is already installed.",
            provider.display_name(),
            release.version
        );
        return Ok(());
    }
    let expected = kiln_fetch::Expected::sha256(release.sha256).map_err(|e| e.to_string())?;
    let label = format!("Downloading {}", release.archive);
    let archive = crate::php::fetcher()?
        .fetch(
            release.url,
            release.archive,
            &expected,
            &mut crate::php::progress_printer(&label),
        )
        .map_err(|e| e.to_string())?;
    kiln_services::install(provider, release, &archive, bin).map_err(|e| e.to_string())?;
    println!(
        "Verified {} (SHA-256 matches the pinned release) and installed {} {} ({} license).",
        release.archive,
        provider.display_name(),
        release.version,
        provider.license()
    );
    Ok(())
}

/// `kiln service add <kind> [--version] [--name] [--port role=N]`.
pub fn add(
    kind: &str,
    version: Option<&str>,
    name: Option<&str>,
    ports: &[String],
) -> Result<(), String> {
    let provider = provider_of(kind)?;
    let release = release_of(provider, version)?;
    let major = release.version.split('.').next().unwrap_or(release.version);
    let name = name.map_or_else(|| format!("{kind}-{major}"), str::to_owned);
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
        || name.starts_with(['.', '-'])
        || name == "bin"
        || name == "supervisor"
    {
        return Err(format!(
            "{name:?} can't be a service name: use letters, digits, '.', '-' and '_'"
        ));
    }
    let mut store = crate::web::open_store()?;
    if store.service(&name).map_err(|e| e.to_string())?.is_some() {
        return Err(format!(
            "a service called {name} already exists; pass --name to add another"
        ));
    }
    let services = services_dir()?;
    let dirs = InstanceDirs::new(&services, kind, release.version, &name);
    if dirs.data.exists() {
        return Err(format!(
            "{} already holds data from an earlier {name}; remove that folder or pick another --name",
            dirs.data.display()
        ));
    }

    let taken: Vec<u16> = store
        .services()
        .map_err(|e| e.to_string())?
        .iter()
        .flat_map(|s| s.ports.values().copied())
        .collect();
    let chosen = choose_ports(provider, &parse_ports(ports)?, &taken)?;

    install_binaries(provider, release, &dirs.bin)?;

    provider
        .init_data(&dirs)
        .map_err(|e| format!("cannot set up {}: {e}", dirs.data.display()))?;
    let record = ServiceRecord {
        name: name.clone(),
        kind: kind.to_owned(),
        version: release.version.to_owned(),
        ports: chosen,
        enabled: true,
    };
    store.put_service(&record).map_err(|e| e.to_string())?;
    if let Err(e) = apply(&store) {
        return Err(format!(
            "{name} was added but didn't start: {e}\nSee `kiln service logs {name}`."
        ));
    }
    println!(
        "{} {} is running as {name}: {}.",
        provider.display_name(),
        release.version,
        describe_ports(provider, &record.ports)
    );
    println!(
        "It listens on this computer only. Its data is in {}.",
        dirs.data.display()
    );
    Ok(())
}

/// Whether the instance answers on its main port.
fn answers(record: &ServiceRecord) -> bool {
    let Some(provider) = kiln_services::provider(&record.kind) else {
        return false;
    };
    provider
        .ports()
        .first()
        .and_then(|p| record.ports.get(p.role))
        .is_some_and(|&port| {
            TcpStream::connect_timeout(
                &(Ipv4Addr::LOCALHOST, port).into(),
                Duration::from_millis(300),
            )
            .is_ok()
        })
}

/// `kiln service list`.
pub fn list() -> Result<(), String> {
    let store = crate::web::open_store()?;
    let records = store.services().map_err(|e| e.to_string())?;
    if records.is_empty() {
        let kinds: Vec<_> = kiln_services::providers()
            .iter()
            .map(|p| p.kind())
            .collect();
        println!(
            "No services yet. Add one with `kiln service add <kind>` ({}).",
            kinds.join(", ")
        );
        return Ok(());
    }
    let running = matches!(
        supervisor()?.status().map_err(|e| e.to_string())?,
        ProcStatus::Running { .. }
    );
    for record in &records {
        let state = match (record.enabled, running && answers(record)) {
            (true, true) => "running",
            (true, false) => "not running (`kiln services up` starts it)",
            (false, _) => "stopped",
        };
        let ports = kiln_services::provider(&record.kind).map_or_else(
            || format!("{:?}", record.ports),
            |p| describe_ports(p, &record.ports),
        );
        println!(
            "{}  {} {}  {ports}  {state}",
            record.name, record.kind, record.version
        );
    }
    Ok(())
}

fn set_enabled(names: &[String], enabled: bool) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let targets: Vec<String> = if names.is_empty() {
        store
            .services()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|s| s.name)
            .collect()
    } else {
        for name in names {
            record_of(&store, name)?;
        }
        names.to_vec()
    };
    if targets.is_empty() {
        println!("No services yet. Add one with `kiln service add <kind>`.");
        return Ok(());
    }
    for name in &targets {
        store
            .set_service_enabled(name, enabled)
            .map_err(|e| e.to_string())?;
    }
    apply(&store)?;
    let verb = if enabled { "Running" } else { "Stopped" };
    println!("{verb}: {}.", targets.join(", "));
    Ok(())
}

/// `kiln service start [name…]`; all of them without names (`kiln services up`).
pub fn start(names: &[String]) -> Result<(), String> {
    set_enabled(names, true)
}

/// `kiln service stop [name…]`; all of them without names (`kiln services down`).
pub fn stop(names: &[String]) -> Result<(), String> {
    set_enabled(names, false)
}

/// `kiln service restart <name>`: stopped, then started again.
pub fn restart(name: &str) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    record_of(&store, name)?;
    store
        .set_service_enabled(name, false)
        .map_err(|e| e.to_string())?;
    apply(&store)?;
    store
        .set_service_enabled(name, true)
        .map_err(|e| e.to_string())?;
    apply(&store)?;
    println!("Restarted {name}.");
    Ok(())
}

/// `kiln service logs <name> [--lines N]`.
pub fn logs(name: &str, lines: usize) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let record = record_of(&store, name)?;
    let log = dirs_of(&record)?.log;
    let text = match std::fs::read(&log) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("{name} hasn't written anything yet ({}).", log.display());
            return Ok(());
        }
        Err(e) => return Err(format!("cannot read {}: {e}", log.display())),
    };
    let all: Vec<&str> = text.lines().collect();
    for line in &all[all.len().saturating_sub(lines)..] {
        println!("{line}");
    }
    Ok(())
}

/// `kiln service remove <name> [--delete-data]` ([ADR 0036] §9): its data stays unless asked.
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
pub fn remove(name: &str, delete_data: bool) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let record = record_of(&store, name)?;
    let dirs = dirs_of(&record)?;
    store
        .set_service_enabled(name, false)
        .map_err(|e| e.to_string())?;
    apply(&store)?;
    store.remove_service(name).map_err(|e| e.to_string())?;

    if delete_data {
        match std::fs::remove_dir_all(dirs.instance()) {
            Ok(()) => println!("Deleted {} and its data.", dirs.instance().display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot delete {}: {e}", dirs.instance().display())),
        }
    } else {
        println!(
            "Kept its data: {} ({}). Delete that folder when you no longer need it; `kiln uninstall --delete-data` deletes it too.",
            dirs.data.display(),
            crate::uninstall::mb(crate::uninstall::dir_size(&dirs.data))
        );
    }
    // Its binaries go when no other instance uses that version.
    let shared = store
        .services()
        .map_err(|e| e.to_string())?
        .iter()
        .any(|s| s.kind == record.kind && s.version == record.version);
    if !shared {
        let _ = std::fs::remove_dir_all(&dirs.bin);
        // And `bin/<kind>` and `bin` once empty (remove_dir only removes empty folders).
        for parent in dirs.bin.ancestors().skip(1).take(2) {
            let _ = std::fs::remove_dir(parent);
        }
    }
    println!("Removed {name}.");
    Ok(())
}

/// Every service data folder with something in it, also of removed instances whose data was
/// kept, with its size. Uninstall lists these ([ADR 0036] §9).
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
pub(crate) fn data_folders() -> Vec<(PathBuf, u64)> {
    let Ok(services) = services_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&services) else {
        return Vec::new();
    };
    let mut found: Vec<(PathBuf, u64)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path().join("data"))
        .filter(|data| has_files(data))
        .map(|data| {
            let size = crate::uninstall::dir_size(&data);
            (data, size)
        })
        .collect();
    found.sort();
    found
}

fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_options_are_role_and_number() {
        assert_eq!(
            parse_ports(&["smtp=2525".into(), "http=8080".into()]).unwrap(),
            BTreeMap::from([("http".into(), 8080), ("smtp".into(), 2525)])
        );
        assert!(parse_ports(&["smtp".into()]).is_err());
        assert!(parse_ports(&["smtp=0".into()]).is_err());
        assert!(parse_ports(&["smtp=70000".into()]).is_err());
    }

    #[test]
    fn ports_skip_what_is_taken_and_refuse_unknown_roles() {
        let mailpit = kiln_services::provider("mailpit").unwrap();
        // Hold the usual SMTP port's neighbour range start, if we can, to force a move.
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let held_port = held.local_addr().unwrap().port();
        let chosen = choose_ports(mailpit, &BTreeMap::new(), &[1025, 8025]).unwrap();
        assert_ne!(chosen["smtp"], 1025, "taken by another instance");
        assert_ne!(chosen["http"], 8025, "taken by another instance");
        assert_ne!(chosen["smtp"], chosen["http"]);

        let asked = BTreeMap::from([("smtp".into(), held_port)]);
        assert!(choose_ports(mailpit, &asked, &[]).is_err(), "port in use");
        let asked = BTreeMap::from([("imap".into(), 1143)]);
        assert!(choose_ports(mailpit, &asked, &[]).is_err(), "no such role");
        drop(held);
    }
}
