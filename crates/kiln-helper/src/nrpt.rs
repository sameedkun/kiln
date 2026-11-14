//! The helper's half of names without the hosts file ([ADR 0078]): Kiln's one Name Resolution
//! Policy rule for its TLD, added or removed. The address (127.0.0.1) and the marker are
//! constants here; the request carries only the TLD, validated again. A rule for the TLD
//! without Kiln's marker is another program's: Kiln never adds beside it or removes it.
//!
//! [ADR 0078]: ../../../docs/adr/0078-names-without-the-hosts-file.md

use kiln_core::Tld;

use crate::proto::ErrorCode;

/// A failure, as the protocol reports it.
#[derive(Debug)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
}

/// The script that adds Kiln's rule for `.tld` unless one is there; exit 0 (added), 1 (already
/// there), 3 (another program's rule). `tld` is a validated [`Tld`]: letters, digits, hyphens.
#[must_use]
pub fn add_script(tld: &Tld) -> String {
    let marker = kiln_platform::nrpt_op::MARKER;
    format!(
        "$ns = '.{tld}'; \
         $rules = @(Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -contains $ns }}); \
         if ($rules | Where-Object {{ $_.Comment -ne '{marker}' }}) {{ exit 3 }}; \
         if ($rules.Count) {{ exit 1 }}; \
         Add-DnsClientNrptRule -Namespace $ns -NameServers '127.0.0.1' -Comment '{marker}' | Out-Null; \
         Clear-DnsClientCache; exit 0"
    )
}

/// The script that removes Kiln's rule (its marker) for `.tld`; exit 0 (removed), 1 (none).
#[must_use]
pub fn remove_script(tld: &Tld) -> String {
    let marker = kiln_platform::nrpt_op::MARKER;
    format!(
        "$ns = '.{tld}'; \
         $ours = @(Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -contains $ns -and $_.Comment -eq '{marker}' }}); \
         if (-not $ours.Count) {{ exit 1 }}; \
         $ours | ForEach-Object {{ Remove-DnsClientNrptRule -Name $_.Name -Force }}; \
         Clear-DnsClientCache; exit 0"
    )
}

/// Kiln's rule for `tld` on or off; whether anything changed.
pub fn set(tld: &str, on: bool) -> Result<bool, Failure> {
    let tld = Tld::recorded(tld).map_err(|e| Failure {
        code: ErrorCode::InvalidRequest,
        message: e.to_string(),
    })?;
    let script = if on {
        add_script(&tld)
    } else {
        remove_script(&tld)
    };
    run(&script, &tld, on)
}

#[cfg(windows)]
fn run(script: &str, tld: &Tld, on: bool) -> Result<bool, Failure> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let exe = std::path::Path::new(&root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let output = std::process::Command::new(exe)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| Failure {
            code: ErrorCode::Io,
            message: format!("cannot run PowerShell: {e}"),
        })?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        Some(3) if on => Err(Failure {
            code: ErrorCode::InvalidRequest,
            message: format!(
                "another program has a name resolution rule for .{tld}; Kiln leaves it"
            ),
        }),
        _ => Err(Failure {
            code: ErrorCode::Io,
            message: format!(
                "PowerShell failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }),
    }
}

#[cfg(not(windows))]
fn run(_script: &str, _tld: &Tld, _on: bool) -> Result<bool, Failure> {
    Err(Failure {
        code: ErrorCode::InvalidRequest,
        message: "name resolution rules are Windows' (ADR 0078)".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scripts_name_only_kilns_rule_and_loopback() {
        let tld = Tld::default();
        let add = add_script(&tld);
        assert!(add.contains("$ns = '.test'"));
        assert!(add.contains("-NameServers '127.0.0.1'"));
        assert!(add.contains("-Comment 'Kiln (ADR 0078)'"));
        assert!(
            add.contains("exit 3"),
            "refuses beside another program's rule"
        );
        let remove = remove_script(&tld);
        assert!(
            remove.contains("$_.Comment -eq 'Kiln (ADR 0078)'"),
            "removes only Kiln's"
        );
    }

    #[test]
    fn a_tld_that_isnt_one_is_refused_before_anything_runs() {
        let err = set("test'; Remove-Item C:\\ -Recurse; '", true).unwrap_err();
        assert!(matches!(err.code, ErrorCode::InvalidRequest));
    }
}
