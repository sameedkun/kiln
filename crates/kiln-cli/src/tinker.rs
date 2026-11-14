//! `kiln tinker <site> <code>` ([ADR 0079]): PHP run in a Laravel site's app, as `php artisan
//! tinker --execute` runs it, with the PHP Kiln picks for the site's folder. The code is the
//! user's own, run in their own project.
//!
//! [ADR 0079]: ../../../docs/adr/0079-snapshots-and-tinker.md

use std::process::{Command, Stdio};

/// The most output kept from one run, so a loop that prints forever can't fill memory.
const MOST: usize = 256 * 1024;

/// `kiln tinker <site> <code>`: runs it, prints its output, fails with its exit code.
pub fn run(site: &str, code: &str) -> Result<(), String> {
    if code.trim().is_empty() {
        return Err("give the PHP to run, e.g. `kiln tinker shop \"User::count()\"`".into());
    }
    let store = crate::web::open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let host = crate::web::host_name_under(site, &tld)?;
    let found = crate::web::resolved(&store)?
        .sites
        .into_iter()
        .find(|s| s.host == host && s.alias_of.is_none())
        .ok_or_else(|| format!("Kiln doesn't serve {host}"))?;
    if found.driver != kiln_sites::Driver::Laravel {
        return Err(format!("{host} isn't a Laravel app: Tinker is Laravel's"));
    }
    let php = kiln_runtime::php_for_dir(&store, &found.project)
        .map_err(|e| e.to_string())?
        .php
        .php
        .ok_or_else(|| format!("Kiln has no PHP for {host}: `kiln php install` first"))?;
    drop(store);
    let mut command = Command::new(php.dir.join(kiln_php::PHP_EXE));
    command
        .args(["artisan", "tinker", "--execute", code])
        .current_dir(&found.project)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot run php artisan tinker: {e}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if text.len() > MOST {
        let cut = text.floor_char_boundary(MOST);
        text.truncate(cut);
        text.push_str("\n… (cut off)");
    }
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "host": host.as_str(),
            "output": text,
            "exit": output.status.code(),
        }));
    } else {
        print!("{text}");
    }
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "tinker ended with exit code {}",
            output.status.code().unwrap_or(1)
        ))
    }
}
