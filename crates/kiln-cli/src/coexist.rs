//! Living next to other local PHP tools ([ADR 0019]): free ports for `kiln web start`, notes
//! when a site name is also another tool's, and the facts `kiln doctor` reports.
//!
//! Read-only towards other tools: nothing here stops, edits or reconfigures them.
//!
//! [ADR 0019]: ../../../docs/adr/0019-coexistence.md

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::path::PathBuf;

use kiln_coexist::{Claim, Holder};
use kiln_state::StateStore;

use crate::web;

/// One of the web server's two ports.
#[derive(Debug)]
pub(crate) struct PortKind {
    pub purpose: &'static str,
    pub key: &'static str,
    /// "1": Kiln picks this port; "0": the user chose it. Unset: picked, unless a port was
    /// saved before automatic ports existed (that one was the user's choice).
    auto_key: &'static str,
    flag: &'static str,
    pub preferred: &'static [u16],
}

pub(crate) const HTTP: PortKind = PortKind {
    purpose: "HTTP",
    key: web::HTTP_PORT_KEY,
    auto_key: "web.http_port_auto",
    flag: "--http-port",
    preferred: &kiln_coexist::HTTP_PORTS,
};

pub(crate) const HTTPS: PortKind = PortKind {
    purpose: "HTTPS",
    key: web::HTTPS_PORT_KEY,
    auto_key: "web.https_port_auto",
    flag: "--https-port",
    preferred: &kiln_coexist::HTTPS_PORTS,
};

/// The TCP listeners on this computer, with their PIDs.
#[derive(Debug, Default)]
pub(crate) struct Listeners(Vec<(SocketAddr, u32)>);

impl Listeners {
    #[cfg(windows)]
    pub fn now() -> Self {
        Self(kiln_sys::win::tcp_listeners().unwrap_or_default())
    }

    #[cfg(not(windows))]
    pub fn now() -> Self {
        Self::default()
    }

    pub fn all(&self) -> &[(SocketAddr, u32)] {
        &self.0
    }

    /// Who listens on `port` on a loopback or wildcard address, if anyone does.
    pub fn holder(&self, port: u16) -> Option<Holder> {
        self.0
            .iter()
            .find(|(addr, _)| {
                addr.port() == port && (addr.ip().is_loopback() || addr.ip().is_unspecified())
            })
            .map(|(_, pid)| holder(*pid))
    }
}

/// A process, with its program path where it can be inspected.
#[cfg(windows)]
pub(crate) fn holder(pid: u32) -> Holder {
    let image = kiln_sys::win::Process::open_for_query(pid)
        .ok()
        .flatten()
        .and_then(|p| p.image_path().ok());
    Holder::new(pid, image)
}

#[cfg(not(windows))]
pub(crate) fn holder(pid: u32) -> Holder {
    Holder::new(pid, None)
}

/// Whether Kiln could listen on `port`: on `127.0.0.1` and, where IPv6 works, on `::1`.
pub(crate) fn is_free(port: u16) -> bool {
    let ipv6 = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok();
    TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
        && (!ipv6 || TcpListener::bind((Ipv6Addr::LOCALHOST, port)).is_ok())
}

/// "used by Laravel Herd (nginx.exe)", or why else it can't be used.
fn why_busy(listeners: &Listeners, port: u16) -> String {
    match listeners.holder(port) {
        Some(holder) => format!("used by {}", holder.describe()),
        None => match first_unprivileged_port() {
            Some(first) if port < first => {
                format!("only for root on this system (ports below {first})")
            }
            // Nobody listens, but binding fails: e.g. a range Windows reserves (Hyper-V, WSL).
            _ => "not available (reserved, or used by another program)".to_owned(),
        },
    }
}

/// The lowest port a normal user may listen on: Linux reserves those below it for root
/// (1024 unless changed). `None` where every port is open to users (Windows, macOS).
fn first_unprivileged_port() -> Option<u16> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::fs::read_to_string("/proc/sys/net/ipv4/ip_unprivileged_port_start")
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .or(Some(1024))
        .filter(|&first| first > 0)
}

/// Decides the port `kind` uses for this start and saves it ([ADR 0019] §3). `chosen` is the
/// command-line port; `needed` is false for HTTPS without secure sites, where a busy port is
/// no reason to fail.
///
/// [ADR 0019]: ../../../docs/adr/0019-coexistence.md
pub(crate) fn choose_port(
    store: &mut StateStore,
    kind: &PortKind,
    chosen: Option<u16>,
    auto_ports: bool,
    needed: bool,
    listeners: &Listeners,
) -> Result<u16, String> {
    let auto = match chosen {
        Some(_) => false,
        None if auto_ports => true,
        None => is_auto(store, kind)?,
    };
    let current = match chosen {
        Some(port) => port,
        None => web::port_setting(store, kind.key, kind.preferred[0])?,
    };
    // The port and who picked it, saved together (I-53); nothing is saved when the start is
    // refused, so a busy port given by mistake isn't remembered.
    let save = |store: &mut StateStore, port: u16| -> Result<u16, String> {
        store
            .set_setting(kind.key, &port.to_string())
            .and_then(|()| store.set_setting(kind.auto_key, if auto { "1" } else { "0" }))
            .map_err(|e| e.to_string())?;
        Ok(port)
    };
    if is_free(current) {
        return save(store, current);
    }
    let alternative = kiln_coexist::pick(kind.preferred, |p| p != current && is_free(p));
    if !auto {
        if !needed {
            return save(store, current);
        }
        let suggestion = alternative.map_or_else(
            || format!("kiln web start {} <port>", kind.flag),
            |p| format!("kiln web start {} {p}", kind.flag),
        );
        return Err(format!(
            "{} port {current} (your choice) is {}. Kiln doesn't move a port you chose; pick another ({suggestion}) or let Kiln choose (kiln web start --auto-ports).",
            kind.purpose,
            why_busy(listeners, current),
        ));
    }
    match alternative {
        Some(port) => {
            println!(
                "{} port {current} is {}; Kiln uses {port}.",
                kind.purpose,
                why_busy(listeners, current)
            );
            save(store, port)
        }
        None if !needed => save(store, current),
        None => Err(format!(
            "no free {} port: {current} is {}, and {} are all busy; choose one with kiln web start {} <port>",
            kind.purpose,
            why_busy(listeners, current),
            kind.preferred
                .iter()
                .filter(|&&p| p != current)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            kind.flag
        )),
    }
}

/// Names other programs put in the hosts file (empty if it can't be read).
pub(crate) fn hosts_claims() -> Vec<Claim> {
    std::fs::read(crate::system_hosts_path())
        .map(|bytes| kiln_coexist::hosts_claims(&bytes))
        .unwrap_or_default()
}

/// Of `names`, those another program also claims in the hosts file, with its claim.
pub(crate) fn shared_names(names: &[String], claims: &[Claim]) -> Vec<(String, Claim)> {
    names
        .iter()
        .filter_map(|name| {
            claims
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(name))
                .map(|c| (name.clone(), c.clone()))
        })
        .collect()
}

/// After `kiln link` / `kiln park`: says which of `names` another tool also uses, and which
/// URL opens Kiln's site ([ADR 0019] §4).
///
/// [ADR 0019]: ../../../docs/adr/0019-coexistence.md
pub(crate) fn note_shared_names(store: &StateStore, names: &[String]) {
    let shared = shared_names(names, &hosts_claims());
    if shared.is_empty() {
        return;
    }
    let urls = web::site_urls(store);
    for (name, claim) in shared {
        let url = urls.get(&name).map_or("", String::as_str);
        println!(
            "Note: {name} is also in the hosts file for {}. Both point to this computer, so the port decides which server answers; Kiln's is {url}",
            claim.owner()
        );
    }
}

/// `php` on `PATH`, if it belongs to another tool.
pub(crate) fn php_from_tool(
    php_on_path: Option<&PathBuf>,
) -> Option<(kiln_coexist::Tool, PathBuf)> {
    let php = php_on_path?;
    kiln_coexist::tool_of(php).map(|tool| (tool, php.clone()))
}

/// The port `kind` uses: the saved one or, before the first `kiln web start`, the one it would
/// pick now, so URLs printed before that start are the ones it will serve.
pub(crate) fn planned_port(store: &StateStore, kind: &PortKind) -> Result<u16, String> {
    if store
        .setting(kind.key)
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return web::port_setting(store, kind.key, kind.preferred[0]);
    }
    Ok(kiln_coexist::pick(kind.preferred, is_free).unwrap_or(kind.preferred[0]))
}

/// Whether Kiln picks `kind`'s port itself (see [`PortKind`]'s `auto_key`).
pub(crate) fn is_auto(store: &StateStore, kind: &PortKind) -> Result<bool, String> {
    Ok(
        match store.setting(kind.auto_key).map_err(|e| e.to_string())? {
            Some(flag) => flag == "1",
            None => store
                .setting(kind.key)
                .map_err(|e| e.to_string())?
                .is_none(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A port something else holds, and a kind that prefers it over a free one.
    fn setup() -> (
        tempfile::TempDir,
        StateStore,
        TcpListener,
        PortKind,
        u16,
        u16,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = StateStore::open_dir(dir.path()).unwrap();
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let busy = held.local_addr().unwrap().port();
        let free = (0..20)
            .filter_map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok())
            .map(|l| l.local_addr().unwrap().port())
            .find(|&p| p != busy && is_free(p))
            .expect("a free port");
        let preferred: &'static [u16] = Box::leak(vec![busy, free].into_boxed_slice());
        let kind = PortKind { preferred, ..HTTP };
        (dir, store, held, kind, busy, free)
    }

    #[test]
    fn a_busy_port_is_replaced_by_the_next_free_one_and_remembered() {
        let (_dir, mut store, _held, kind, _busy, free) = setup();
        let none = Listeners::default();
        assert_eq!(
            choose_port(&mut store, &kind, None, false, true, &none),
            Ok(free)
        );
        assert_eq!(store.setting(kind.key).unwrap(), Some(free.to_string()));
        assert!(is_auto(&store, &kind).unwrap());
        // Still free next time: the same port, so URLs don't change.
        assert_eq!(
            choose_port(&mut store, &kind, None, false, true, &none),
            Ok(free)
        );
    }

    #[test]
    fn a_chosen_port_is_kept_and_a_conflict_suggests_a_free_one() {
        let (_dir, mut store, _held, kind, busy, free) = setup();
        let none = Listeners::default();
        let err = choose_port(&mut store, &kind, Some(busy), false, true, &none).unwrap_err();
        assert!(err.contains("doesn't move a port you chose"), "{err}");
        assert!(err.contains(&format!("--http-port {free}")), "{err}");
        assert!(err.contains("--auto-ports"), "{err}");
        assert_eq!(
            store.setting(kind.key).unwrap(),
            None,
            "refused: nothing saved"
        );
        // Not needed (HTTPS without secure sites): no reason to fail, and it is remembered.
        assert_eq!(
            choose_port(&mut store, &kind, Some(busy), false, false, &none),
            Ok(busy)
        );
        assert!(!is_auto(&store, &kind).unwrap());
        assert!(choose_port(&mut store, &kind, None, false, true, &none).is_err());
        // --auto-ports hands the port back to Kiln.
        assert_eq!(
            choose_port(&mut store, &kind, None, true, true, &none),
            Ok(free)
        );
    }

    #[test]
    fn a_port_saved_before_automatic_ports_counts_as_chosen() {
        let (_dir, mut store, _held, kind, busy, _free) = setup();
        store.set_setting(kind.key, &busy.to_string()).unwrap();
        assert!(!is_auto(&store, &kind).unwrap());
        assert!(choose_port(&mut store, &kind, None, false, true, &Listeners::default()).is_err());
    }

    #[test]
    fn urls_before_the_first_start_use_the_port_it_will_pick() {
        let (_dir, mut store, _held, kind, busy, free) = setup();
        assert_eq!(
            planned_port(&store, &kind),
            Ok(free),
            "nothing saved: the pick"
        );
        store.set_setting(kind.key, &busy.to_string()).unwrap();
        assert_eq!(planned_port(&store, &kind), Ok(busy), "saved: that one");
    }

    #[test]
    fn shared_names_match_without_case() {
        let claims = kiln_coexist::hosts_claims(
            b"# Herd generated Hosts. Do not change.\n127.0.0.1 blog.test\n# End Herd generated Hosts\n",
        );
        let shared = shared_names(&["Blog.test".into(), "shop.test".into()], &claims);
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].0, "Blog.test");
        assert_eq!(shared[0].1.owner(), "Laravel Herd");
    }
}
