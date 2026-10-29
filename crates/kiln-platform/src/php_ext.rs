//! Turning a PHP extension on or off in Kiln's `php.ini` for one build ([ADR 0030] §4).
//!
//! One line changes per toggle: `on` appends `extension=<name>` (or `zend_extension=`), `off`
//! comments out every active line loading it with [`OFF_MARKER`]. Undo removes that line or
//! the marker, so the rest of the file, including the user's own edits, is kept.
//!
//! [ADR 0030]: ../../../docs/adr/0030-php-extension-toggles.md

use std::fs;
use std::io;
use std::path::PathBuf;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`PhpExtOp`].
pub const PHP_EXT_KIND: &str = "php.ini.extension";

/// Prefix of a line Kiln commented out; PHP reads it as a comment.
pub const OFF_MARKER: &str = ";[kiln off] ";

/// Extensions loaded with `zend_extension=`.
pub const ZEND_EXTENSIONS: &[&str] = &["opcache", "xdebug"];

/// An extension's name as `php -m` would print it, lowercased, from a name, file name or
/// path: `php_zip.dll`, `zip.so` and `/usr/lib/php/20230831/zip.so` are all `zip`.
#[must_use]
pub fn normalise(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let lower = file.trim().to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".dll")
        .or_else(|| lower.strip_suffix(".so"))
        .unwrap_or(&lower);
    stem.strip_prefix("php_").unwrap_or(stem).to_owned()
}

/// The extension an active `extension=` or `zend_extension=` line loads.
#[must_use]
pub fn loaded_by(line: &str) -> Option<String> {
    let line = line.trim();
    let rest = line
        .strip_prefix("zend_extension")
        .or_else(|| line.strip_prefix("extension"))?;
    let value = rest.trim_start().strip_prefix('=')?;
    let value = value.split(';').next().unwrap_or_default().trim();
    let value = value.trim_matches(|c| c == '"' || c == '\'');
    (!value.is_empty()).then(|| normalise(value))
}

/// The line that turns `name` on.
#[must_use]
pub fn on_line(name: &str) -> String {
    if ZEND_EXTENSIONS.contains(&name) {
        format!("zend_extension={name}")
    } else {
        format!("extension={name}")
    }
}

/// Lines of `text` with their terminators.
fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split_inclusive(|&b| b == b'\n')
}

/// A line without its `\n` or `\r\n`.
fn content(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// The extensions active lines of `text` load.
#[must_use]
pub fn loads(text: &[u8]) -> Vec<String> {
    lines(text)
        .filter_map(|l| loaded_by(&String::from_utf8_lossy(content(l))))
        .collect()
}

/// Whether an active line of `text` loads `name`.
fn is_loaded(text: &[u8], name: &str) -> bool {
    loads(text).iter().any(|n| n == name)
}

/// `text` with `line` appended; also whether a missing final line ending was added.
fn append_line(text: &[u8], line: &str) -> (Vec<u8>, bool) {
    let added_eol = !text.is_empty() && !text.ends_with(b"\n");
    let mut out = text.to_vec();
    if added_eol {
        out.push(b'\n');
    }
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
    (out, added_eol)
}

/// `text` without the last line that reads `line`, and without the line ending
/// [`append_line`] added before it.
fn remove_line(text: &[u8], line: &str, added_eol: bool) -> Vec<u8> {
    let all: Vec<&[u8]> = lines(text).collect();
    let Some(index) = all.iter().rposition(|l| content(l) == line.as_bytes()) else {
        return text.to_vec();
    };
    let mut out: Vec<u8> = all[..index].concat();
    let rest = all[index + 1..].concat();
    if added_eol && rest.is_empty() {
        let _ = out.pop_if(|&mut b| b == b'\n');
    }
    out.extend_from_slice(&rest);
    out
}

/// `text` with every active line loading `name` commented out, and those lines as they were.
fn comment_out(text: &[u8], name: &str) -> (Vec<u8>, Vec<String>) {
    let mut out = Vec::with_capacity(text.len());
    let mut originals = Vec::new();
    for line in lines(text) {
        let line_text = String::from_utf8_lossy(content(line));
        if loaded_by(&line_text).as_deref() == Some(name) {
            originals.push(line_text.into_owned());
            out.extend_from_slice(OFF_MARKER.as_bytes());
        }
        out.extend_from_slice(line);
    }
    (out, originals)
}

/// `text` with the marker taken off the first commented copy of each of `originals`.
fn uncomment(text: &[u8], originals: &[String]) -> Vec<u8> {
    let mut all: Vec<Vec<u8>> = lines(text).map(<[u8]>::to_vec).collect();
    for original in originals {
        let marked = format!("{OFF_MARKER}{original}");
        if let Some(line) = all.iter_mut().find(|l| content(l) == marked.as_bytes()) {
            line.drain(..OFF_MARKER.len());
        }
    }
    all.concat()
}

fn failed(e: impl std::fmt::Display) -> OpError {
    OpError::Failed(e.to_string())
}

/// Turns extension `name` on or off in the `php.ini` of PHP `version`.
#[derive(Debug, Clone)]
pub struct PhpExtOp {
    ini: PathBuf,
    version: String,
    name: String,
    on: bool,
}

impl PhpExtOp {
    #[must_use]
    pub fn new(ini: PathBuf, version: impl Into<String>, name: &str, on: bool) -> Self {
        Self {
            ini,
            version: version.into(),
            name: normalise(name),
            on,
        }
    }

    pub fn from_params(params: &Value) -> Result<Self, OpError> {
        let text = |key: &str| {
            params[key]
                .as_str()
                .ok_or_else(|| failed(format!("params.{key} missing")))
        };
        let on = params["on"]
            .as_bool()
            .ok_or_else(|| failed("params.on missing"))?;
        Ok(Self::new(
            PathBuf::from(text("ini")?),
            text("version")?,
            text("name")?,
            on,
        ))
    }

    /// The `php.ini` this changes.
    #[must_use]
    pub fn ini(&self) -> &std::path::Path {
        &self.ini
    }

    /// `None` if the file is gone.
    fn read(&self) -> Result<Option<Vec<u8>>, OpError> {
        match fs::read(&self.ini) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(failed(format!("{}: {e}", self.ini.display()))),
        }
    }

    fn existing(&self) -> Result<Vec<u8>, OpError> {
        self.read()?
            .ok_or_else(|| failed(format!("{} doesn't exist", self.ini.display())))
    }

    /// Replaces the file atomically. It is Kiln's own file, so a rename is right (I-44).
    fn write(&self, bytes: &[u8]) -> Result<(), OpError> {
        let temp = self.ini.with_extension("ini.kiln-tmp");
        fs::write(&temp, bytes)
            .and_then(|()| kiln_sys::rename_replace(&temp, &self.ini))
            .map_err(|e| {
                let _ = fs::remove_file(&temp);
                failed(format!("{}: {e}", self.ini.display()))
            })
    }

    fn state_word(&self, on: bool) -> String {
        format!("{} {}", self.name, if on { "on" } else { "off" })
    }
}

impl Operation for PhpExtOp {
    fn kind(&self) -> &'static str {
        PHP_EXT_KIND
    }

    fn params(&self) -> Value {
        json!({
            "ini": self.ini.to_string_lossy(),
            "version": self.version,
            "name": self.name,
            "on": self.on,
        })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let loaded = is_loaded(&self.existing()?, &self.name);
        Ok(Preview {
            summary: format!(
                "Turn {} {} for PHP {}",
                self.name,
                if self.on { "on" } else { "off" },
                self.version
            ),
            changes: vec![Change {
                target: self.ini.display().to_string(),
                before: Some(self.state_word(loaded)),
                after: Some(self.state_word(self.on)),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        let text = self.existing()?;
        let loaded = is_loaded(&text, &self.name);
        Ok(Receipt(if self.on {
            json!({
                "changed": !loaded,
                "added_eol": append_line(&text, "").1,
            })
        } else {
            json!({
                "changed": loaded,
                "originals": comment_out(&text, &self.name).1,
            })
        }))
    }

    fn apply(&self, receipt: &Receipt) -> Result<(), OpError> {
        if !receipt.0["changed"].as_bool().unwrap_or(false) {
            return Ok(());
        }
        let text = self.existing()?;
        let updated = if self.on {
            if is_loaded(&text, &self.name) {
                return Ok(());
            }
            append_line(&text, &on_line(&self.name)).0
        } else {
            comment_out(&text, &self.name).0
        };
        if updated == text {
            return Ok(());
        }
        self.write(&updated)
    }

    fn verify(&self) -> Result<(), OpError> {
        if is_loaded(&self.existing()?, &self.name) == self.on {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "{} isn't {} in {}",
                self.name,
                if self.on { "on" } else { "off" },
                self.ini.display()
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, _force: bool) -> Result<(), OpError> {
        if !receipt.0["changed"].as_bool().unwrap_or(false) {
            return Ok(());
        }
        let Some(text) = self.read()? else {
            return Ok(()); // the build is gone, and its php.ini with it
        };
        let restored = if self.on {
            let added_eol = receipt.0["added_eol"].as_bool().unwrap_or(false);
            remove_line(&text, &on_line(&self.name), added_eol)
        } else {
            let originals: Vec<String> = receipt.0["originals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            uncomment(&text, &originals)
        };
        if restored == text {
            return Ok(());
        }
        self.write(&restored)
    }
}

/// Lets the engine rebuild [`PhpExtOp`]s from the journal.
pub fn register_php_ext(registry: &mut Registry) {
    registry.register(PHP_EXT_KIND, |params| {
        Ok(Box::new(PhpExtOp::from_params(params)?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "[PHP]\r\n;extension=zip\r\nmemory_limit = 128M\r\n\n; ---- Kiln defaults ----\nextension_dir = \"ext\"\nextension=curl\nextension=zip\n";

    #[test]
    fn names_and_lines_are_read_like_php_does() {
        assert_eq!(normalise("php_PDO_MySQL.dll"), "pdo_mysql");
        assert_eq!(normalise("/usr/lib/php/20230831/zip.so"), "zip");
        assert_eq!(normalise(r"C:\php\ext\php_xsl.dll"), "xsl");
        assert_eq!(
            loaded_by("extension = \"php_curl.dll\" ; note").as_deref(),
            Some("curl")
        );
        assert_eq!(
            loaded_by("zend_extension=opcache").as_deref(),
            Some("opcache")
        );
        assert_eq!(loaded_by(";extension=zip"), None);
        assert_eq!(loaded_by("extension_dir = \"ext\""), None);
        assert_eq!(loaded_by(&format!("{OFF_MARKER}extension=zip")), None);
        assert_eq!(on_line("opcache"), "zend_extension=opcache");
        assert_eq!(on_line("xsl"), "extension=xsl");
        assert_eq!(loads(INI.as_bytes()), ["curl", "zip"]);
    }

    #[test]
    fn on_then_undo_gives_back_the_same_bytes() {
        for original in [INI, "no final newline", ""] {
            let (with, added) = append_line(original.as_bytes(), "extension=xsl");
            assert!(is_loaded(&with, "xsl"));
            assert_eq!(
                remove_line(&with, "extension=xsl", added),
                original.as_bytes(),
                "{original:?}"
            );
        }
    }

    #[test]
    fn off_then_undo_gives_back_the_same_bytes() {
        let twice = format!("{INI}extension = php_zip.dll\r\n");
        let (without, originals) = comment_out(twice.as_bytes(), "zip");
        assert_eq!(originals, ["extension=zip", "extension = php_zip.dll"]);
        assert!(!is_loaded(&without, "zip") && is_loaded(&without, "curl"));
        let text = String::from_utf8(without.clone()).unwrap();
        assert!(
            text.contains("\r\n;extension=zip\r\n"),
            "PHP's own example untouched"
        );
        assert!(text.contains(";[kiln off] extension=zip\n"));
        assert_eq!(uncomment(&without, &originals), twice.as_bytes());
    }

    #[test]
    fn the_operation_toggles_and_undo_keeps_later_edits() {
        let dir = tempfile::tempdir().unwrap();
        let ini = dir.path().join("php.ini");
        fs::write(&ini, INI).unwrap();

        let on = PhpExtOp::new(ini.clone(), "8.4.26", "php_xsl.dll", true);
        let on_receipt = on.prepare().unwrap();
        on.apply(&on_receipt).unwrap();
        on.verify().unwrap();
        let off = PhpExtOp::new(ini.clone(), "8.4.26", "zip", false);
        let off_receipt = off.prepare().unwrap();
        off.apply(&off_receipt).unwrap();
        off.verify().unwrap();
        assert_eq!(loads(&fs::read(&ini).unwrap()), ["curl", "xsl"]);

        // The user edits the file; undoing the older toggle first keeps the edit and the
        // newer toggle.
        let edited = format!("{}date.timezone = UTC\n", fs::read_to_string(&ini).unwrap());
        fs::write(&ini, &edited).unwrap();
        on.revert(&on_receipt, false).unwrap();
        on.revert(&on_receipt, false).unwrap(); // idempotent
        assert_eq!(loads(&fs::read(&ini).unwrap()), ["curl"]);
        off.revert(&off_receipt, false).unwrap();
        assert_eq!(
            fs::read_to_string(&ini).unwrap(),
            format!("{INI}date.timezone = UTC\n")
        );

        // Already in the asked-for state: nothing changes, and undo changes nothing.
        let again = PhpExtOp::new(ini.clone(), "8.4.26", "curl", true);
        let receipt = again.prepare().unwrap();
        assert_eq!(receipt.0["changed"], false);
        again.apply(&receipt).unwrap();
        again.revert(&receipt, false).unwrap();
        assert_eq!(
            fs::read_to_string(&ini).unwrap(),
            format!("{INI}date.timezone = UTC\n")
        );

        // The build was removed: undo has nothing left to do.
        fs::remove_file(&ini).unwrap();
        on.revert(&on_receipt, false).unwrap();
        assert!(on.apply(&on_receipt).is_err());
    }

    #[test]
    fn rebuilds_from_its_params() {
        let op = PhpExtOp::new(
            PathBuf::from("/k/php/8.4.26/php.ini"),
            "8.4.26",
            "XSL",
            false,
        );
        let rebuilt = PhpExtOp::from_params(&op.params()).unwrap();
        assert_eq!(rebuilt.params(), op.params());
        assert_eq!(op.params()["name"], "xsl");
    }
}
