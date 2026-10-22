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
    use kiln_platform::trust::{TrustCaOp, register_trust_ca};
    use kiln_platform::user_path::{RegistryPath, UserPathOp, register_user_path};
    use kiln_state::StateStore;

    use super::{ca_store, confirm, print_preview};

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
        register_trust_ca(&mut registry);
        register_user_path(&mut registry, Rc::new(RegistryPath));
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
        print_preview(&op.preview().map_err(|e| e.to_string())?, "(no Kiln block)");
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

    /// `kiln undo [n]` ([ADR 0018] §2): the given operation, or the most recent one still in
    /// effect.
    ///
    /// [ADR 0018]: ../../../docs/adr/0018-history-undo-uninstall.md
    pub fn undo(id: Option<u64>, force: bool, yes: bool) -> Result<(), String> {
        let mut ctx = open()?;
        let entries =
            kiln_ops::Journal::entries(ctx.engine.journal()).map_err(|e| e.to_string())?;
        let entry = match id {
            Some(id) => entries
                .into_iter()
                .find(|e| e.id == id)
                .ok_or_else(|| format!("no operation #{id}; see `kiln history`"))?,
            None => entries
                .into_iter()
                .rev()
                .find(|e| e.state == kiln_ops::OpState::Applied)
                .ok_or("nothing to undo: no operation is in effect (see `kiln history --all`)")?,
        };
        let id = entry.id;
        let what = kiln_platform::describe::describe(&entry.kind, &entry.params);
        if entry.state != kiln_ops::OpState::Applied {
            return Err(format!(
                "operation #{id} ({what}) is {}, so there is nothing to undo",
                super::state_word(entry.state)
            ));
        }
        println!("Undo operation #{id}: {what}.");
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

    /// Undoes every operation in effect, newest first, stopping at the first failure
    /// ([ADR 0018] §4). Returns the undone operation numbers.
    ///
    /// [ADR 0018]: ../../../docs/adr/0018-history-undo-uninstall.md
    pub fn undo_all() -> Result<Vec<u64>, String> {
        let mut ctx = open()?;
        let result = ctx.engine.revert_all(false).map_err(|e| e.to_string());
        ctx.session.finish().map_err(|e| e.to_string())?;
        report_elevation(&ctx.session);
        result
    }

    /// `kiln shims install` ([ADR 0016] §8, §11): copies `kiln-shim` in as `php.exe` and
    /// `composer.exe`, then puts their folder first on the user PATH as a journaled, undoable
    /// operation, after showing which `php` new terminals get now and after.
    ///
    /// [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md
    pub fn shims_install(yes: bool) -> Result<(), String> {
        use kiln_runtime::shim::{SHIMS_DIR, Tool};
        let local = kiln_state::default_local_dir()
            .ok_or("cannot determine Kiln's local data directory")?;
        let shims = local.join(SHIMS_DIR);
        let source = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name(format!("kiln-shim{}", std::env::consts::EXE_SUFFIX));
        if !source.is_file() {
            return Err(format!(
                "{} not found; build or install kiln-shim next to kiln",
                source.display()
            ));
        }
        std::fs::create_dir_all(&shims).map_err(|e| e.to_string())?;
        for tool in [Tool::Php, Tool::Composer] {
            let target = shims.join(tool.file_name());
            std::fs::copy(&source, &target).map_err(|e| {
                format!(
                    "cannot write {} (is a `{}` command still running?): {e}",
                    target.display(),
                    tool.name()
                )
            })?;
        }
        println!("Shims written to {}.", shims.display());

        let path_now = std::env::var("PATH").unwrap_or_default();
        for tool in [Tool::Php, Tool::Composer] {
            match super::which_on(&path_now, tool.name()) {
                Some(found) if found.starts_with(&shims) => {}
                Some(found) => println!(
                    "`{}` in new terminals: now {}; after this, Kiln's shim (it follows each site's PHP).",
                    tool.name(),
                    found.display()
                ),
                None => println!(
                    "`{}` in new terminals: now not found; after this, Kiln's shim.",
                    tool.name()
                ),
            }
        }

        let mut ctx = open()?;
        let op: Box<dyn Operation> = Box::new(UserPathOp::new(&shims, Rc::new(RegistryPath)));
        print_preview(&op.preview().map_err(|e| e.to_string())?, "(empty)");
        if !yes && !confirm("Change your user PATH? (`kiln undo` puts it back)") {
            println!("Nothing was changed on PATH. The shims are in place but not on PATH.");
            return Ok(());
        }
        let report = ctx.engine.apply(&[op]).map_err(|e| e.to_string())?;
        println!(
            "Done. Recorded as operation #{} (undo with `kiln undo {}`).",
            report.entry_ids[0], report.entry_ids[0]
        );
        println!("Open a new terminal to use it; terminals already open keep their old PATH.");
        Ok(())
    }

    /// Trusts Kiln's root for the current Windows user (no administrator rights; Windows
    /// shows its own confirmation dialog).
    pub fn ca_trust(yes: bool) -> Result<(), String> {
        let root = ca_store()?.root().map_err(|e| e.to_string())?;
        let mut ctx = open()?;
        let op: Box<dyn Operation> =
            Box::new(TrustCaOp::new("Root", root.der, root.sha1, root.subject));
        print_preview(&op.preview().map_err(|e| e.to_string())?, "(not trusted)");
        if !yes
            && !confirm(
                "Trust it? Windows will show a security warning naming this certificate; choose Yes there.",
            )
        {
            println!("Nothing was changed.");
            return Ok(());
        }
        let report = ctx.engine.apply(&[op]).map_err(|e| e.to_string())?;
        println!(
            "Trusted. Recorded as operation #{} (remove with `kiln undo {}`).",
            report.entry_ids[0], report.entry_ids[0]
        );
        Ok(())
    }
}

/// Prints a preview's summary and before/after lines.
#[cfg_attr(not(windows), allow(dead_code))] // only the Windows commands change the system so far
pub fn print_preview(preview: &kiln_ops::Preview, empty: &str) {
    println!("{}", preview.summary);
    for change in &preview.changes {
        let indent = |s: &str| s.replace('\n', "\n            ");
        println!("  in {}:", change.target);
        println!(
            "    before: {}",
            indent(change.before.as_deref().unwrap_or(empty))
        );
        println!(
            "    after:  {}",
            indent(change.after.as_deref().unwrap_or(empty))
        );
    }
}

/// The CA directory inside Kiln's state directory, with the platform's key protector.
pub fn ca_store() -> Result<kiln_pki::CaStore, String> {
    let dir = kiln_state::default_state_dir().ok_or("cannot determine the state directory")?;
    Ok(kiln_pki::CaStore::new(
        dir.join("ca"),
        kiln_pki::default_protector(),
    ))
}

fn print_cert(label: &str, info: &kiln_pki::CertInfo) {
    println!("{label}: {}", info.subject);
    println!("  SHA-256: {}", info.sha256);
    println!("  expires: {}", info.not_after.date());
}

/// Creates Kiln's root CA if there is none, and issues a fresh intermediate for the web server.
pub fn ca_init(tld: &str) -> Result<(), String> {
    let tld = Tld::new(tld).map_err(|e| e.to_string())?;
    let ca = ca_store()?;
    let root = if ca.root_exists() {
        println!("A Kiln root CA already exists; keeping it.");
        ca.root().map_err(|e| e.to_string())?
    } else {
        let owner = format!(
            "{}@{}",
            std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "user".into()),
            std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "host".into())
        );
        let root = ca.create_root(&tld, &owner).map_err(|e| e.to_string())?;
        println!("Created a root CA that can only vouch for names under .{tld}.");
        root
    };
    print_cert("Root", &root);
    let inter = ca.issue_intermediate(&tld).map_err(|e| e.to_string())?;
    print_cert("Intermediate (for the web server)", &inter);
    if ca.key_encrypted_at_rest() {
        println!(
            "The root key is encrypted for your Windows account (DPAPI) and never given to the web server."
        );
    } else {
        println!(
            "Warning: the root key is protected by file permissions only on this platform (encryption arrives with P1-04)."
        );
    }
    println!("Next: `kiln ca trust` so browsers accept these certificates.");
    Ok(())
}

/// Shows the CA's certificates and where they live.
pub fn ca_info() -> Result<(), String> {
    let ca = ca_store()?;
    println!("CA directory: {}", ca.dir().display());
    if !ca.root_exists() {
        println!("No root CA yet; run `kiln ca init`.");
        return Ok(());
    }
    print_cert("Root", &ca.root().map_err(|e| e.to_string())?);
    match ca.intermediate().map_err(|e| e.to_string())? {
        Some(i) => print_cert("Intermediate", &i),
        None => println!("No intermediate yet; run `kiln ca init`."),
    }
    println!(
        "Root key: {}",
        if ca.key_encrypted_at_rest() {
            "encrypted (DPAPI)"
        } else {
            "file permissions only"
        }
    );
    Ok(())
}

#[cfg(windows)]
pub use windows::{ca_trust, hosts_set, shims_install, undo, undo_all};

#[cfg(not(windows))]
pub fn ca_trust(_yes: bool) -> Result<(), String> {
    Err("trusting the CA is not available on this platform yet (Linux: P1-04)".into())
}

#[cfg(not(windows))]
pub fn hosts_set(_tld: Tld, _entries: Vec<DnsEntry>, _yes: bool) -> Result<(), String> {
    Err("changing the hosts file is not available on this platform yet (Linux: P1-04)".into())
}

#[cfg(not(windows))]
pub fn undo(_id: Option<u64>, _force: bool, _yes: bool) -> Result<(), String> {
    Err("undo is not available on this platform yet (Linux: P1-04)".into())
}

#[cfg(not(windows))]
pub fn shims_install(_yes: bool) -> Result<(), String> {
    Err("the php/composer shims are only available on Windows so far (Linux: P1-04)".into())
}

/// Where `name` resolves on `path` (a `PATH` value), trying `PATHEXT` like a Windows shell.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn which_on(path: &str, name: &str) -> Option<std::path::PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    path.split(';')
        .filter(|d| !d.trim().is_empty())
        .flat_map(|dir| {
            exts.split(';').map(move |ext| {
                std::path::Path::new(dir.trim()).join(format!("{name}{}", ext.to_ascii_lowercase()))
            })
        })
        .find(|candidate| candidate.is_file())
}

/// How a journal state reads in a sentence.
pub(crate) fn state_word(state: kiln_ops::OpState) -> &'static str {
    use kiln_ops::OpState;
    match state {
        OpState::Prepared => "half-done (interrupted)",
        OpState::Applied => "in effect",
        OpState::RolledBack => "rolled back",
        OpState::Reverted => "undone",
        OpState::Stuck => "stuck (needs attention)",
    }
}

#[cfg(not(windows))]
pub fn undo_all() -> Result<Vec<u64>, String> {
    Err("undoing system changes is not available on this platform yet (Linux: P1-04)".into())
}
