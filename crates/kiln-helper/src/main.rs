//! `kiln-helper` binary: started elevated by `kiln`, serves one authenticated session.
//!
//! Usage (internal):
//! - Windows: `kiln-helper --pipe <name> --client-pid <pid> --key <64 hex chars>` (ADR 0009)
//! - Linux: `pkexec kiln-helper --stdio`; the key is the first line on stdin (ADR 0020)
//!
//! Exit codes: 0 session completed, 2 session failed, 3 session deadline passed,
//! 64 bad arguments.

use std::process::ExitCode;

const USAGE: &str =
    "usage: kiln-helper --pipe <name> --client-pid <pid> --key <64 hex chars> (Windows)
       kiln-helper --stdio (Linux; started by kiln through pkexec)";

/// A session must finish within this; a stuck client can't keep a privileged process alive.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Debug, PartialEq, Eq)]
struct Args {
    pipe: String,
    client_pid: u32,
    key: kiln_helper::proto::SessionKey,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut pipe = None;
    let mut pid = None;
    let mut key = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let slot = match flag.as_str() {
            "--pipe" => &mut pipe,
            "--client-pid" => &mut pid,
            "--key" => &mut key,
            other => return Err(format!("unknown argument {other}")),
        };
        if slot.replace(value.clone()).is_some() {
            return Err(format!("{flag} given twice"));
        }
    }
    let pipe = pipe.ok_or("--pipe is required")?;
    if !pipe.starts_with("kiln-helper-") {
        return Err("--pipe must be a kiln-helper pipe name".into());
    }
    let client_pid = pid
        .ok_or("--client-pid is required")?
        .parse()
        .map_err(|_| "--client-pid must be a number")?;
    let key_bytes = kiln_helper::proto::from_hex(&key.ok_or("--key is required")?)
        .ok_or("--key must be hex")?;
    let key = key_bytes
        .try_into()
        .map_err(|_| "--key must be 32 bytes (64 hex chars)")?;
    Ok(Args {
        pipe,
        client_pid,
        key,
    })
}

/// Runs `session` on a thread and waits at most [`DEADLINE`] for it. Exiting ends a blocked
/// session thread with the process.
fn with_deadline<E: std::fmt::Display + Send + 'static>(
    session: impl FnOnce() -> Result<kiln_helper::server::Summary, E> + Send + 'static,
) -> ExitCode {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(session());
    });
    match rx.recv_timeout(DEADLINE) {
        Ok(Ok(_)) => ExitCode::SUCCESS,
        Ok(Err(e)) => {
            eprintln!("kiln-helper: {e}");
            ExitCode::from(2)
        }
        Err(_) => {
            eprintln!("kiln-helper: session deadline passed");
            ExitCode::from(3)
        }
    }
}

#[cfg(windows)]
fn run(args: Args) -> ExitCode {
    // Fixed target from the OS, never from the (inherited) environment: ADR 0009 §10.
    let hosts_path = match kiln_sys::win::system_directory() {
        Ok(dir) => dir.join(r"drivers\etc\hosts"),
        Err(e) => {
            eprintln!("kiln-helper: cannot locate the system directory: {e}");
            return ExitCode::from(2);
        }
    };
    let config = kiln_helper::server::ServerConfig {
        pipe_name: args.pipe,
        client_pid: args.client_pid,
        key: args.key,
        hosts_path,
    };
    with_deadline(move || kiln_helper::server::serve(&config))
}

#[cfg(not(windows))]
fn run(_args: Args) -> ExitCode {
    eprintln!("kiln-helper: on this platform kiln starts it with --stdio (through pkexec)");
    ExitCode::from(64)
}

/// `--stdio` (ADR 0020 §4–5): the session runs on stdin/stdout, and the hosts file is always
/// `/etc/hosts`, whatever the (cleared) environment says. Nothing else may print to stdout.
#[cfg(unix)]
fn run_stdio() -> ExitCode {
    with_deadline(|| {
        kiln_helper::server::serve_stdio(
            std::io::stdin().lock(),
            std::io::stdout(),
            std::path::Path::new("/etc/hosts"),
        )
    })
}

#[cfg(not(unix))]
fn run_stdio() -> ExitCode {
    eprintln!("kiln-helper: --stdio is for Linux; on Windows kiln starts it with --pipe");
    ExitCode::from(64)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--stdio"] {
        return run_stdio();
    }
    match parse(&args) {
        Ok(args) => run(args),
        Err(e) => {
            eprintln!("kiln-helper: {e}\n{USAGE}");
            ExitCode::from(64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_complete_arguments() {
        let key = "ab".repeat(32);
        let args = parse(&strings(&[
            "--pipe",
            "kiln-helper-x",
            "--client-pid",
            "42",
            "--key",
            &key,
        ]))
        .unwrap();
        assert_eq!(args.client_pid, 42);
        assert_eq!(args.key, [0xab; 32]);
    }

    #[test]
    fn rejects_incomplete_duplicate_or_foreign_arguments() {
        let key = "ab".repeat(32);
        for bad in [
            vec!["--pipe", "kiln-helper-x", "--client-pid", "42"],
            vec!["--pipe", "evil", "--client-pid", "42", "--key", &key],
            vec![
                "--pipe",
                "kiln-helper-x",
                "--client-pid",
                "x",
                "--key",
                &key,
            ],
            vec![
                "--pipe",
                "kiln-helper-x",
                "--client-pid",
                "42",
                "--key",
                "abcd",
            ],
            vec![
                "--pipe",
                "kiln-helper-x",
                "--pipe",
                "kiln-helper-y",
                "--client-pid",
                "1",
                "--key",
                &key,
            ],
            vec![
                "--pipe",
                "kiln-helper-x",
                "--client-pid",
                "1",
                "--key",
                &key,
                "--hosts",
                "C:/x",
            ],
        ] {
            assert!(parse(&strings(&bad)).is_err(), "{bad:?}");
        }
    }
}
