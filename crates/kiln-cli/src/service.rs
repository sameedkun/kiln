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

pub(crate) fn provider_of(kind: &str) -> Result<&'static dyn ServiceProvider, String> {
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

pub(crate) fn dirs_of(record: &ServiceRecord) -> Result<InstanceDirs, String> {
    Ok(InstanceDirs::new(
        &services_dir()?,
        &record.kind,
        &record.version,
        &record.name,
    ))
}

pub(crate) fn record_of(store: &StateStore, name: &str) -> Result<ServiceRecord, String> {
    store
        .service(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("no service called {name} (see `kiln service list`)"))
}

/// Writes the provider's config for the instance, if it has one.
fn write_config(
    provider: &dyn ServiceProvider,
    dirs: &InstanceDirs,
    ports: &BTreeMap<String, u16>,
) -> Result<(), String> {
    let Some(text) = provider.config(dirs, ports) else {
        return Ok(());
    };
    let path = dirs.config();
    std::fs::create_dir_all(dirs.instance())
        .map_err(|e| format!("cannot create {}: {e}", dirs.instance().display()))?;
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// pgAdmin's list of Kiln's PostgreSQL instances (ADR 0071), which its launcher loads before
/// each start: one added since shows after a restart of pgAdmin.
fn write_pgadmin_servers(store: &StateStore, dirs: &InstanceDirs) -> Result<(), String> {
    let postgres = provider_of("postgres")?;
    let role = postgres.ports().first().map_or("postgres", |p| p.role);
    let instances: Vec<(String, u16)> = store
        .services()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| r.kind == "postgres")
        .filter_map(|r| r.ports.get(role).map(|&port| (r.name.clone(), port)))
        .collect();
    let path = kiln_services::pgadmin::servers_file(dirs);
    std::fs::create_dir_all(&dirs.data)
        .map_err(|e| format!("cannot create {}: {e}", dirs.data.display()))?;
    std::fs::write(&path, kiln_services::pgadmin::servers_json(&instances))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// The PHP or Node executable Kiln picks for `project`, as `provider` runs on
/// ([ADR 0060] §1).
///
/// [ADR 0060]: ../../../docs/adr/0060-dev-processes.md
fn runtime_for(
    store: &StateStore,
    provider: &dyn ServiceProvider,
    project: &Path,
) -> Result<PathBuf, String> {
    match provider.runtime() {
        kiln_services::Runtime::Php => kiln_runtime::php_for_dir(store, project)
            .map_err(|e| e.to_string())?
            .php
            .php
            .map(|php| php.dir.join(kiln_php::PHP_EXE))
            .ok_or_else(|| {
                format!(
                    "Kiln has no PHP for {}: `kiln php install` first",
                    project.display()
                )
            }),
        kiln_services::Runtime::Node => {
            let local = kiln_state::default_local_dir()
                .ok_or("cannot determine Kiln's local data directory")?;
            let installed = kiln_node::installed(&local.join(kiln_runtime::node::NODE_DIR));
            let default = store
                .setting(kiln_runtime::node::DEFAULT_KEY)
                .map_err(|e| e.to_string())?;
            kiln_runtime::node::node_for_dir(&installed, default.as_deref(), project)
                .node
                .map(|node| node.node_exe())
                .ok_or_else(|| {
                    format!(
                        "Kiln has no Node for {}: `kiln node install` first",
                        project.display()
                    )
                })
        }
    }
}

/// How a service that runs in a project starts: in its folder, with the PHP or Node Kiln picks
/// there.
fn project_launch(
    store: &StateStore,
    provider: &dyn ServiceProvider,
    record: &ServiceRecord,
) -> Result<kiln_services::Launch, String> {
    let project = record
        .project
        .as_deref()
        .ok_or("it has no project folder recorded")?;
    if !project.is_dir() {
        return Err(format!("{} is gone", project.display()));
    }
    let runtime = runtime_for(store, provider, project)?;
    provider
        .project_launch(project, &runtime, &record.ports)
        .ok_or_else(|| "it has no way to start".to_owned())
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
        let launch = if provider.runs_in_project() {
            // The project's PHP as it is now (ADR 0047): a site pinned to another version
            // brings its Reverb along. A project that can't run is skipped, not fatal.
            match project_launch(store, provider, &record) {
                Ok(launch) => launch,
                Err(why) => {
                    say!("Note: {} isn't started: {why}.", record.name);
                    continue;
                }
            }
        } else {
            // Rewritten before every start, so a changed port takes effect.
            write_config(provider, &dirs, &record.ports)?;
            if record.kind == "pgadmin" {
                write_pgadmin_servers(store, &dirs)?;
            }
            provider.launch(&dirs, &record.ports)
        };
        // A process that listens on nothing (a queue worker, ADR 0060) runs on port 0.
        let main = match provider.ports().first() {
            None => 0,
            Some(p) => record
                .ports
                .get(p.role)
                .copied()
                .ok_or_else(|| format!("{} has no main port recorded", record.name))?,
        };
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
/// supervisor, or stops it when nothing should run. Refuses first if another program has
/// taken one of their ports ([ADR 0037] §3).
///
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
fn apply(store: &StateStore) -> Result<(), String> {
    let pools = desired(store)?;
    let supervisor = supervisor()?;
    // What ran before, to wait for what no longer should: the supervisor stops programs in
    // the background, and a stopped service must really be gone before the command ends.
    let before = supervisor.pools().ok().flatten().unwrap_or_default();
    if pools.pools.is_empty() {
        supervisor.stop().map_err(|e| e.to_string())?;
    } else {
        check_ports(store)?;
        let log_from = std::fs::metadata(supervisor.log_path()).map_or(0, |m| m.len());
        supervisor.start(&pools).map_err(|e| e.to_string())?;
        // A process on no port can't be seen listening: the supervisor's log says when one
        // started or stopped (ADR 0060 §2).
        let missing = wait_logged(
            &supervisor.log_path(),
            log_from,
            &portless_changes(&before, &pools),
        );
        if !missing.is_empty() {
            say!(
                "Note: still waiting on the supervisor for: {}. See `kiln service logs <name>`.",
                missing.join("; ")
            );
        }
        // Listening isn't always accepting work yet (a database still starting up).
        let starting: Vec<_> = pools
            .pools
            .iter()
            .filter_map(|p| store.service(&p.id).ok().flatten())
            .filter_map(|record| {
                Some((
                    record.name.clone(),
                    provider_of(&record.kind).ok()?,
                    dirs_of(&record).ok()?,
                    record.ports.clone(),
                ))
            })
            .collect();
        let deadline = std::time::Instant::now() + READY_TIMEOUT;
        while let Some((name, ..)) = starting.iter().find(|(_, p, d, ports)| !p.ready(d, ports)) {
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "{name} is listening but not ready after {} s. See `kiln service logs {name}`.",
                    READY_TIMEOUT.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let wanted: Vec<u16> = pools
        .ready_addresses()
        .iter()
        .map(std::net::SocketAddr::port)
        .collect();
    let gone: Vec<u16> = before
        .ready_addresses()
        .iter()
        .map(std::net::SocketAddr::port)
        .filter(|p| !wanted.contains(p))
        .collect();
    // The instances that were stopped, which may still be finishing (a database flushes
    // its data after closing its port).
    let stopping: Vec<(&'static dyn ServiceProvider, InstanceDirs)> = before
        .pools
        .iter()
        .filter(|old| !pools.pools.iter().any(|p| p.id == old.id))
        .filter_map(|old| store.service(&old.id).ok().flatten())
        .filter_map(|record| Some((provider_of(&record.kind).ok()?, dirs_of(&record).ok()?)))
        .collect();
    if !wait_released(&gone, &stopping) {
        say!("Note: a service is taking long to stop; it may still be finishing.");
    }
    // What a stopped process in a project leaves behind, such as Vite's `public/hot`.
    for old in before
        .pools
        .iter()
        .filter(|old| !pools.pools.iter().any(|p| p.id == old.id))
    {
        if let Some(record) = store.service(&old.id).ok().flatten()
            && let Some(project) = &record.project
            && let Ok(provider) = provider_of(&record.kind)
        {
            provider.after_stop(project, &record.ports);
        }
    }
    // The web tools list the instances as they now are (ADR 0043). Only a convenience: a
    // failure is reported, never in the way of the services themselves.
    if let Err(e) = crate::tool::write_servers(store) {
        say!("Note: the web tools' server list wasn't updated: {e}");
    }
    Ok(())
}

/// The supervisor log lines that say each process on no port started (new in `after`) or
/// stopped (gone from `before`).
fn portless_changes(before: &Pools, after: &Pools) -> Vec<String> {
    let portless = |p: &&kiln_supervisor::pools::PoolSpec| {
        p.program_spec.is_some() && p.workers.iter().all(|w| w.port == 0)
    };
    let has = |pools: &Pools, id: &str| pools.pools.iter().any(|p| p.id == id);
    let started = after
        .pools
        .iter()
        .filter(portless)
        .filter(|p| !has(before, &p.id))
        .map(|p| format!("{} :0 started", p.id));
    let stopped = before
        .pools
        .iter()
        .filter(portless)
        .filter(|p| !has(after, &p.id))
        .map(|p| format!("{} :0 stopped", p.id));
    started.chain(stopped).collect()
}

/// Waits until each of `lines` is in the log at `path` after byte `from`, for as long as a
/// graceful stop may take; returns those that never came.
fn wait_logged(path: &Path, from: u64, lines: &[String]) -> Vec<String> {
    let deadline = std::time::Instant::now()
        + kiln_supervisor::handle::PROGRAM_STOP_TIMEOUT
        + Duration::from_secs(5);
    loop {
        let text = std::fs::read(path)
            .ok()
            .and_then(|b| b.get(usize::try_from(from).ok()?..).map(<[u8]>::to_vec))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let missing: Vec<String> = lines
            .iter()
            .filter(|l| !text.lines().any(|t| t.ends_with(l.as_str())))
            .cloned()
            .collect();
        if missing.is_empty() || std::time::Instant::now() >= deadline {
            return missing;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Waits until every port in `ports` is free again and every instance in `stopping` has
/// finished stopping, for as long as a graceful stop may take (the supervisor's own limit).
/// Returns whether all have.
fn wait_released(ports: &[u16], stopping: &[(&'static dyn ServiceProvider, InstanceDirs)]) -> bool {
    let deadline = std::time::Instant::now() + kiln_supervisor::handle::PROGRAM_STOP_TIMEOUT;
    loop {
        if ports.iter().all(|&p| is_free(p)) && stopping.iter().all(|(p, d)| p.stopped(d)) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn is_free(port: u16) -> bool {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// Whether `holder` is one of Kiln's own service programs (in `<services>/bin`).
fn is_ours(holder: &kiln_coexist::Holder) -> bool {
    let Ok(bin) = services_dir().map(|s| s.join("bin")) else {
        return false;
    };
    // A service that runs in a project runs Kiln's PHP (ADR 0047) or Node (ADR 0060).
    let runtimes: Vec<PathBuf> = kiln_state::default_local_dir()
        .iter()
        .flat_map(|local| [local.join("php"), local.join(kiln_runtime::node::NODE_DIR)])
        .collect();
    holder.image.as_ref().is_some_and(|image| {
        std::iter::once(&bin).chain(&runtimes).any(|dir| {
            if cfg!(windows) {
                image
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with(&dir.to_string_lossy().to_lowercase())
            } else {
                image.starts_with(dir)
            }
        })
    })
}

/// Every port of every instance that should run is free, or held by Kiln's own instance
/// ([ADR 0037] §3); otherwise who holds it, and the two ways out.
///
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
fn check_ports(store: &StateStore) -> Result<(), String> {
    let listeners = crate::coexist::Listeners::now();
    let mut problems = Vec::new();
    for record in store.services().map_err(|e| e.to_string())? {
        if !record.enabled {
            continue;
        }
        for (role, &port) in &record.ports {
            if is_free(port) || listeners.holder(port).is_some_and(|h| is_ours(&h)) {
                continue;
            }
            let why = crate::coexist::why_busy_service(
                &listeners,
                port,
                &crate::coexist::system_services(),
            );
            problems.push(format!(
                "{} can't start: port {port} ({role}) is {why}. Stop that program, or move {} with `kiln service port {} {role}=<port>`.",
                record.name, record.name, record.name
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// Ports for a new instance ([ADR 0036] §7): what the user asked for, else the usual port if
/// it is free and no other instance has it, else the next free one above it. A move says
/// who holds the usual port ([ADR 0037] §2). `taken` maps other instances' ports to their
/// names.
///
/// [ADR 0036]: ../../../docs/adr/0036-service-framework.md
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
fn choose_ports(
    provider: &dyn ServiceProvider,
    asked: &BTreeMap<String, u16>,
    taken: &BTreeMap<u16, String>,
) -> Result<BTreeMap<String, u16>, String> {
    let mut chosen = BTreeMap::new();
    for role in provider.ports() {
        if let Some(&port) = asked.get(role.role) {
            if let Some(owner) = taken.get(&port) {
                return Err(format!(
                    "port {port} for {} is Kiln's {owner}'s; pick another with --port {}=<n>",
                    role.role, role.role
                ));
            }
            if !is_free(port) {
                let why = crate::coexist::why_busy_service(
                    &crate::coexist::Listeners::now(),
                    port,
                    &crate::coexist::system_services(),
                );
                return Err(format!(
                    "port {port} for {} is {why}; pick another with --port {}=<n>",
                    role.role, role.role
                ));
            }
            chosen.insert(role.role.to_owned(), port);
            continue;
        }
        let port = (role.usual..role.usual.saturating_add(PORT_SEARCH))
            .find(|&p| is_free(p) && !taken.contains_key(&p) && !chosen.values().any(|&c| c == p))
            .ok_or_else(|| {
                format!(
                    "no free port between {} and {} for {}",
                    role.usual,
                    role.usual.saturating_add(PORT_SEARCH - 1),
                    role.role
                )
            })?;
        if port != role.usual {
            let why = match taken.get(&role.usual) {
                Some(owner) => format!("used by Kiln's {owner}"),
                None => crate::coexist::why_busy_service(
                    &crate::coexist::Listeners::now(),
                    role.usual,
                    &crate::coexist::system_services(),
                ),
            };
            say!(
                "Port {} ({}) is {why}; using {port}.",
                role.usual,
                role.what
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

/// The ports of every instance but `except`, with the instance's name.
fn ports_of_others(store: &StateStore, except: &str) -> Result<BTreeMap<u16, String>, String> {
    Ok(store
        .services()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|s| s.name != except)
        .flat_map(|s| {
            s.ports
                .values()
                .map(|&port| (port, s.name.clone()))
                .collect::<Vec<_>>()
        })
        .collect())
}

/// `kiln service port <name> <role>=<port>…` ([ADR 0037] §4): new ports for an instance,
/// which must be free; a running instance restarts on them, and its data stays.
///
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
pub fn port(name: &str, values: &[String]) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    let mut record = record_of(&store, name)?;
    let provider = provider_of(&record.kind)?;
    let asked = parse_ports(values)?;
    if asked.is_empty() {
        return Err(format!(
            "give the new ports, e.g. `kiln service port {name} {}=<port>`",
            provider.ports().first().map_or("port", |p| p.role)
        ));
    }
    let ui_before = ui_target(provider, &record.ports);
    let others = ports_of_others(&store, name)?;
    let listeners = crate::coexist::Listeners::now();
    for (role, &port) in &asked {
        if !provider.ports().iter().any(|p| p.role == role.as_str()) {
            let roles: Vec<_> = provider.ports().iter().map(|p| p.role).collect();
            return Err(format!(
                "{} has no port called {role}; its ports are: {}",
                provider.kind(),
                roles.join(", ")
            ));
        }
        if let Some(owner) = others.get(&port) {
            return Err(format!("port {port} is Kiln's {owner}'s"));
        }
        // Its own current port is fine; anything else must be free.
        let own = record.ports.get(role) == Some(&port);
        if !own && !is_free(port) {
            return Err(format!(
                "port {port} is {}",
                crate::coexist::why_busy_service(
                    &listeners,
                    port,
                    &crate::coexist::system_services()
                )
            ));
        }
    }
    record.ports.extend(asked);
    store.put_service(&record).map_err(|e| e.to_string())?;
    if record.enabled {
        apply(&store)?;
    }
    // A web UI site that followed the old port follows the new one.
    if let (Some((site, old)), Some((_, new))) = (ui_before, ui_target(provider, &record.ports))
        && old != new
        && let Some(proxy) = ui_proxy(site, &old)?
    {
        crate::proxy::proxy(site, &new, proxy.secure)?;
    }
    say!(
        "{name} now uses {}{}.",
        describe_ports(provider, &record.ports),
        if record.enabled {
            " and is running there"
        } else {
            ""
        }
    );
    say!(
        "Projects wired to its old ports need their .env changed (`kiln env use` comes with P3-08)."
    );
    Ok(())
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
        say!(
            "{} {} is already installed.",
            provider.display_name(),
            release.version
        );
    } else {
        let archive = download(release)?;
        crate::out::step(&format!(
            "Unpacking {} {}",
            provider.display_name(),
            release.version
        ));
        kiln_services::install(provider, release, &archive, bin).map_err(|e| e.to_string())?;
        say!(
            "Verified {} (SHA-256 matches the pinned release) and installed {} {}.",
            release.archive,
            provider.display_name(),
            release.version,
        );
        say!("License: {}.", provider.license());
    }
    // After the server: installing it clears `bin`, companions included.
    for companion in provider.companions(release) {
        let dir = kiln_services::release_folder(bin, companion);
        if dir.join(companion.executable).is_file() {
            continue;
        }
        let archive = download(companion)?;
        crate::out::step(&format!("Unpacking {}", companion.archive));
        kiln_services::install_into_folder(companion, &archive, bin).map_err(|e| e.to_string())?;
        say!(
            "Verified {} (SHA-256 matches the pinned release) and installed it with {}.",
            companion.archive,
            provider.display_name(),
        );
    }
    Ok(())
}

/// Downloads `release`'s archive (or takes it from the cache), checked against its pin.
pub(crate) fn download(release: &kiln_services::Release) -> Result<std::path::PathBuf, String> {
    let expected = kiln_fetch::Expected::sha256(release.sha256).map_err(|e| e.to_string())?;
    let label = format!("Downloading {}", release.archive);
    crate::php::fetcher()?
        .fetch(
            release.url,
            release.archive,
            &expected,
            &mut crate::php::progress_printer(&label),
        )
        .map_err(|e| e.to_string())
}

/// For a service that runs in a project (Reverb, [ADR 0047]): the project folder, the version
/// it has installed, and the label its instance is named after (its site's, else the
/// folder's).
///
/// [ADR 0047]: ../../../docs/adr/0047-reverb.md
fn in_project(
    provider: &dyn ServiceProvider,
    project: Option<PathBuf>,
) -> Result<(PathBuf, String, String), String> {
    let dir = match project {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let dir = std::fs::canonicalize(&dir)
        .map(crate::web::strip_verbatim)
        .map_err(|e| format!("cannot use {}: {e}", dir.display()))?;
    let version = provider.project_version(&dir)?;
    let store = crate::web::open_store()?;
    runtime_for(&store, provider, &dir)?;
    let found = kiln_runtime::php_for_dir(&store, &dir).map_err(|e| e.to_string())?;
    let label: String = found
        .site
        .as_deref()
        .and_then(|host| host.split('.').next())
        .map(str::to_owned)
        .or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
        })
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    Ok((dir, version, label))
}

/// What `service add` says once an instance runs: its ports, and where it runs or keeps its
/// data.
fn say_added(
    provider: &dyn ServiceProvider,
    record: &ServiceRecord,
    project: Option<&Path>,
    dirs: &InstanceDirs,
) {
    let (name, version) = (&record.name, &record.version);
    let ports = describe_ports(provider, &record.ports);
    if ports.is_empty() {
        say!(
            "{} {version} is running as {name}.",
            provider.display_name()
        );
    } else {
        say!(
            "{} {version} is running as {name}: {ports}.",
            provider.display_name()
        );
    }
    let runtime = match provider.runtime() {
        kiln_services::Runtime::Php => "PHP",
        kiln_services::Runtime::Node => "Node",
    };
    match project {
        Some(dir) if ports.is_empty() => say!(
            "It runs in {} with its {runtime}, and Kiln keeps it running.",
            dir.display()
        ),
        Some(dir) => say!(
            "It listens on this computer only, and runs in {} with its {runtime}.",
            dir.display()
        ),
        None => say!(
            "It listens on this computer only. Its data is in {}.",
            dirs.data.display()
        ),
    }
}

/// `kiln service add <kind> [--version] [--name] [--port role=N] [--project]`. Returns the
/// instance's name.
pub fn add(
    kind: &str,
    version: Option<&str>,
    name: Option<&str>,
    ports: &[String],
    project: Option<PathBuf>,
) -> Result<String, String> {
    let provider = provider_of(kind)?;
    let (version, series, project) = if provider.runs_in_project() {
        let (dir, version, label) = in_project(provider, project)?;
        (version, label, Some(dir))
    } else {
        if project.is_some() {
            return Err(format!(
                "{kind} doesn't run in a project; --project is for reverb, queue, scheduler and vite"
            ));
        }
        let release = release_of(provider, version)?;
        (
            release.version.to_owned(),
            provider.series(release.version),
            None,
        )
    };
    let name = name.map_or_else(|| format!("{kind}-{series}"), str::to_owned);
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
    let dirs = InstanceDirs::new(&services, kind, &version, &name);
    if dirs.data.exists() {
        return Err(format!(
            "{} already holds data from an earlier {name}; remove that folder or pick another --name",
            dirs.data.display()
        ));
    }

    let chosen = choose_ports(
        provider,
        &parse_ports(ports)?,
        &ports_of_others(&store, "")?,
    )?;

    if project.is_none() {
        let release = release_of(provider, Some(&version))?;
        install_binaries(provider, release, &dirs.bin)?;
        // Runs on this system? (A missing VC++ runtime or Linux library, ADR 0038 §6.)
        crate::out::step("Checking it runs on this PC");
        provider.check(&dirs.bin)?;
        crate::out::step("Setting up its data folder");
    }

    write_config(provider, &dirs, &chosen)?;
    if let Err(e) = provider.init_data(&dirs, &chosen) {
        // Nothing was there before, so nothing of the user's is lost.
        let _ = std::fs::remove_dir_all(dirs.instance());
        return Err(e);
    }
    let record = ServiceRecord {
        name: name.clone(),
        kind: kind.to_owned(),
        version: version.clone(),
        ports: chosen,
        enabled: true,
        project: project.clone(),
    };
    store.put_service(&record).map_err(|e| e.to_string())?;
    crate::out::step(&format!("Starting {name}"));
    if let Err(e) = apply(&store) {
        return Err(format!(
            "{name} was added but didn't start: {e}\nSee `kiln service logs {name}`."
        ));
    }
    say_added(provider, &record, project.as_deref(), &dirs);
    // Its web UI as a site: a convenience, so a failure is reported, not fatal.
    if let Err(e) = attach_ui(provider, &record) {
        say!("Note: its web UI wasn't served as a site: {e}");
    }
    Ok(name)
}

/// Whether the instance runs: its main port answers, and Kiln's own program holds it. Another
/// program on that port answers too, and isn't this instance ([ADR 0037] §3).
///
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
fn answers(record: &ServiceRecord, listeners: &crate::coexist::Listeners) -> bool {
    let Some(provider) = kiln_services::provider(&record.kind) else {
        return false;
    };
    // A process that listens on nothing (a queue worker, ADR 0060) runs while the
    // supervisor does, which keeps it up.
    if provider.ports().is_empty() {
        return true;
    }
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
                && listeners.holder(port).is_some_and(|h| is_ours(&h))
        })
}

/// Each site's host and its `.env` text, for "used by" (ADR 0056 §1).
fn site_projects(store: &StateStore) -> Vec<(String, String)> {
    crate::web::resolved(store)
        .map(|all| {
            all.sites
                .iter()
                .filter(|s| s.alias_of.is_none())
                .filter_map(|s| {
                    std::fs::read_to_string(s.project.join(".env"))
                        .ok()
                        .map(|text| (s.host.to_string(), text))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The sites whose `.env` holds every key Kiln would write for this instance's port (ADR 0056
/// §1). A key counts if its value names one of the instance's ports.
fn used_by(record: &ServiceRecord, projects: &[(String, String)]) -> Vec<String> {
    let Some(provider) = kiln_services::provider(&record.kind) else {
        return Vec::new();
    };
    let ports: Vec<String> = record.ports.values().map(ToString::to_string).collect();
    let keys: Vec<(&str, String)> = provider
        .env_keys(&record.ports)
        .into_iter()
        .filter(|(_, value)| ports.iter().any(|p| value.contains(p.as_str())))
        .collect();
    uses(&keys, projects)
}

/// For an instance's page (ADR 0068 §1): each database name, and the sites whose `.env` points
/// at this instance with that `DB_DATABASE`.
pub(crate) fn database_users(record: &ServiceRecord) -> BTreeMap<String, Vec<String>> {
    let Ok(store) = crate::web::open_store() else {
        return BTreeMap::new();
    };
    let projects = site_projects(&store);
    let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for host in used_by(record, &projects) {
        let database = projects
            .iter()
            .find(|(h, _)| *h == host)
            .and_then(|(_, text)| kiln_platform::env_file::value_of(text, "DB_DATABASE"));
        if let Some(database) = database {
            users.entry(database).or_default().push(host);
        }
    }
    users
}

/// The hosts of `projects` whose `.env` text has every one of `keys`.
fn uses(keys: &[(&str, String)], projects: &[(String, String)]) -> Vec<String> {
    if keys.is_empty() {
        return Vec::new();
    }
    projects
        .iter()
        .filter(|(_, text)| {
            keys.iter().all(|(key, value)| {
                kiln_platform::env_file::value_of(text, key).as_deref() == Some(value.as_str())
            })
        })
        .map(|(host, _)| host.clone())
        .collect()
}

/// The host of the site serving this instance's web page (Mailpit's inbox), if Kiln serves one.
fn web_host(record: &ServiceRecord, proxies: &[kiln_web::SiteSpec]) -> Option<String> {
    let provider = kiln_services::provider(&record.kind)?;
    let (_, role) = provider.web_ui()?;
    let port = record.ports.get(role)?;
    let suffix = format!(":{port}");
    proxies
        .iter()
        .find(|p| {
            p.proxy
                .as_ref()
                .is_some_and(|t| t.to_string().trim_end_matches('/').ends_with(&suffix))
        })
        .map(|p| p.host.to_string())
}

/// `kiln service catalog`: every kind Kiln offers, with the versions for this machine.
pub fn catalog() {
    let here = |r: &&kiln_services::Release| {
        r.os == std::env::consts::OS && r.arch == std::env::consts::ARCH
    };
    let kinds: Vec<_> = kiln_services::providers()
        .iter()
        .map(|p| {
            let versions: Vec<&str> = p
                .releases()
                .iter()
                .filter(here)
                .map(|r| r.version)
                .collect();
            (p, versions)
        })
        .collect();
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "kinds": kinds.iter().map(|(p, versions)| serde_json::json!({
                "kind": p.kind(),
                "name": p.display_name(),
                "license": p.license(),
                "versions": versions,
                "ports": p.ports().iter().map(|port| serde_json::json!({
                    "role": port.role,
                    "usual": port.usual,
                    "what": port.what,
                })).collect::<Vec<_>>(),
                "runs_in_project": p.runs_in_project(),
            })).collect::<Vec<_>>(),
        }));
        return;
    }
    for (p, versions) in &kinds {
        let shown = if p.runs_in_project() {
            "runs in its project (composer require)".to_owned()
        } else if versions.is_empty() {
            "not for this machine yet".to_owned()
        } else {
            versions.join(", ")
        };
        say!(
            "{:<12} {:<22} {shown}  ({})",
            p.kind(),
            p.display_name(),
            p.license()
        );
    }
    say!("Add one with `kiln service add <kind> [--version X]`.");
}

/// `kiln service list`.
pub fn list() -> Result<(), String> {
    let store = crate::web::open_store()?;
    let records = store.services().map_err(|e| e.to_string())?;
    if crate::out::json() {
        let projects = site_projects(&store);
        let proxies = crate::web::proxy_specs(&store).unwrap_or_default();
        let listeners = crate::coexist::Listeners::now();
        let running = matches!(
            supervisor()?.status().map_err(|e| e.to_string())?,
            ProcStatus::Running { .. }
        );
        crate::out::data(serde_json::json!({
            "supervisor_running": running,
            "instances": records.iter().map(|r| serde_json::json!({
                "name": r.name,
                "kind": r.kind,
                "version": r.version,
                "ports": r.ports,
                "used_by": used_by(r, &projects),
                // What `kiln env use` writes, for the page's .env tab (ADR 0068 §3).
                "env": kiln_services::provider(&r.kind)
                    .map(|p| p.env_keys(&r.ports))
                    .unwrap_or_default(),
                "web": web_host(r, &proxies),
                "databases": kiln_services::provider(&r.kind)
                    .is_some_and(kiln_services::ServiceProvider::has_databases),
                // The port apps connect to (the provider's first role), not the first by name.
                "main_port": kiln_services::provider(&r.kind)
                    .and_then(|p| p.ports().first())
                    .and_then(|role| r.ports.get(role.role).copied()),
                "enabled": r.enabled,
                "running": r.enabled && running && answers(r, &listeners),
                "project": r.project,
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if records.is_empty() {
        let kinds: Vec<_> = kiln_services::providers()
            .iter()
            .map(|p| p.kind())
            .collect();
        say!(
            "No services yet. Add one with `kiln service add <kind>` ({}).",
            kinds.join(", ")
        );
        return Ok(());
    }
    let listeners = crate::coexist::Listeners::now();
    let running = matches!(
        supervisor()?.status().map_err(|e| e.to_string())?,
        ProcStatus::Running { .. }
    );
    for record in &records {
        let state = match (record.enabled, running && answers(record, &listeners)) {
            (true, true) => "running",
            (true, false) => "not running (`kiln services up` starts it)",
            (false, _) => "stopped",
        };
        let ports = kiln_services::provider(&record.kind).map_or_else(
            || format!("{:?}", record.ports),
            |p| describe_ports(p, &record.ports),
        );
        say!(
            "{}  {} {}  {ports}  {state}",
            record.name,
            record.kind,
            record.version
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
        say!("No services yet. Add one with `kiln service add <kind>`.");
        return Ok(());
    }
    for name in &targets {
        store
            .set_service_enabled(name, enabled)
            .map_err(|e| e.to_string())?;
    }
    apply(&store)?;
    let verb = if enabled { "Running" } else { "Stopped" };
    say!("{verb}: {}.", targets.join(", "));
    Ok(())
}

/// `kiln services resume` ([ADR 0048]): the enabled instances started again under a new
/// supervisor, after the old one exited on its own. Nothing is enabled or disabled.
///
/// [ADR 0048]: ../../../docs/adr/0048-kilnd-and-services.md
pub fn resume() -> Result<(), String> {
    let store = crate::web::open_store()?;
    let running: Vec<String> = store
        .services()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.name)
        .collect();
    apply(&store)?;
    if running.is_empty() {
        say!("No services should be running.");
    } else {
        say!("Running again: {}.", running.join(", "));
    }
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
    say!("Restarted {name}.");
    Ok(())
}

/// `kiln service logs <name> [--lines N]`.
pub fn logs(name: &str, lines: usize) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let record = record_of(&store, name)?;
    let provider = provider_of(&record.kind)?;
    let logs = provider.logs(&dirs_of(&record)?);
    if crate::out::json() {
        let mut files = Vec::new();
        for log in &logs {
            let text = match std::fs::read(log) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("cannot read {}: {e}", log.display())),
            };
            let all: Vec<&str> = text.lines().collect();
            files.push(serde_json::json!({
                "path": log,
                "lines": all[all.len().saturating_sub(lines)..],
            }));
        }
        crate::out::data(serde_json::json!({ "files": files }));
        return Ok(());
    }
    let mut wrote = false;
    for log in &logs {
        let text = match std::fs::read(log) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("cannot read {}: {e}", log.display())),
        };
        let all: Vec<&str> = text.lines().collect();
        if all.is_empty() {
            continue;
        }
        if logs.len() > 1 {
            say!("== {}", log.display());
        }
        for line in &all[all.len().saturating_sub(lines)..] {
            say!("{line}");
        }
        wrote = true;
    }
    if !wrote {
        say!("{name} hasn't written anything yet.");
    }
    Ok(())
}

/// `kiln db <instance> [database]` ([ADR 0038] §7): the service's client on the instance, in
/// this terminal.
///
/// [ADR 0038]: ../../../docs/adr/0038-mysql-and-mariadb.md
pub fn db(name: &str, database: Option<&str>) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let record = record_of(&store, name)?;
    let provider = provider_of(&record.kind)?;
    let client = provider
        .client(&dirs_of(&record)?, &record.ports, database)
        .ok_or_else(|| format!("{} has no client to open", provider.display_name()))?;
    let status = std::process::Command::new(&client.program)
        .args(&client.args)
        .envs(&client.env)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", client.program.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} exited with {status}", client.program.display()))
    }
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
        // Windows can keep a stopped program's files open for a moment after it exits.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::remove_dir_all(dirs.instance()) {
                Ok(()) => {
                    say!("Deleted {} and its data.", dirs.instance().display());
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => {
                    return Err(format!("cannot delete {}: {e}", dirs.instance().display()));
                }
            }
        }
    } else {
        say!(
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
    say!("Removed {name}.");
    // Its web UI site goes with it, if it was Kiln's for this instance.
    if let Ok(provider) = provider_of(&record.kind)
        && let Some((site, target)) = ui_target(provider, &record.ports)
        && ui_proxy(site, &target)?.is_some()
    {
        crate::proxy::unproxy(site)?;
    }
    Ok(())
}

/// What `kiln doctor` reports about Kiln's instances ([ADR 0037] §5): whether each runs, and
/// which of its ports another program holds.
///
/// [ADR 0037]: ../../../docs/adr/0037-service-ports-and-coexistence.md
pub(crate) fn doctor_facts(
    store: Option<&StateStore>,
    listeners: &crate::coexist::Listeners,
    system: &[kiln_coexist::SystemService],
) -> kiln_doctor::ServicesFacts {
    let kinds = kiln_services::providers()
        .iter()
        .map(|p| p.kind().to_owned())
        .collect();
    // An older state database has no services table: nothing to report.
    let records = store.and_then(|s| s.services().ok()).unwrap_or_default();
    let instances = records
        .iter()
        .map(|record| {
            let provider = kiln_services::provider(&record.kind);
            let running = record.enabled && answers(record, listeners);
            let conflicts = if record.enabled && !running {
                record
                    .ports
                    .iter()
                    .filter(|&(_, &port)| {
                        !is_free(port) && !listeners.holder(port).is_some_and(|h| is_ours(&h))
                    })
                    .map(|(role, &port)| {
                        (
                            port,
                            role.clone(),
                            crate::coexist::why_busy_service(listeners, port, system),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            kiln_doctor::InstanceFacts {
                name: record.name.clone(),
                what: format!(
                    "{} {}",
                    provider.map_or(record.kind.as_str(), |p| p.display_name()),
                    record.version
                ),
                ports: provider.map_or_else(
                    || format!("{:?}", record.ports),
                    |p| describe_ports(p, &record.ports),
                ),
                enabled: record.enabled,
                running,
                conflicts,
            }
        })
        .collect();
    kiln_doctor::ServicesFacts { instances, kinds }
}

/// Whether `pid` runs one of Kiln's own service programs.
pub(crate) fn is_ours_pid(pid: u32) -> bool {
    is_ours(&crate::coexist::holder(pid))
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

/// Where `provider`'s web UI is for an instance on `ports`, and its site name: the
/// proxy site Kiln serves it as ([ADR 0044]).
///
/// [ADR 0044]: ../../../docs/adr/0044-service-web-uis.md
fn ui_target(
    provider: &dyn ServiceProvider,
    ports: &BTreeMap<String, u16>,
) -> Option<(&'static str, String)> {
    let (site, role) = provider.web_ui()?;
    let port = ports.get(role)?;
    Some((site, format!("http://127.0.0.1:{port}")))
}

/// The proxy `<site>.test`, if it forwards to `target`: one Kiln made for this instance.
fn ui_proxy(site: &str, target: &str) -> Result<Option<kiln_state::ProxyRecord>, String> {
    let host = crate::web::host_name(site)?;
    let store = crate::web::open_store()?;
    let normal = |t: &str| kiln_web::ProxyTarget::parse(t).map(|t| t.to_string()).ok();
    Ok(store
        .proxy(host.as_str())
        .map_err(|e| e.to_string())?
        .filter(|p| normal(&p.target).is_some() && normal(&p.target) == normal(target)))
}

/// After `kiln service add`: serves the instance's web UI as `<site>.test`, over HTTPS when
/// Kiln's CA is set up, unless that name is taken by a site or another proxy.
fn attach_ui(provider: &dyn ServiceProvider, record: &ServiceRecord) -> Result<(), String> {
    let Some((site, target)) = ui_target(provider, &record.ports) else {
        return Ok(());
    };
    let host = crate::web::host_name(site)?;
    let store = crate::web::open_store()?;
    let taken = store
        .site(host.as_str())
        .map_err(|e| e.to_string())?
        .is_some()
        || store
            .proxy(host.as_str())
            .map_err(|e| e.to_string())?
            .is_some();
    drop(store);
    if taken {
        say!(
            "Its web UI is at {target}; {host} is already taken, so `kiln proxy <name> {target}` serves it under another name."
        );
        return Ok(());
    }
    let secure = crate::web::ca_files().is_ok();
    crate::proxy::proxy(site, &target, secure)?;
    if !secure {
        say!(
            "It's served over HTTP until Kiln's CA is set up: `kiln ca init`, then `kiln secure {site}`."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_less_processes_are_waited_for_by_their_log_lines() {
        let program = |id: &str, port| {
            PoolSpec::program(
                id.into(),
                "/k/php/php".into(),
                port,
                kiln_supervisor::pools::ProgramSpec::default(),
            )
        };
        let before = Pools {
            pools: vec![program("queue-shop", 0), program("mysql-8.4", 3306)],
        };
        let after = Pools {
            pools: vec![program("scheduler-shop", 0), program("mysql-8.4", 3306)],
        };
        assert_eq!(
            portless_changes(&before, &after),
            ["scheduler-shop :0 started", "queue-shop :0 stopped"]
        );
        assert_eq!(portless_changes(&after, &after).len(), 0);
    }

    #[test]
    fn a_site_uses_an_instance_when_its_env_names_every_port_key() {
        let keys = vec![
            ("DB_HOST", "127.0.0.1".to_owned()),
            ("DB_PORT", "3306".to_owned()),
        ];
        let projects = vec![
            (
                "shop.test".to_owned(),
                "DB_HOST=127.0.0.1\nDB_PORT=3306\n".to_owned(),
            ),
            (
                "blog.test".to_owned(),
                "DB_HOST=127.0.0.1\nDB_PORT=5432\n".to_owned(),
            ),
            ("docs.test".to_owned(), "APP_NAME=docs\n".to_owned()),
        ];
        assert_eq!(uses(&keys, &projects), vec!["shop.test".to_owned()]);
        assert!(uses(&[], &projects).is_empty(), "no keys, no users");
    }

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
        // A port another program holds, for the asked-for case.
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let held_port = held.local_addr().unwrap().port();
        let others = BTreeMap::from([
            (1025, "mailpit-1".to_owned()),
            (8025, "mailpit-1".to_owned()),
        ]);
        let chosen = choose_ports(mailpit, &BTreeMap::new(), &others).unwrap();
        assert_ne!(chosen["smtp"], 1025, "taken by another instance");
        assert_ne!(chosen["http"], 8025, "taken by another instance");
        assert_ne!(chosen["smtp"], chosen["http"]);

        let asked = BTreeMap::from([("smtp".into(), held_port)]);
        assert!(
            choose_ports(mailpit, &asked, &BTreeMap::new()).is_err(),
            "port in use"
        );
        let asked = BTreeMap::from([("smtp".into(), 1025)]);
        let err = choose_ports(mailpit, &asked, &others).unwrap_err();
        assert!(err.contains("Kiln's mailpit-1"), "{err}");
        let asked = BTreeMap::from([("imap".into(), 1143)]);
        assert!(
            choose_ports(mailpit, &asked, &BTreeMap::new()).is_err(),
            "no such role"
        );
        drop(held);
    }
}
