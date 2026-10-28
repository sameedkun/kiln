//! `kiln.toml`, a project's own settings ([ADR 0027]): read live from the project folder,
//! never copied into Kiln's state.
//!
//! [ADR 0027]: ../../../docs/adr/0027-project-config-kiln-toml.md

use std::fmt::Write as _;
use std::path::Path;

/// The file's name in a project folder.
pub const FILE: &str = "kiln.toml";

/// What a `kiln.toml` asks for. Everything is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// The name `kiln link` and `kiln import herd` use by default.
    pub name: Option<String>,
    /// `Some(true)`: serve over HTTPS. `false` or absent adds nothing.
    pub secure: Option<bool>,
    /// Extra names for the same site, with or without the TLD.
    pub aliases: Vec<String>,
    /// A PHP minor (`8.3`) or exact (`8.3.36`) version.
    pub php: Option<String>,
    /// A Node version (`22`, `22.11`, `22.11.0`), used by the Node shims.
    pub node: Option<String>,
    /// What was ignored, such as unknown keys: shown, not fatal.
    pub warnings: Vec<String>,
}

/// Reads `<project>/kiln.toml`: `Ok(None)` if there is none, `Err` with a message naming the
/// file if it can't be used.
pub fn read(project: &Path) -> Result<Option<Manifest>, String> {
    let path = project.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// `1`, `1.2` or `1.2.3`: dotted numbers, as versions are written.
fn is_version(text: &str, parts: std::ops::RangeInclusive<usize>) -> bool {
    let numbers: Vec<&str> = text.split('.').collect();
    parts.contains(&numbers.len())
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Parses a `kiln.toml`.
pub fn parse(text: &str) -> Result<Manifest, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
        e.message().to_owned()
            + &e.span()
                .map(|s| format!(" (at byte {})", s.start))
                .unwrap_or_default()
    })?;
    let mut manifest = Manifest::default();
    for (section, value) in &table {
        let Some(fields) = value.as_table() else {
            manifest
                .warnings
                .push(format!("ignored `{section}`: expected a [section]"));
            continue;
        };
        for (key, value) in fields {
            let at = format!("[{section}] {key}");
            match (section.as_str(), key.as_str()) {
                ("site", "name") => manifest.name = Some(string(value, &at)?),
                ("site", "secure") => {
                    manifest.secure = Some(
                        value
                            .as_bool()
                            .ok_or_else(|| format!("{at} must be true or false"))?,
                    );
                }
                ("site", "aliases") => {
                    let list = value
                        .as_array()
                        .ok_or_else(|| format!("{at} must be a list of names"))?;
                    manifest.aliases = list
                        .iter()
                        .map(|v| string(v, &at))
                        .collect::<Result<_, _>>()?;
                }
                ("php", "version") => {
                    let version = version(value, &at)?;
                    if !is_version(&version, 2..=3) {
                        return Err(format!(
                            "{at} must be a version like \"8.3\" or \"8.3.36\", not {version:?}"
                        ));
                    }
                    manifest.php = Some(version);
                }
                ("node", "version") => {
                    let version = version(value, &at)?;
                    let trimmed = version.strip_prefix('v').unwrap_or(&version);
                    if !is_version(trimmed, 1..=3) {
                        return Err(format!(
                            "{at} must be a version like \"22\" or \"22.11.0\", not {version:?}"
                        ));
                    }
                    manifest.node = Some(trimmed.to_owned());
                }
                ("site" | "php" | "node", _) => {
                    manifest.warnings.push(format!("ignored unknown key {at}"));
                }
                _ => {
                    manifest
                        .warnings
                        .push(format!("ignored unknown section [{section}]"));
                    break;
                }
            }
        }
    }
    Ok(manifest)
}

fn string(value: &toml::Value, at: &str) -> Result<String, String> {
    value
        .as_str()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{at} must be a non-empty string"))
}

/// A version must be a string: TOML reads `8.10` as the number 8.1.
fn version(value: &toml::Value, at: &str) -> Result<String, String> {
    match value {
        toml::Value::String(s) => Ok(s.trim().to_owned()),
        toml::Value::Float(_) | toml::Value::Integer(_) => Err(format!(
            "{at} must be quoted, like \"8.3\": unquoted it is a number, and 8.10 would read as 8.1"
        )),
        _ => Err(format!("{at} must be a version in quotes, like \"8.3\"")),
    }
}

/// `text` as a TOML basic string: quotes and backslashes escaped, control characters as
/// `\uXXXX` (TOML has no `\u{…}` form).
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The text of a `kiln.toml` holding `manifest`, after `comment` (one line, without `#`).
#[must_use]
pub fn render(manifest: &Manifest, comment: &str) -> String {
    let mut out = format!("# {comment}\n");
    if manifest.name.is_some() || manifest.secure.is_some() || !manifest.aliases.is_empty() {
        out.push_str("\n[site]\n");
        if let Some(name) = &manifest.name {
            let _ = writeln!(out, "name = {}", quoted(name));
        }
        if let Some(secure) = manifest.secure {
            let _ = writeln!(out, "secure = {secure}");
        }
        if !manifest.aliases.is_empty() {
            let list: Vec<String> = manifest.aliases.iter().map(|a| quoted(a)).collect();
            let _ = writeln!(out, "aliases = [{}]", list.join(", "));
        }
    }
    if let Some(php) = &manifest.php {
        let _ = write!(out, "\n[php]\nversion = {}\n", quoted(php));
    }
    if let Some(node) = &manifest.node {
        let _ = write!(out, "\n[node]\nversion = {}\n", quoted(node));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[site]
name = "shop"
secure = true
aliases = ["admin", "api.test"]

[php]
version = "8.3"

[node]
version = "v22"
"#;

    #[test]
    fn reads_every_setting() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.name.as_deref(), Some("shop"));
        assert_eq!(m.secure, Some(true));
        assert_eq!(m.aliases, ["admin", "api.test"]);
        assert_eq!(m.php.as_deref(), Some("8.3"));
        assert_eq!(m.node.as_deref(), Some("22"), "a leading v is dropped");
        assert_eq!(m.warnings, Vec::<String>::new());
        assert_eq!(parse("").unwrap(), Manifest::default());
    }

    #[test]
    fn unknown_keys_warn_and_bad_values_fail_with_the_key_named() {
        let m = parse("[php]\nversion = \"8.4\"\nini = 1\n[services]\nmysql = true\n").unwrap();
        assert_eq!(m.php.as_deref(), Some("8.4"));
        assert_eq!(m.warnings.len(), 2, "{:?}", m.warnings);
        let unquoted = parse("[php]\nversion = 8.10\n").unwrap_err();
        assert!(
            unquoted.contains("[php] version must be quoted"),
            "{unquoted}"
        );
        assert!(
            parse("[php]\nversion = \"8\"\n").is_err(),
            "a major alone is too vague for PHP"
        );
        assert!(parse("[node]\nversion = \"lts\"\n").is_err());
        assert!(
            parse("[site]\nsecure = \"yes\"\n")
                .unwrap_err()
                .contains("[site] secure")
        );
        assert!(parse("[site]\naliases = \"admin\"\n").is_err());
        assert!(parse("[site\n").is_err(), "not TOML");
    }

    #[test]
    fn rendered_files_read_back_the_same() {
        let m = parse(FULL).unwrap();
        let text = render(&m, "Kiln project settings");
        assert!(text.starts_with("# Kiln project settings\n"));
        assert_eq!(parse(&text).unwrap(), m);
        let odd = Manifest {
            name: Some("quote \" and \\ back, tab\there, escape \u{1b}".into()),
            ..Manifest::default()
        };
        assert_eq!(parse(&render(&odd, "x")).unwrap(), odd);
    }

    #[test]
    fn a_missing_file_is_none_and_a_bad_one_names_its_path() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()).unwrap(), None);
        std::fs::write(dir.path().join(FILE), "[php]\nversion = 8.3\n").unwrap();
        let err = read(dir.path()).unwrap_err();
        assert!(err.contains(FILE), "{err}");
    }
}
