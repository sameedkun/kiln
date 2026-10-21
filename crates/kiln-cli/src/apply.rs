//! Commands that change the system: `kiln hosts set|clear` and `kiln undo`. Every change goes
//! through the operation engine with the SQLite journal, after a preview and a confirmation.

use std::io::{BufRead, Write};
use std::net::{IpAddr, Ipv4Addr};

use kiln_core::{HostName, Tld};
use kiln_platform::DnsEntry;

/// Parses site names (with or without the TLD) into validated loopback entries.
pub fn site_entries(names: &[String], tld: &str) -> Result<(Tld, Vec<DnsEntry>), String> {
    let tld = Tld::new(tld).map_err(|e| e.to_string())?;
    let entries = names
        .iter()
        .map(|name| {
            let fqdn = if name.ends_with(&format!(".{tld}")) {
                name.clone()
            } else {
                format!("{name}.{tld}")
            };
            HostName::new(&fqdn, &tld).map(|name| DnsEntry {
                name,
                ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok((tld, entries))
}

/// Asks `question [y/N]` on the terminal; anything but y/yes (including no input) is "no".
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows commands change the system so far
pub fn confirm(question: &str) -> bool {
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;
    use std::rc::Rc;

    use kiln_core::Tld;
    use kiln_helper::session::{HelperSession, default_helper_exe};
    use kiln_ops::{Engine, Operation, Registry};
    use kiln_platform::DnsEntry;
    use kiln_platform::hosts_op::{HostsBlockOp, PrivilegedHosts, register_hosts_block};
    use kiln_state::StateStore;

    use super::confirm;

    pub struct Context {
        pub engine: Engine<StateStore>,
        pub session: Rc<HelperSession>,
        pub hosts_path: PathBuf,
    }

    /// Opens the journal, wires the hosts operation to a lazily launched helper, and rolls
    /// back anything a previous run left half-done.
    pub fn open() -> Result<Context, String> {
        let dir = kiln_state::default_state_dir().ok_or("cannot determine the state directory")?;
        let (store, report) = StateStore::open_dir(&dir).map_err(|e| e.to_string())?;
        if let Some(import) = report.imported {
            println!(
                "Imported {} journal events from the previous format.",
                import.events
            );
        }
        let hosts_path = kiln_sys::win::system_directory()
            .map_err(|e| e.to_string())?
            .join(r"drivers\etc\hosts");
        let helper = default_helper_exe().map_err(|e| e.to_string())?;
        if !helper.is_file() {
            return Err(format!(
                "{} not found; build or install kiln-helper next to kiln",
                helper.display()
            ));
        }
        let session = Rc::new(HelperSession::launching(helper));
        let writer: Rc<dyn PrivilegedHosts> = session.clone();
        let mut registry = Registry::default();
        register_hosts_block(&mut registry, hosts_path.clone(), writer);
        let mut engine = Engine::new(store, registry);
        let recovery = engine.recover().map_err(|e| e.to_string())?;
        if !recovery.rolled_back.is_empty() {
            println!(
                "Rolled back interrupted operations: {:?}",
                recovery.rolled_back
            );
        }
        for (id, error) in &recovery.stuck {
            println!("Operation #{id} could not be rolled back and needs attention: {error}");
        }
        Ok(Context {
            engine,
            session,
            hosts_path,
        })
    }

    fn report_elevation(session: &HelperSession) {
        match session.launches() {
            0 => println!("No administrator approval was needed."),
            n => println!("Administrator approval used: {n} prompt(s)."),
        }
    }

    pub fn hosts_set(tld: Tld, entries: Vec<DnsEntry>, yes: bool) -> Result<(), String> {
        let mut ctx = open()?;
        let writer: Rc<dyn PrivilegedHosts> = ctx.session.clone();
        let op: Box<dyn Operation> = Box::new(HostsBlockOp::new(
            ctx.hosts_path.clone(),
            writer,
            tld,
            entries,
        ));
        let preview = op.preview().map_err(|e| e.to_string())?;
        println!("{}", preview.summary);
        for change in &preview.changes {
            println!("  in {}:", change.target);
            println!(
                "    before: {}",
                change
                    .before
                    .as_deref()
                    .unwrap_or("(no Kiln block)")
                    .replace('\n', "\n            ")
            );
            println!(
                "    after:  {}",
                change
                    .after
                    .as_deref()
                    .unwrap_or("(no Kiln block)")
                    .replace('\n', "\n            ")
            );
        }
        if !yes
            && !confirm("Apply this change? Windows will ask for administrator approval if needed.")
        {
            println!("Nothing was changed.");
            return Ok(());
        }
        let report = ctx.engine.apply(&[op]).map_err(|e| e.to_string())?;
        ctx.session.finish().map_err(|e| e.to_string())?;
        println!(
            "Done. Recorded as operation #{} (undo with `kiln undo {}`).",
            report.entry_ids[0], report.entry_ids[0]
        );
        report_elevation(&ctx.session);
        Ok(())
    }

    pub fn undo(id: u64, force: bool, yes: bool) -> Result<(), String> {
        let mut ctx = open()?;
        let entry = kiln_ops::Journal::entries(ctx.engine.journal())
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("no operation #{id} in the journal"))?;
        println!(
            "Undo operation #{id} ({}, currently {:?}).",
            entry.kind, entry.state
        );
        if !yes && !confirm("Undo it? Windows will ask for administrator approval if needed.") {
            println!("Nothing was changed.");
            return Ok(());
        }
        ctx.engine.revert(id, force).map_err(|e| e.to_string())?;
        ctx.session.finish().map_err(|e| e.to_string())?;
        println!("Undone.");
        report_elevation(&ctx.session);
        Ok(())
    }
}

#[cfg(windows)]
pub use windows::{hosts_set, undo};

#[cfg(not(windows))]
pub fn hosts_set(_tld: Tld, _entries: Vec<DnsEntry>, _yes: bool) -> Result<(), String> {
    Err("changing the hosts file is not available on this platform yet (Linux: P1-04)".into())
}

#[cfg(not(windows))]
pub fn undo(_id: u64, _force: bool, _yes: bool) -> Result<(), String> {
    Err("undo is not available on this platform yet (Linux: P1-04)".into())
}
