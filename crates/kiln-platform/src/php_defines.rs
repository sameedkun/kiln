//! Setting constants in a PHP config file ([ADR 0082]): `kiln env use` for a `WordPress`
//! site, whose settings are `define( 'DB_NAME', '…' );` lines in `wp-config.php`.
//!
//! Each constant is edited where it is: the value of its first `define` (the one PHP keeps)
//! is rewritten in place, and every other byte stays. A missing one goes on its own line
//! above `WordPress`'s "That's all, stop editing!" comment, else above the line that loads
//! `wp-settings.php`. A `define` on a commented line is never touched.
//!
//! As with `.env` keys (ADR 0045), the receipt keeps the whole file before and after: undo
//! writes the old bytes back if the file is as Kiln left it; if it was edited since, undo
//! stops, and with `force` puts back the old value (or removes the line Kiln added) only
//! where Kiln's value is still there.
//!
//! [ADR 0082]: ../../../docs/adr/0082-wordpress-config.md

use std::fs;
use std::io;
use std::path::PathBuf;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`PhpDefinesOp`].
pub const PHP_DEFINES_KIND: &str = "project.php.defines";

/// Where a constant's value is in the text: the literal's byte range.
fn find(text: &str, key: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(at) = text[from..].find("define").map(|i| i + from) {
        from = at + "define".len();
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let before = text[line_start..at].trim_start();
        if before.starts_with("//") || before.starts_with('#') || before.starts_with('*') {
            continue;
        }
        if at > 0 && text[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
            continue; // `undefined`, `my_define`
        }
        let Some(range) = literal_after(text, from, key) else {
            continue;
        };
        return Some(range);
    }
    None
}

fn skip_ws(text: &str, at: usize) -> usize {
    at + (text[at..].len() - text[at..].trim_start().len())
}

/// After `define`: `( 'KEY', <literal> )`, the literal's range.
fn literal_after(text: &str, at: usize, key: &str) -> Option<(usize, usize)> {
    let mut i = skip_ws(text, at);
    i += text[i..].strip_prefix('(').map(|_| 1)?;
    i = skip_ws(text, i);
    let quote = text[i..]
        .chars()
        .next()
        .filter(|c| *c == '\'' || *c == '"')?;
    i += 1;
    i += text[i..].strip_prefix(key).map(|_| key.len())?;
    i += text[i..].strip_prefix(quote).map(|_| 1)?;
    i = skip_ws(text, i);
    i += text[i..].strip_prefix(',').map(|_| 1)?;
    let start = skip_ws(text, i);
    let first = text[start..].chars().next()?;
    let end = if first == '\'' || first == '"' {
        let mut escaped = false;
        let close = text[start + 1..].char_indices().find_map(|(j, c)| {
            match c {
                '\\' if !escaped => escaped = true,
                c if c == first && !escaped => return Some(j),
                _ => escaped = false,
            }
            None
        })?;
        start + 1 + close + 1
    } else {
        // A bare value (`true`, `6379`): up to the closing parenthesis.
        let close = text[start..].find(')')?;
        start + text[start..start + close].trim_end().len()
    };
    let after = skip_ws(text, end);
    text[after..].starts_with(')').then_some((start, end))
}

/// A literal's value: a quoted string unescaped, anything else as written.
fn decode(literal: &str) -> String {
    let mut chars = literal.chars();
    match chars.next() {
        Some(q @ ('\'' | '"')) if literal.len() >= 2 && literal.ends_with(q) => {
            let body = &literal[1..literal.len() - 1];
            let mut out = String::new();
            let mut it = body.chars().peekable();
            while let Some(c) = it.next() {
                if c == '\\'
                    && let Some(&next) = it.peek()
                    && (next == q || next == '\\' || (q == '"' && next == '$'))
                {
                    out.push(next);
                    it.next();
                    continue;
                }
                out.push(c);
            }
            out
        }
        _ => literal.to_owned(),
    }
}

/// How Kiln writes a value: a port as a number, anything else single-quoted.
fn encode(key: &str, value: &str) -> String {
    if key.ends_with("_PORT") && !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The value `text` gives the constant `key`, if it defines it.
#[must_use]
pub fn value_of(text: &str, key: &str) -> Option<String> {
    find(text, key).map(|(s, e)| decode(&text[s..e]))
}

/// The literal as written, for putting back exactly.
fn literal_of(text: &str, key: &str) -> Option<String> {
    find(text, key).map(|(s, e)| text[s..e].to_owned())
}

/// Where a new `define` goes: the start of the line `WordPress` says to stop editing above,
/// else of the one loading `wp-settings.php`.
fn insertion_point(text: &str) -> Option<usize> {
    let at = text
        .find("That's all, stop editing")
        .or_else(|| text.find("wp-settings.php"))?;
    Some(text[..at].rfind('\n').map_or(0, |i| i + 1))
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// `text` with `key` set to the literal `literal`.
fn set_literal(text: &str, key: &str, literal: &str) -> Result<String, String> {
    if let Some((s, e)) = find(text, key) {
        let mut out = text.to_owned();
        out.replace_range(s..e, literal);
        return Ok(out);
    }
    let at = insertion_point(text).ok_or_else(|| {
        format!("no place for {key}: no \"stop editing\" line or wp-settings.php")
    })?;
    let mut out = text.to_owned();
    out.insert_str(
        at,
        &format!("define( '{key}', {literal} );{}", newline(text)),
    );
    Ok(out)
}

/// `text` with the `define` line of `key` taken out (one Kiln added).
fn remove(text: &str, key: &str) -> String {
    let Some((s, _)) = find(text, key) else {
        return text.to_owned();
    };
    let start = text[..s].rfind('\n').map_or(0, |i| i + 1);
    let end = text[s..].find('\n').map_or(text.len(), |i| s + i + 1);
    let mut out = text.to_owned();
    out.replace_range(start..end, "");
    out
}

fn failed(e: impl std::fmt::Display) -> OpError {
    OpError::Failed(e.to_string())
}

/// Sets constants in the PHP file at `path`, in order.
#[derive(Debug, Clone)]
pub struct PhpDefinesOp {
    path: PathBuf,
    keys: Vec<(String, String)>,
}

impl PhpDefinesOp {
    #[must_use]
    pub fn new(path: PathBuf, keys: Vec<(String, String)>) -> Self {
        Self { path, keys }
    }

    pub fn from_params(params: &Value) -> Result<Self, OpError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| failed("params.path missing"))?;
        let keys = params["keys"]
            .as_array()
            .ok_or_else(|| failed("params.keys missing"))?
            .iter()
            .map(|pair| match (pair[0].as_str(), pair[1].as_str()) {
                (Some(k), Some(v)) => Ok((k.to_owned(), v.to_owned())),
                _ => Err(failed("params.keys holds something else than pairs")),
            })
            .collect::<Result<_, _>>()?;
        Ok(Self::new(PathBuf::from(path), keys))
    }

    /// `None` if the file is gone.
    fn read(&self) -> Result<Option<String>, OpError> {
        match fs::read(&self.path) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| failed(format!("{} isn't UTF-8 text", self.path.display()))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(failed(format!("{}: {e}", self.path.display()))),
        }
    }

    fn existing(&self) -> Result<String, OpError> {
        self.read()?
            .ok_or_else(|| failed(format!("{} doesn't exist", self.path.display())))
    }

    fn edited(&self, text: &str) -> Result<String, OpError> {
        self.keys
            .iter()
            .try_fold(text.to_owned(), |text, (key, value)| {
                set_literal(&text, key, &encode(key, value)).map_err(failed)
            })
    }

    fn write(&self, text: &str) -> Result<(), OpError> {
        let temp = self.path.with_extension("kiln-tmp");
        fs::write(&temp, text)
            .and_then(|()| kiln_sys::rename_replace(&temp, &self.path))
            .map_err(|e| {
                let _ = fs::remove_file(&temp);
                failed(format!("{}: {e}", self.path.display()))
            })
    }
}

impl Operation for PhpDefinesOp {
    fn kind(&self) -> &'static str {
        PHP_DEFINES_KIND
    }

    fn params(&self) -> Value {
        json!({ "path": self.path.to_string_lossy(), "keys": self.keys })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let text = self.existing()?;
        self.edited(&text)?; // fails here, before asking, if a constant has nowhere to go
        Ok(Preview {
            summary: format!(
                "Set {} constant(s) in {}",
                self.keys.len(),
                self.path.display()
            ),
            changes: self
                .keys
                .iter()
                .map(|(key, value)| Change {
                    target: key.clone(),
                    before: value_of(&text, key),
                    after: Some(value.clone()),
                })
                .collect(),
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        let before = self.existing()?;
        let olds: Vec<Value> = self
            .keys
            .iter()
            .map(|(key, _)| json!(literal_of(&before, key)))
            .collect();
        let after = self.edited(&before)?;
        Ok(Receipt(
            json!({ "before": before, "after": after, "olds": olds }),
        ))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        let text = self.existing()?;
        let updated = self.edited(&text)?;
        if updated == text {
            return Ok(());
        }
        self.write(&updated)
    }

    fn verify(&self) -> Result<(), OpError> {
        let text = self.existing()?;
        for (key, value) in &self.keys {
            if value_of(&text, key).as_deref() != Some(value.as_str()) {
                return Err(OpError::VerifyFailed(format!(
                    "{key} isn't {value} in {}",
                    self.path.display()
                )));
            }
        }
        Ok(())
    }

    fn revert(&self, receipt: &Receipt, force: bool) -> Result<(), OpError> {
        let (Some(before), Some(after)) =
            (receipt.0["before"].as_str(), receipt.0["after"].as_str())
        else {
            return Err(failed("the receipt has no file contents"));
        };
        let Some(text) = self.read()? else {
            return Ok(()); // the site is gone, and its config with it
        };
        if text == before {
            return Ok(());
        }
        if text == after {
            return self.write(before);
        }
        if !force {
            return Err(OpError::Drifted(format!(
                "{} was edited after Kiln set its constants",
                self.path.display()
            )));
        }
        let olds = receipt.0["olds"].as_array().cloned().unwrap_or_default();
        let mut restored = text.clone();
        for ((key, value), old) in self.keys.iter().zip(olds) {
            if value_of(&restored, key).as_deref() != Some(value.as_str()) {
                continue;
            }
            restored = match old.as_str() {
                Some(literal) => set_literal(&restored, key, literal).map_err(failed)?,
                None => remove(&restored, key),
            };
        }
        if restored == text {
            return Ok(());
        }
        self.write(&restored)
    }
}

/// Lets the engine rebuild [`PhpDefinesOp`]s from the journal.
pub fn register_php_defines(registry: &mut Registry) {
    registry.register(PHP_DEFINES_KIND, |params| {
        Ok(Box::new(PhpDefinesOp::from_params(params)?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of `WordPress`'s `wp-config-sample.php`, shortened.
    const SAMPLE: &str = "<?php\r\n/**\r\n * The base configuration for WordPress\r\n * define( 'DB_NAME', 'in a comment' );\r\n */\r\n\r\n// ** Database settings ** //\r\n/** The name of the database for WordPress */\r\ndefine( 'DB_NAME', 'database_name_here' );\r\n\r\ndefine( 'DB_USER', 'username_here' );\r\ndefine( 'DB_PASSWORD', 'pass\\'word' );\r\ndefine( 'DB_HOST', 'localhost' );\r\n// define( 'DB_HOST', 'commented' );\r\ndefine( 'WP_DEBUG', false );\r\n\r\n/* That's all, stop editing! Happy publishing. */\r\n\r\nrequire_once ABSPATH . 'wp-settings.php';\r\n";

    fn op(path: PathBuf) -> PhpDefinesOp {
        PhpDefinesOp::new(
            path,
            [
                ("DB_NAME", "blog"),
                ("DB_USER", "root"),
                ("DB_PASSWORD", ""),
                ("DB_HOST", "127.0.0.1:3307"),
                ("WP_REDIS_PORT", "6380"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .to_vec(),
        )
    }

    #[test]
    fn values_are_read_as_php_reads_them() {
        assert_eq!(
            value_of(SAMPLE, "DB_NAME").as_deref(),
            Some("database_name_here")
        );
        assert_eq!(
            value_of(SAMPLE, "DB_PASSWORD").as_deref(),
            Some("pass'word")
        );
        assert_eq!(value_of(SAMPLE, "DB_HOST").as_deref(), Some("localhost"));
        assert_eq!(value_of(SAMPLE, "WP_DEBUG").as_deref(), Some("false"));
        assert_eq!(value_of(SAMPLE, "WP_REDIS_PORT"), None);
        assert_eq!(
            value_of("define(\"DB_NAME\",\"x\\\"y\");", "DB_NAME").as_deref(),
            Some("x\"y")
        );
        assert_eq!(value_of("undefine( 'DB_NAME', 'x' );", "DB_NAME"), None);
    }

    #[test]
    fn constants_are_set_in_place_and_new_ones_above_stop_editing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wp-config.php");
        fs::write(&path, SAMPLE).unwrap();
        let op = op(path.clone());
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        op.verify().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("define( 'DB_NAME', 'blog' );\r\n"));
        assert!(text.contains("define( 'DB_HOST', '127.0.0.1:3307' );\r\n"));
        // The comment and the commented-out line are as they were.
        assert!(text.contains(" * define( 'DB_NAME', 'in a comment' );"));
        assert!(text.contains("// define( 'DB_HOST', 'commented' );"));
        assert!(text.contains("define( 'WP_REDIS_PORT', 6380 );\r\n/* That's all, stop editing!"));
        // Undo puts back every byte.
        op.revert(&receipt, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), SAMPLE);
        op.revert(&receipt, false).unwrap(); // idempotent
    }

    #[test]
    fn an_edited_file_is_only_undone_with_force_and_only_where_kilns_values_are() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wp-config.php");
        fs::write(&path, SAMPLE).unwrap();
        let op = op(path.clone());
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("'root'", "'someone'");
        fs::write(&path, &edited).unwrap();
        assert!(matches!(
            op.revert(&receipt, false),
            Err(OpError::Drifted(_))
        ));
        op.revert(&receipt, true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            value_of(&text, "DB_NAME").as_deref(),
            Some("database_name_here")
        );
        assert_eq!(value_of(&text, "DB_PASSWORD").as_deref(), Some("pass'word"));
        assert_eq!(value_of(&text, "DB_USER").as_deref(), Some("someone"));
        assert_eq!(value_of(&text, "WP_REDIS_PORT"), None);
        assert!(!text.contains("WP_REDIS_PORT"));
    }

    #[test]
    fn a_file_with_nowhere_to_put_a_constant_fails_before_asking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wp-config.php");
        fs::write(&path, "<?php\ndefine( 'DB_NAME', 'x' );\n").unwrap();
        let err = op(path).preview().unwrap_err();
        assert!(err.to_string().contains("no place for DB_USER"), "{err}");
    }

    #[test]
    fn values_are_written_so_php_reads_them_back() {
        assert_eq!(encode("DB_PASSWORD", r"it's\x"), r"'it\'s\\x'");
        assert_eq!(decode(&encode("DB_PASSWORD", r"it's\x")), r"it's\x");
        assert_eq!(encode("WP_REDIS_PORT", "6379"), "6379");
        assert_eq!(encode("DB_PASSWORD", "1234"), "'1234'");
    }
}
