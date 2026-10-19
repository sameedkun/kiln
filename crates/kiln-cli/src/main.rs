//! `kiln` CLI. Phase 0 only exposes read-only previews; nothing here writes to the system.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kiln_core::{HostName, Tld};
use kiln_platform::{DnsEntry, hosts};

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
}

#[derive(Debug, Subcommand)]
enum PlanTarget {
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

fn system_hosts_path() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32\\drivers\\etc\\hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

fn plan_hosts(names: &[String], tld: &str, file: Option<PathBuf>) -> Result<(), String> {
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

    let path = file.unwrap_or_else(system_hosts_path);
    let current = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
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
    println!("Nothing was written. Applying arrives with the privileged helper in Phase 1.");
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Plan {
            target: PlanTarget::Hosts { names, tld, file },
        } => plan_hosts(&names, &tld, file),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
