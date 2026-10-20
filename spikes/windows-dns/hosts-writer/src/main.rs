//! Spike 02 throwaway tool. Usage:
//!   hosts-writer apply  <file> <strategy> <name> [name...]   strategy: replacefile | rename | inplace
//!   hosts-writer remove <file> <strategy>
//!   hosts-writer read   <file>
//! Prints one `key=value` line per fact so the PowerShell harness can parse it.

use std::net::{IpAddr, Ipv4Addr};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;

use kiln_core::{HostName, Tld};
use kiln_platform::{DnsEntry, hosts};

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Writes `bytes` to `path` with the given strategy.
/// - replacefile: temp file + ReplaceFileW (keeps the original's ACL, attributes, streams)
/// - rename: temp file + MoveFileEx(REPLACE_EXISTING) via std::fs::rename (temp's ACL wins)
/// - inplace: truncate + write (not atomic; baseline)
fn write(path: &Path, bytes: &[u8], strategy: &str) -> Result<(), String> {
    match strategy {
        "inplace" => std::fs::write(path, bytes).map_err(|e| format!("inplace: {e}")),
        "rename" | "replacefile" => {
            let tmp = path.with_extension("kiln-tmp");
            std::fs::write(&tmp, bytes).map_err(|e| format!("temp write: {e}"))?;
            if strategy == "rename" {
                return std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"));
            }
            let (dst, src) = (wide(path), wide(&tmp));
            // SAFETY: both buffers are NUL-terminated UTF-16 paths that outlive the call.
            let ok = unsafe {
                windows_sys::Win32::Storage::FileSystem::ReplaceFileW(
                    dst.as_ptr(),
                    src.as_ptr(),
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            if ok == 0 {
                let err = std::io::Error::last_os_error();
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("ReplaceFileW: {err}"));
            }
            Ok(())
        }
        other => Err(format!("unknown strategy {other}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = run(&args);
    if let Err(e) = result {
        println!("error={e}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let cmd = args.first().ok_or("missing command")?;
    let file = Path::new(args.get(1).ok_or("missing file")?);
    let text = std::fs::read(file).map_err(|e| format!("read: {e}"))?;
    println!(
        "utf8={} bytes_before={}",
        std::str::from_utf8(&text).is_ok(),
        text.len()
    );
    match cmd.as_str() {
        "read" => {
            for (ip, name) in hosts::read_block(&text).map_err(|e| e.to_string())? {
                println!("entry={ip} {name}");
            }
            Ok(())
        }
        "apply" | "remove" => {
            let strategy = args.get(2).ok_or("missing strategy")?;
            let tld = Tld::default();
            let entries = args
                .iter()
                .skip(3)
                .map(|n| {
                    HostName::new(n, &tld)
                        .map(|name| DnsEntry { name, ip: IpAddr::V4(Ipv4Addr::LOCALHOST) })
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let updated = if cmd == "apply" {
                hosts::upsert_block(&text, &entries)
            } else {
                hosts::remove_block(&text)
            }
            .map_err(|e| e.to_string())?;
            let start = Instant::now();
            write(file, &updated, strategy)?;
            println!("written_ms={} bytes_after={}", start.elapsed().as_millis(), updated.len());
            Ok(())
        }
        other => Err(format!("unknown command {other}")),
    }
}
