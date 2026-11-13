//! Setting keys in a project's `.env` ([ADR 0045]): the wiring `kiln env use` does.
//!
//! Each key is edited where it is: its line is rewritten in place, a commented-out example of
//! it (`# DB_HOST=127.0.0.1`, as Laravel ships for databases other than SQLite) is turned
//! into the setting, and a new key goes after the last key of its group (`DB_`, `REDIS_`…)
//! or at the end. Every other byte stays: comments, order, quoting, line endings.
//!
//! The receipt keeps the whole file before and after. Undo writes the old bytes back if the
//! file is as Kiln left it; if it was edited since, undo stops, and with `force` sets back
//! only the keys that still have Kiln's values.
//!
//! [ADR 0045]: ../../../docs/adr/0045-env-wiring.md

use std::fs;
use std::io;
use std::path::PathBuf;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`EnvKeysOp`].
pub const ENV_KEYS_KIND: &str = "project.env.keys";

/// Lines of `text` with their terminators.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// A line without its `\n` or `\r\n`, and that ending.
fn split_ending(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_suffix("\r\n") {
        (body, "\r\n")
    } else if let Some(body) = line.strip_suffix('\n') {
        (body, "\n")
    } else {
        (line, "")
    }
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// The key a setting line sets and the text after its `=`: `KEY=value`, with optional
/// spaces and an `export ` in front. `None` for comments, blank lines and anything else.
fn setting(body: &str) -> Option<(&str, &str)> {
    let rest = body.trim_start();
    let rest = rest.strip_prefix("export ").unwrap_or(rest).trim_start();
    let end = rest.find(|c: char| !is_key_char(c)).unwrap_or(rest.len());
    let (key, after) = rest.split_at(end);
    let first = key.chars().next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let value = after.trim_start().strip_prefix('=')?;
    Some((key, value))
}

/// The key of a commented-out setting line, such as `# DB_HOST=127.0.0.1`.
fn commented_key(body: &str) -> Option<&str> {
    let rest = body.trim_start().strip_prefix('#')?;
    setting(rest).map(|(key, _)| key)
}

/// A value as written after `=`, read as dotenv files are: quoted, or up to a ` #` comment.
fn read_value(raw: &str) -> String {
    let raw = raw.trim_start();
    if let Some(rest) = raw.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some(other) => out.push(other),
                    None => break,
                },
                c => out.push(c),
            }
        }
        return out;
    }
    if let Some(rest) = raw.strip_prefix('\'') {
        return rest.split('\'').next().unwrap_or_default().to_owned();
    }
    let end = raw.find(" #").unwrap_or(raw.len());
    raw[..end].trim_end().to_owned()
}

/// `value` as Kiln writes it: as it is if plain, else double-quoted.
fn write_value(value: &str) -> String {
    let plain = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-./:@+,".contains(c));
    if plain {
        return value.to_owned();
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' | '\\' | '$' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The value `text` gives `key`: its first setting line, as dotenv loaders keep the first.
#[must_use]
pub fn value_of(text: &str, key: &str) -> Option<String> {
    lines(text).into_iter().find_map(|line| {
        let (body, _) = split_ending(line);
        setting(body)
            .filter(|(k, _)| *k == key)
            .map(|(_, raw)| read_value(raw))
    })
}

/// Every setting in `text`, in order, each key once (its first, as dotenv loaders read it):
/// what `kiln env show` lists ([ADR 0056] §2).
///
/// [ADR 0056]: ../../../docs/adr/0056-services-page.md
#[must_use]
pub fn settings(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in lines(text) {
        let (body, _) = split_ending(line);
        if let Some((key, raw)) = setting(body)
            && !out.iter().any(|(k, _)| k == key)
        {
            out.push((key.to_owned(), read_value(raw)));
        }
    }
    out
}

/// The group a key belongs to, for placing a new one: `DB_` for `DB_HOST`.
fn group(key: &str) -> &str {
    key.find('_').map_or(key, |i| &key[..=i])
}

/// `text` with `key` set to `value` (see the module comment for where).
#[must_use]
pub fn set(text: &str, key: &str, value: &str) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let line_for = |ending: &str| format!("{key}={}{ending}", write_value(value));
    let mut all: Vec<String> = lines(text).into_iter().map(str::to_owned).collect();
    let keyed =
        |pick: &dyn Fn(&str) -> bool| all.iter().position(|line| pick(split_ending(line).0));
    if let Some(i) = keyed(&|body| setting(body).is_some_and(|(k, _)| k == key)) {
        let (body, ending) = split_ending(&all[i]);
        let indent = &body[..body.len() - body.trim_start().len()];
        let export = if body.trim_start().starts_with("export ") {
            "export "
        } else {
            ""
        };
        all[i] = format!("{indent}{export}{}", line_for(ending));
        return all.concat();
    }
    if let Some(i) = keyed(&|body| commented_key(body) == Some(key)) {
        let (_, ending) = split_ending(&all[i]);
        all[i] = line_for(ending);
        return all.concat();
    }
    let prefix = group(key);
    let last_of_group = all.iter().rposition(|line| {
        let body = split_ending(line).0;
        setting(body)
            .map(|(k, _)| k)
            .or_else(|| commented_key(body))
            .is_some_and(|k| k != key && group(k) == prefix && k.len() > prefix.len())
    });
    if let Some(i) = last_of_group {
        if split_ending(&all[i]).1.is_empty() {
            all[i].push_str(eol);
            all.insert(i + 1, line_for(""));
        } else {
            all.insert(i + 1, line_for(eol));
        }
        return all.concat();
    }
    // A group of its own at the end, after a blank line.
    if let Some(last) = all.last_mut() {
        if split_ending(last).1.is_empty() {
            last.push_str(eol);
        }
        if !split_ending(last).0.trim().is_empty() {
            all.push(eol.to_owned());
        }
    }
    all.push(line_for(eol));
    all.concat()
}

/// `text` with `key` back to `old`: set again, or its line removed if it had none.
fn unset(text: &str, key: &str, old: Option<&str>) -> String {
    if let Some(old) = old {
        return set(text, key, old);
    }
    lines(text)
        .into_iter()
        .filter(|line| setting(split_ending(line).0).is_none_or(|(k, _)| k != key))
        .collect()
}

fn failed(e: impl std::fmt::Display) -> OpError {
    OpError::Failed(e.to_string())
}

/// Sets `keys` in the `.env` file at `path`, in order.
#[derive(Debug, Clone)]
pub struct EnvKeysOp {
    path: PathBuf,
    keys: Vec<(String, String)>,
}

impl EnvKeysOp {
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

    /// `text` with every key set.
    fn edited(&self, text: &str) -> String {
        self.keys
            .iter()
            .fold(text.to_owned(), |text, (key, value)| set(&text, key, value))
    }

    /// Replaces the file whole: a temporary file beside it, swapped in.
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

impl Operation for EnvKeysOp {
    fn kind(&self) -> &'static str {
        ENV_KEYS_KIND
    }

    fn params(&self) -> Value {
        json!({ "path": self.path.to_string_lossy(), "keys": self.keys })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let text = self.existing()?;
        Ok(Preview {
            summary: format!("Set {} key(s) in {}", self.keys.len(), self.path.display()),
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
            .map(|(key, _)| json!(value_of(&before, key)))
            .collect();
        let after = self.edited(&before);
        Ok(Receipt(
            json!({ "before": before, "after": after, "olds": olds }),
        ))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        let text = self.existing()?;
        let updated = self.edited(&text);
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
            return Ok(()); // the project is gone, and its .env with it
        };
        if text == before {
            return Ok(()); // never applied, or already undone
        }
        if text == after {
            return self.write(before);
        }
        if !force {
            return Err(OpError::Drifted(format!(
                "{} was edited after Kiln set its keys",
                self.path.display()
            )));
        }
        // Edited since: only the keys that still have Kiln's values go back.
        let olds = receipt.0["olds"].as_array().cloned().unwrap_or_default();
        let mut restored = text.clone();
        for ((key, value), old) in self.keys.iter().zip(olds) {
            if value_of(&restored, key).as_deref() == Some(value.as_str()) {
                restored = unset(&restored, key, old.as_str());
            }
        }
        if restored == text {
            return Ok(());
        }
        self.write(&restored)
    }
}

/// Lets the engine rebuild [`EnvKeysOp`]s from the journal.
pub fn register_env_keys(registry: &mut Registry) {
    registry.register(ENV_KEYS_KIND, |params| {
        Ok(Box::new(EnvKeysOp::from_params(params)?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_come_in_order_each_key_once() {
        let text = "APP_NAME=\"My Shop\"\n# DB_HOST=commented\nDB_PORT=3306 # mysql\nDB_PORT=9999\r\nEMPTY=\n";
        assert_eq!(
            settings(text),
            vec![
                ("APP_NAME".to_owned(), "My Shop".to_owned()),
                ("DB_PORT".to_owned(), "3306".to_owned()),
                ("EMPTY".to_owned(), String::new()),
            ]
        );
    }

    /// The shape of Laravel 12's `.env`, with SQLite and the other databases commented out.
    const LARAVEL: &str = "APP_NAME=Laravel\nAPP_KEY=base64:abc=\n\nDB_CONNECTION=sqlite\n# DB_HOST=127.0.0.1\n# DB_PORT=3306\n# DB_DATABASE=laravel\n# DB_USERNAME=root\n# DB_PASSWORD=\n\nREDIS_CLIENT=phpredis\nREDIS_HOST=127.0.0.1\nREDIS_PASSWORD=null\nREDIS_PORT=6379\n\nMAIL_MAILER=log\nMAIL_HOST=127.0.0.1 # the inbox\nMAIL_FROM_ADDRESS=\"hello@example.com\"\n";

    fn op(path: PathBuf) -> EnvKeysOp {
        EnvKeysOp::new(
            path,
            [
                ("DB_CONNECTION", "mysql"),
                ("DB_HOST", "127.0.0.1"),
                ("DB_PORT", "3307"),
                ("DB_DATABASE", "shop"),
                ("DB_PASSWORD", ""),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .to_vec(),
        )
    }

    #[test]
    fn values_are_read_as_dotenv_files_write_them() {
        assert_eq!(value_of(LARAVEL, "APP_KEY").as_deref(), Some("base64:abc="));
        assert_eq!(value_of(LARAVEL, "MAIL_HOST").as_deref(), Some("127.0.0.1"));
        assert_eq!(
            value_of(LARAVEL, "MAIL_FROM_ADDRESS").as_deref(),
            Some("hello@example.com")
        );
        assert_eq!(value_of(LARAVEL, "DB_HOST"), None, "commented out");
        assert_eq!(value_of("export A = 'x y'\n", "A").as_deref(), Some("x y"));
        assert_eq!(value_of("A=\"a\\\"b\"\n", "A").as_deref(), Some("a\"b"));
    }

    #[test]
    fn keys_are_set_in_place_uncommented_or_added_to_their_group() {
        let text = set(LARAVEL, "DB_CONNECTION", "mysql");
        assert!(text.contains("\nDB_CONNECTION=mysql\n# DB_HOST"));
        let text = set(&text, "DB_HOST", "127.0.0.1");
        assert!(text.contains("DB_CONNECTION=mysql\nDB_HOST=127.0.0.1\n# DB_PORT"));
        let text = set(&text, "REDIS_PORT", "6380");
        assert!(text.contains("REDIS_PASSWORD=null\nREDIS_PORT=6380\n"));
        let text = set(&text, "REDIS_DB", "1");
        assert!(text.contains("REDIS_PORT=6380\nREDIS_DB=1\n\nMAIL_MAILER"));
        let text = set(&text, "MEILISEARCH_HOST", "http://127.0.0.1:7700");
        assert!(text.ends_with("hello@example.com\"\n\nMEILISEARCH_HOST=http://127.0.0.1:7700\n"));
        assert_eq!(set("A=1", "B", "two words"), "A=1\n\nB=\"two words\"\n");
        assert_eq!(
            set("A_X=1\r\nB=2\r\n", "A_Y", "3"),
            "A_X=1\r\nA_Y=3\r\nB=2\r\n"
        );
        assert_eq!(set("  export A=1\n", "A", "2"), "  export A=2\n");
        assert_eq!(set("", "A", "1"), "A=1\n");
    }

    #[test]
    fn values_that_need_quotes_get_them() {
        assert_eq!(write_value(""), "");
        assert_eq!(
            write_value("mongodb://127.0.0.1:27017"),
            "mongodb://127.0.0.1:27017"
        );
        assert_eq!(write_value("a b$c\"d"), "\"a b\\$c\\\"d\"");
        let text = set("", "K", "a b$c\"d");
        assert_eq!(value_of(&text, "K").as_deref(), Some("a b$c\"d"));
    }

    #[test]
    fn apply_then_undo_gives_back_the_same_bytes() {
        for original in [
            LARAVEL.to_owned(),
            LARAVEL.replace('\n', "\r\n"),
            "APP=1".into(),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(".env");
            fs::write(&path, &original).unwrap();
            let op = op(path.clone());
            let receipt = op.prepare().unwrap();
            op.apply(&receipt).unwrap();
            op.verify().unwrap();
            let applied = fs::read_to_string(&path).unwrap();
            assert_ne!(applied, original);
            if original.contains("\r\n") {
                assert!(
                    !applied.replace("\r\n", "").contains('\n'),
                    "CRLF kept:\n{applied}"
                );
            }
            op.revert(&receipt, false).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
            op.revert(&receipt, false).unwrap(); // idempotent
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn undo_after_an_edit_stops_and_force_restores_only_kilns_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, LARAVEL).unwrap();
        let op = op(path.clone());
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        let edited = fs::read_to_string(&path)
            .unwrap()
            .replace("DB_PORT=3307", "DB_PORT=3999")
            + "APP_DEBUG=true\n";
        fs::write(&path, &edited).unwrap();
        assert!(matches!(
            op.revert(&receipt, false),
            Err(OpError::Drifted(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), edited, "untouched");

        op.revert(&receipt, true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(value_of(&text, "DB_CONNECTION").as_deref(), Some("sqlite"));
        assert_eq!(
            value_of(&text, "DB_PORT").as_deref(),
            Some("3999"),
            "the user's edit"
        );
        assert_eq!(value_of(&text, "DB_DATABASE"), None);
        assert_eq!(value_of(&text, "APP_DEBUG").as_deref(), Some("true"));
    }

    #[test]
    fn rebuilds_from_its_params_and_previews_old_and_new_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, LARAVEL).unwrap();
        let op = op(path);
        let rebuilt = EnvKeysOp::from_params(&op.params()).unwrap();
        assert_eq!(rebuilt.params(), op.params());
        let preview = op.preview().unwrap();
        assert_eq!(preview.changes[0].before.as_deref(), Some("sqlite"));
        assert_eq!(preview.changes[0].after.as_deref(), Some("mysql"));
        assert_eq!(preview.changes[1].before, None);
    }
}
