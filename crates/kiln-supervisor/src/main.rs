//! `kiln-supervisor --dir <run dir>`: keeps the php-cgi workers listed in `<run dir>/pools.json`
//! running until that file is removed ([ADR 0014]). Started and stopped by `kiln web`.
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use kiln_supervisor::handle::{ERRORS_FILE, LOG_FILE, POOLS_FILE};
use kiln_supervisor::launcher::PhpCgiLauncher;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let dir = match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(dir), None) if flag == "--dir" => PathBuf::from(dir),
        _ => {
            eprintln!(
                "usage: kiln-supervisor --dir <run directory>  (started by `kiln web start`)"
            );
            return ExitCode::from(2);
        }
    };
    let launcher = match PhpCgiLauncher::new(&dir.join(ERRORS_FILE)) {
        Ok(launcher) => launcher,
        Err(e) => {
            eprintln!("kiln-supervisor: {e}");
            return ExitCode::FAILURE;
        }
    };
    let never = AtomicBool::new(false);
    match kiln_supervisor::run::run(&dir.join(POOLS_FILE), &dir.join(LOG_FILE), launcher, &never) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kiln-supervisor: {e}");
            ExitCode::FAILURE
        }
    }
}
