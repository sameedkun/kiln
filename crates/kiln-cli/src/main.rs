//! `kiln` CLI: previews (`plan`), state inspection (`state info`), hosts block changes
//! (`hosts set|sync|clear`), the local CA (`ca`), sites and the web server (`link`, `secure`,
//! `web`), PHP runtimes (`php`), and `undo`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kiln_core::Tld;
use kiln_platform::hosts;

mod apply;
mod coexist;
mod doctor;
mod history;
mod php;
mod pool;
mod uninstall;
mod web;

#[derive(Debug, Parser)]
#[command(
    name = "kiln",
    version,
    about = "Local PHP development environment (pre-alpha)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show what a change would do, without applying it.
    Plan {
        #[command(subcommand)]
        target: PlanTarget,
    },
    /// Inspect Kiln's own state database.
    State {
        #[command(subcommand)]
        action: StateAction,
    },
    /// Manage Kiln's block in the system hosts file (Windows; needs administrator approval).
    Hosts {
        #[command(subcommand)]
        action: HostsAction,
    },
    /// Kiln's local certificate authority (only ever valid for the development TLD).
    Ca {
        #[command(subcommand)]
        action: CaAction,
    },
    /// Serve a folder as <name>.test, over plain HTTP unless --secure is given.
    Link {
        /// Site name (default: the folder's name), with or without `.test`.
        name: Option<String>,
        /// Folder to serve (default: the current folder).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Serve it over HTTPS (same as `kiln secure` afterwards).
        #[arg(long)]
        secure: bool,
    },
    /// Stop serving a site. Its files are untouched.
    Unlink { name: String },
    /// Serve every subfolder of a folder as <subfolder>.test (default: the current folder).
    Park { path: Option<PathBuf> },
    /// Stop serving a parked folder's subfolders. Nothing is deleted.
    Unpark { path: Option<PathBuf> },
    /// Turn HTTPS on for a site (HTTP then redirects to it). Needs `kiln ca init` once.
    Secure { name: String },
    /// Turn HTTPS off for a site again; it is served over plain HTTP.
    Unsecure { name: String },
    /// List sites (linked and parked) with their URLs, framework and document root.
    Sites,
    /// Pin a site to a PHP version, e.g. `kiln isolate 8.3` in its folder, or `kiln isolate 8.3 blog`.
    Isolate {
        /// A minor version (newest installed patch) or an exact one, e.g. 8.3 or 8.3.36.
        version: String,
        /// The site (default: the one whose folder contains the current folder).
        site: Option<String>,
    },
    /// Let a site use the default PHP version again.
    Unisolate { site: Option<String> },
    /// Check Kiln's health: state, ports, web server, PHP, sites, DNS, certificates, shims.
    /// Changes nothing; exits with 1 if something failed.
    Doctor {
        /// Print the findings as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show what `php` and `composer` run in a folder (default: the current one), and why.
    Which { path: Option<PathBuf> },
    /// Composer (verified download from getcomposer.org, run through the `composer` shim).
    Composer {
        #[command(subcommand)]
        action: ComposerAction,
    },
    /// The `php` and `composer` commands on your PATH that follow each site's PHP version.
    Shims {
        #[command(subcommand)]
        action: ShimsAction,
    },
    /// PHP runtimes from windows.php.net (verified downloads, into Kiln's own folder).
    Php {
        #[command(subcommand)]
        action: PhpAction,
    },
    /// Kiln's web server (Caddy, running as kiln-caddy).
    Web {
        #[command(subcommand)]
        action: WebAction,
    },
    /// List what Kiln changed on this machine (newest first) and what is still in effect.
    History {
        /// Include operations that were undone or rolled back.
        #[arg(long)]
        all: bool,
    },
    /// Remove Kiln: stop it, undo every system change (newest first), delete its folders.
    /// Always shows the full plan first; your projects and other tools are never touched.
    Uninstall {
        /// Show the plan and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Stop and undo system changes, but keep Kiln's folders (e.g. before reinstalling).
        #[arg(long)]
        keep_files: bool,
        /// Do not ask for confirmation (the plan is still printed).
        #[arg(long, short)]
        yes: bool,
    },
    /// Undo a recorded operation by its number (see `kiln history`), or the newest one.
    Undo {
        /// The operation number (see `kiln history`); default: the newest one still in effect.
        id: Option<u64>,
        /// Undo even if the target was edited by hand since (overwrites those edits).
        #[arg(long)]
        force: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ComposerAction {
    /// Download and verify the current stable Composer.
    Install,
}

#[derive(Debug, Subcommand)]
enum ShimsAction {
    /// Install `php` and `composer` shims and put them first on your user PATH (asks first; undoable).
    Install {
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PhpAction {
    /// List the PHP versions that can be installed (latest patch of each).
    Available,
    /// Download, verify and install a PHP version, e.g. 8.4 (or its exact latest patch).
    Install {
        version: String,
        /// Linux: use the PHP your distribution installed (php8.3 and php-fpm8.3) instead of
        /// downloading one.
        #[arg(long)]
        system: bool,
    },
    /// List the PHP versions Kiln has installed.
    List,
    /// Delete an installed PHP version.
    Remove { version: String },
    /// Choose the PHP version that serves sites (applied live if the server runs).
    Default { version: String },
    /// Set how many PHP workers serve sites per version (1–32, default 4; applied live).
    Workers { count: usize },
}

#[derive(Debug, Subcommand)]
enum WebAction {
    /// Download (or take from --archive) the pinned Caddy release, verify its SHA-512, install it.
    Install {
        /// Use this already downloaded release archive instead of downloading it.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Start the web server in the background.
    Start {
        /// HTTP port, kept even if busy (remembered). Without it Kiln picks a free one:
        /// 80, else 8080, 8000, 8081, 8888.
        #[arg(long)]
        http_port: Option<u16>,
        /// HTTPS port, kept even if busy (remembered). Without it Kiln picks a free one:
        /// 443, else 8443, 4443, 9443. Only used when a site is secure.
        #[arg(long)]
        https_port: Option<u16>,
        /// Forget ports chosen with --http-port/--https-port and pick free ones again.
        #[arg(long, conflicts_with_all = ["http_port", "https_port"])]
        auto_ports: bool,
    },
    /// Stop the web server.
    Stop,
    /// Apply the current sites and settings to the running server without restarting it.
    Reload,
    /// Stop and start the web server (needed after changing ports).
    Restart,
    /// Show whether the web server is installed and running.
    Status,
    /// Print the Caddy config Kiln would run, without starting anything.
    Config,
}

#[derive(Debug, Subcommand)]
enum CaAction {
    /// Create the root CA (if there is none) and issue a fresh intermediate for the web server.
    Init {
        #[arg(long, default_value = "test")]
        tld: String,
    },
    /// Show the CA's certificates, fingerprints and expiry.
    Info,
    /// Trust the root CA for your account: the Windows certificate store (Windows asks you to
    /// confirm), or on Linux each browser's NSS database (Chrome, Firefox).
    Trust {
        /// Do not ask for confirmation in the terminal (Windows still shows its own dialog).
        #[arg(long, short)]
        yes: bool,
        /// Linux: also trust it machine-wide in the system store, for command-line tools
        /// (curl, PHP, Node, git). Asks for an administrator password.
        #[arg(long)]
        system: bool,
    },
}

#[derive(Debug, Subcommand)]
enum HostsAction {
    /// Make Kiln's block contain exactly these site names (replacing its previous content).
    Set {
        /// Site names, with or without the TLD (e.g. `blog` or `blog.test`).
        #[arg(required = true)]
        names: Vec<String>,
        #[arg(long, default_value = "test")]
        tld: String,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Make Kiln's block list exactly the linked and parked sites (and reload the server).
    Sync {
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Remove Kiln's block from the hosts file.
    Clear {
        #[arg(long, default_value = "test")]
        tld: String,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum StateAction {
    /// Where the state database is, its schema version and journal summary.
    Info {
        /// State directory to inspect instead of the per-user default.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

fn state_info(dir: Option<PathBuf>) -> Result<(), String> {
    let dir = dir
        .or_else(kiln_state::default_state_dir)
        .ok_or("cannot determine the state directory (APPDATA/HOME not set)")?;
    let db = dir.join(kiln_state::DB_FILE);
    println!("State directory: {}", dir.display());
    if !db.is_file() {
        println!("No state database yet ({} does not exist).", db.display());
        if dir.join(kiln_state::LEGACY_JOURNAL_FILE).is_file() {
            println!("A legacy journal.jsonl is present; it will be imported on first use.");
        }
        return Ok(());
    }
    let store = kiln_state::StateStore::open(&db).map_err(|e| e.to_string())?;
    println!("Database:        {}", db.display());
    println!(
        "Schema version:  {} (this build: {})",
        store.schema_version().map_err(|e| e.to_string())?,
        kiln_state::SCHEMA_VERSION
    );
    let counts = store.counts_by_state().map_err(|e| e.to_string())?;
    let total: u64 = counts.iter().map(|(_, n)| n).sum();
    println!("Operations:      {total}");
    for (state, n) in counts {
        println!("  {state:?}: {n}");
    }
    Ok(())
}

#[derive(Debug, Subcommand)]
enum PlanTarget {
    /// Show what `kiln uninstall` would do (same as `kiln uninstall --dry-run`).
    Uninstall,
    /// Preview Kiln's managed block in the hosts file for the given site names.
    Hosts {
        /// Site names, with or without the TLD (e.g. `blog` or `blog.test`).
        names: Vec<String>,
        #[arg(long, default_value = "test")]
        tld: String,
        /// Hosts file to read instead of the system one.
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

pub(crate) fn system_hosts_path() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32\\drivers\\etc\\hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

fn plan_hosts(names: &[String], tld: &str, file: Option<PathBuf>) -> Result<(), String> {
    let (_tld, entries) = apply::site_entries(names, tld)?;
    let path = file.unwrap_or_else(system_hosts_path);
    let current =
        std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let before = hosts::read_block(&current).map_err(|e| e.to_string())?;
    let updated = hosts::upsert_block(&current, &entries).map_err(|e| e.to_string())?;
    let after = hosts::read_block(&updated).map_err(|e| e.to_string())?;

    println!("Plan: update Kiln's block in {}", path.display());
    for (ip, name) in &before {
        if !after.contains(&(ip.clone(), name.clone())) {
            println!("  - {ip} {name}");
        }
    }
    for (ip, name) in &after {
        if !before.contains(&(ip.clone(), name.clone())) {
            println!("  + {ip} {name}");
        }
    }
    if before == after {
        println!("  (no changes)");
    }
    let other = hosts::remove_block(&current)
        .map_err(|e| e.to_string())?
        .len();
    println!("Lines outside Kiln's block stay unchanged ({other} bytes of other content).");
    println!("Nothing was written. Use `kiln hosts set` to apply.");
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Command::Doctor { json } = cli.command {
        return if doctor::run(json) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let result = match cli.command {
        Command::Doctor { .. } => unreachable!("handled above"),
        Command::Plan {
            target: PlanTarget::Hosts { names, tld, file },
        } => plan_hosts(&names, &tld, file),
        Command::Plan {
            target: PlanTarget::Uninstall,
        } => uninstall::uninstall(true, false, false),
        Command::History { all } => history::history(all),
        Command::Uninstall {
            dry_run,
            keep_files,
            yes,
        } => uninstall::uninstall(dry_run, keep_files, yes),
        Command::State {
            action: StateAction::Info { dir },
        } => state_info(dir),
        Command::Hosts {
            action: HostsAction::Set { names, tld, yes },
        } => apply::site_entries(&names, &tld)
            .and_then(|(tld, entries)| apply::hosts_set(tld, entries, yes)),
        Command::Hosts {
            action: HostsAction::Clear { tld, yes },
        } => Tld::new(&tld)
            .map_err(|e| e.to_string())
            .and_then(|tld| apply::hosts_set(tld, Vec::new(), yes)),
        Command::Ca {
            action: CaAction::Init { tld },
        } => apply::ca_init(&tld),
        Command::Ca {
            action: CaAction::Info,
        } => apply::ca_info(),
        Command::Ca {
            action: CaAction::Trust { yes, system },
        } => apply::ca_trust(yes, system),
        Command::Hosts {
            action: HostsAction::Sync { yes },
        } => web::hosts_sync(yes),
        Command::Link { name, path, secure } => web::link(name, path, secure),
        Command::Unlink { name } => web::unlink(&name),
        Command::Park { path } => web::park(path),
        Command::Unpark { path } => web::unpark(path),
        Command::Secure { name } => web::set_secure(&name, true),
        Command::Unsecure { name } => web::set_secure(&name, false),
        Command::Sites => web::sites(),
        Command::Isolate { version, site } => php::isolate(&version, site),
        Command::Unisolate { site } => php::unisolate(site),
        Command::Which { path } => php::which(path),
        Command::Composer {
            action: ComposerAction::Install,
        } => php::composer_install(),
        Command::Shims {
            action: ShimsAction::Install { yes },
        } => apply::shims_install(yes),
        Command::Php { action } => match action {
            PhpAction::Available => php::available(),
            PhpAction::Install { version, system } => php::install(&version, system),
            PhpAction::List => php::list(),
            PhpAction::Remove { version } => php::remove(&version),
            PhpAction::Default { version } => php::set_default(&version),
            PhpAction::Workers { count } => php::set_workers(count),
        },
        Command::Web { action } => match action {
            WebAction::Install { archive } => web::install(archive),
            WebAction::Start {
                http_port,
                https_port,
                auto_ports,
            } => web::start(http_port, https_port, auto_ports),
            WebAction::Stop => web::stop(),
            WebAction::Reload => web::reload(),
            WebAction::Restart => web::restart(),
            WebAction::Status => web::status(),
            WebAction::Config => web::print_config(),
        },
        Command::Undo { id, force, yes } => apply::undo(id, force, yes),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
