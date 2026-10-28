//! `php.ini` for an installed runtime ([ADR 0013] §11): the bundled `php.ini-development`, plus
//! a Kiln section that sets `extension_dir` and enables the extensions most frameworks need.
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

use std::fmt::Write as _;

/// Extensions enabled when the build ships them (`ext/php_<name>.dll`).
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "curl",
    "fileinfo",
    "gd",
    "intl",
    "mbstring",
    "exif",
    // WordPress refuses to run without it (I-62).
    "mysqli",
    "openssl",
    "pdo_mysql",
    "pdo_pgsql",
    "pdo_sqlite",
    "sqlite3",
    "sodium",
    "zip",
];

pub const KILN_MARKER: &str = "; ---- Kiln defaults (generated; edit freely) ----";

/// Limits for local development, the same on every platform. PHP's own development defaults
/// (30 s, 128 MB) are tight for Composer and for a first `WordPress` or Laravel request while
/// Windows Defender scans every file it loads (I-65). Later lines override earlier ones, so
/// these win over the bundled `php.ini-development`.
pub const LIMITS: &str = "memory_limit = 512M\n\
                          max_execution_time = 60\n\
                          upload_max_filesize = 64M\n\
                          post_max_size = 64M\n";

/// Whether `ini` already enables `name` with an uncommented `extension=` line.
fn enables(ini: &str, name: &str) -> bool {
    ini.lines().any(|line| {
        let line = line.trim();
        let Some(value) = line.strip_prefix("extension") else {
            return false;
        };
        let Some(value) = value.trim_start().strip_prefix('=') else {
            return false;
        };
        let value = value.trim().trim_matches('"');
        value == name || value == format!("php_{name}.dll")
    })
}

/// `development_ini` with Kiln's section appended. `shipped(name)` says whether
/// `ext/php_<name>.dll` exists in this build. Returns the text and the extensions enabled.
pub fn render(
    development_ini: &str,
    shipped: impl Fn(&str) -> bool,
) -> (String, Vec<&'static str>) {
    let mut enabled = Vec::new();
    let mut text = development_ini.trim_end().to_owned();
    text.push_str("\n\n");
    text.push_str(KILN_MARKER);
    text.push_str("\nextension_dir = \"ext\"\n");
    text.push_str(LIMITS);
    for &name in DEFAULT_EXTENSIONS {
        if shipped(name) && !enables(development_ini, name) {
            let _ = writeln!(text, "extension={name}");
            enabled.push(name);
        }
    }
    (text, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "[PHP]\r\n;extension=curl\r\n;extension_dir = \"ext\"\r\nextension=mbstring\r\nmemory_limit = 128M\r\n";

    #[test]
    fn appends_a_section_enabling_shipped_extensions_once() {
        let shipped = |name: &str| name != "intl";
        let (ini, enabled) = render(DEV, shipped);
        assert!(ini.starts_with("[PHP]\r\n;extension=curl"), "original kept");
        let section = ini.split(KILN_MARKER).nth(1).unwrap();
        assert!(section.starts_with("\nextension_dir = \"ext\"\n"));
        assert!(
            section.contains("max_execution_time = 60\n")
                && section.contains("memory_limit = 512M\n"),
            "Kiln's limits override the development defaults"
        );
        assert!(section.contains("extension=curl\n"));
        assert!(!section.contains("extension=intl"), "not shipped");
        assert!(
            !section.contains("extension=mbstring"),
            "already enabled upstream"
        );
        assert!(!enabled.contains(&"mbstring") && enabled.contains(&"zip"));
    }

    #[test]
    fn recognises_dll_style_lines() {
        assert!(enables("extension=php_curl.dll", "curl"));
        assert!(enables("extension = \"curl\"", "curl"));
        assert!(!enables(";extension=curl", "curl"));
        assert!(!enables("extension=curlx", "curl"));
    }
}
