//! Putting Kiln's shims on `PATH` on Linux ([ADR 0023] §3): a marked block in a shell startup
//! file (`~/.profile`, `~/.bashrc`, `~/.zshrc`, or fish's `conf.d/kiln.fish`).
//!
//! The block is edited as bytes, like Kiln's hosts block: every byte outside it is kept,
//! ambiguous markers are refused, the file's line endings and mode are kept, and appending
//! then removing the block gives back the original file byte for byte.
//!
//! [ADR 0023]: ../../../docs/adr/0023-linux-shims-and-path.md

use std::fs;
use std::io;
use std::path::PathBuf;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`ShellProfileOp`].
pub const SHELL_PROFILE_KIND: &str = "env.shell_profile";

pub const BEGIN_MARKER: &str = "# kiln:begin - managed by Kiln; `kiln undo` removes this block";
pub const END_MARKER: &str = "# kiln:end";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BlockError {
    #[error("{0} Kiln begin markers; fix the file by hand first")]
    Duplicate(usize),
    #[error("a Kiln begin marker without its end marker; fix the file by hand first")]
    Unterminated,
    #[error("a Kiln end marker without a begin marker; fix the file by hand first")]
    StrayEnd,
}

/// Byte range of Kiln's block, from its begin marker to the end of its end-marker line.
fn find_block(text: &[u8]) -> Result<Option<std::ops::Range<usize>>, BlockError> {
    let (mut begins, mut ends, mut offset) = (Vec::new(), Vec::new(), 0);
    for line in text.split_inclusive(|&b| b == b'\n') {
        let trimmed = line.trim_ascii();
        if trimmed == BEGIN_MARKER.as_bytes() {
            begins.push(offset);
        } else if trimmed == END_MARKER.as_bytes() {
            ends.push(offset + line.len());
        }
        offset += line.len();
    }
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([start], [end]) if start < end => Ok(Some(*start..*end)),
        ([_], []) => Err(BlockError::Unterminated),
        ([] | [_], _) => Err(BlockError::StrayEnd),
        (many, _) => Err(BlockError::Duplicate(many.len())),
    }
}

fn line_ending(text: &[u8]) -> &'static str {
    if text.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// The lines between Kiln's markers, if the block exists.
pub fn read_block(text: &[u8]) -> Result<Option<String>, BlockError> {
    Ok(find_block(text)?.map(|range| {
        String::from_utf8_lossy(&text[range])
            .lines()
            .filter(|l| l.trim() != BEGIN_MARKER && l.trim() != END_MARKER)
            .collect::<Vec<_>>()
            .join("\n")
    }))
}

/// `text` with Kiln's block holding `body` (one or more lines), replaced in place or appended
/// at the end.
pub fn upsert_block(text: &[u8], body: &str) -> Result<Vec<u8>, BlockError> {
    let eol = line_ending(text);
    let block = format!(
        "{BEGIN_MARKER}{eol}{}{eol}{END_MARKER}{eol}",
        body.lines().collect::<Vec<_>>().join(eol)
    );
    if let Some(range) = find_block(text)? {
        return Ok([&text[..range.start], block.as_bytes(), &text[range.end..]].concat());
    }
    if text.is_empty() {
        return Ok(block.into_bytes());
    }
    // One line ending is added, which `remove_block` strips again: a blank separator line
    // after a terminated file, or the missing terminator.
    Ok([text, eol.as_bytes(), block.as_bytes()].concat())
}

/// `text` without Kiln's block, undoing the separator [`upsert_block`] added.
pub fn remove_block(text: &[u8]) -> Result<Vec<u8>, BlockError> {
    let Some(range) = find_block(text)? else {
        return Ok(text.to_vec());
    };
    let eol = if text[..range.end].ends_with(b"\r\n") {
        &b"\r\n"[..]
    } else {
        &b"\n"[..]
    };
    let mut before = &text[..range.start];
    let after = &text[range.end..];
    if after.is_empty()
        && let Some(stripped) = before.strip_suffix(eol)
    {
        before = stripped;
    }
    Ok([before, after].concat())
}

fn failed(e: impl std::fmt::Display) -> OpError {
    OpError::Failed(e.to_string())
}

/// Makes Kiln's block in one shell startup file hold `body` (a line sourcing Kiln's `env`).
#[derive(Debug, Clone)]
pub struct ShellProfileOp {
    file: PathBuf,
    body: String,
}

impl ShellProfileOp {
    #[must_use]
    pub fn new(file: PathBuf, body: impl Into<String>) -> Self {
        Self {
            file,
            body: body.into(),
        }
    }

    pub fn from_params(params: &Value) -> Result<Self, OpError> {
        let text = |key: &str| {
            params[key]
                .as_str()
                .ok_or_else(|| failed(format!("params.{key} missing")))
        };
        Ok(Self::new(PathBuf::from(text("file")?), text("body")?))
    }

    /// The file's bytes; empty if it doesn't exist.
    fn read(&self) -> Result<Option<Vec<u8>>, OpError> {
        match fs::read(&self.file) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(failed(format!("{}: {e}", self.file.display()))),
        }
    }

    fn current_block(&self) -> Result<Option<String>, OpError> {
        let bytes = self.read()?.unwrap_or_default();
        read_block(&bytes).map_err(|e| failed(format!("{}: {e}", self.file.display())))
    }

    /// Writes `bytes` to the file, keeping its mode (atomic replace) or creating it.
    fn write(&self, bytes: &[u8], existed: bool) -> Result<(), OpError> {
        let result = if existed {
            kiln_sys::write_replacing(&self.file, bytes)
        } else {
            if let Some(dir) = self.file.parent() {
                fs::create_dir_all(dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
            }
            fs::write(&self.file, bytes)
        };
        result.map_err(|e| failed(format!("{}: {e}", self.file.display())))
    }
}

impl Operation for ShellProfileOp {
    fn kind(&self) -> &'static str {
        SHELL_PROFILE_KIND
    }

    fn params(&self) -> Value {
        json!({ "file": self.file.to_string_lossy(), "body": self.body })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let existed = self.read()?.is_some();
        let before = self.current_block()?;
        Ok(Preview {
            summary: if before.as_deref() == Some(self.body.as_str()) {
                format!("{} already puts Kiln's shims on PATH", self.file.display())
            } else {
                format!(
                    "Put Kiln's shims first on PATH in {}{}",
                    self.file.display(),
                    if existed { "" } else { " (a new file)" }
                )
            },
            changes: vec![Change {
                target: self.file.display().to_string(),
                before,
                after: Some(self.body.clone()),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        Ok(Receipt(json!({
            "existed": self.read()?.is_some(),
            "block_before": self.current_block()?,
        })))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        let current = self.read()?;
        let existed = current.is_some();
        let current = current.unwrap_or_default();
        let updated = upsert_block(&current, &self.body)
            .map_err(|e| failed(format!("{}: {e}", self.file.display())))?;
        if updated == current {
            return Ok(());
        }
        self.write(&updated, existed)
    }

    fn verify(&self) -> Result<(), OpError> {
        if self.current_block()?.as_deref() == Some(self.body.as_str()) {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "Kiln's block isn't in {}",
                self.file.display()
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, _force: bool) -> Result<(), OpError> {
        let Some(current) = self.read()? else {
            return Ok(()); // the file is gone: nothing of Kiln's is left in it
        };
        let restored = match receipt.0["block_before"].as_str() {
            Some(before) => upsert_block(&current, before),
            None => remove_block(&current),
        }
        .map_err(|e| failed(format!("{}: {e}", self.file.display())))?;
        let created = !receipt.0["existed"].as_bool().unwrap_or(true);
        if created && restored.is_empty() {
            return fs::remove_file(&self.file)
                .map_err(|e| failed(format!("{}: {e}", self.file.display())));
        }
        if restored == current {
            return Ok(());
        }
        self.write(&restored, true)
    }
}

/// Lets the engine rebuild [`ShellProfileOp`]s from the journal.
pub fn register_shell_profile(registry: &mut Registry) {
    registry.register(SHELL_PROFILE_KIND, |params| {
        Ok(Box::new(ShellProfileOp::from_params(params)?))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASHRC: &[u8] = b"# ~/.bashrc\nexport EDITOR=vim\n";
    const BODY: &str = ". \"/home/me/.local/share/kiln/env\"";

    #[test]
    fn append_then_remove_gives_back_the_same_bytes() {
        for original in [BASHRC, b"no trailing newline", b"crlf\r\nfile\r\n", b""] {
            let with = upsert_block(original, BODY).unwrap();
            assert_eq!(read_block(&with).unwrap().as_deref(), Some(BODY));
            assert_eq!(remove_block(&with).unwrap(), original, "{original:?}");
            assert_eq!(upsert_block(&with, BODY).unwrap(), with, "idempotent");
        }
        let crlf = upsert_block(b"a\r\n", BODY).unwrap();
        assert!(String::from_utf8(crlf).unwrap().ends_with("# kiln:end\r\n"));
    }

    #[test]
    fn ambiguous_markers_are_refused() {
        let twice = format!("{BEGIN_MARKER}\nx\n{END_MARKER}\n{BEGIN_MARKER}\ny\n{END_MARKER}\n");
        assert_eq!(
            upsert_block(twice.as_bytes(), BODY),
            Err(BlockError::Duplicate(2))
        );
        assert_eq!(
            upsert_block(format!("{BEGIN_MARKER}\n").as_bytes(), BODY),
            Err(BlockError::Unterminated)
        );
        assert_eq!(
            remove_block(format!("{END_MARKER}\n").as_bytes()),
            Err(BlockError::StrayEnd)
        );
    }

    #[test]
    fn the_operation_adds_and_undo_restores_the_file_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let bashrc = dir.path().join(".bashrc");
        fs::write(&bashrc, BASHRC).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bashrc, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let op = ShellProfileOp::new(bashrc.clone(), BODY);
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        op.verify().unwrap();
        let text = fs::read_to_string(&bashrc).unwrap();
        assert!(text.starts_with("# ~/.bashrc\nexport EDITOR=vim\n\n# kiln:begin"));

        // The user edits the file afterwards; undo keeps their edit.
        fs::write(&bashrc, format!("{text}alias ll='ls -l'\n")).unwrap();
        op.revert(&receipt, false).unwrap();
        assert_eq!(
            fs::read_to_string(&bashrc).unwrap(),
            "# ~/.bashrc\nexport EDITOR=vim\n\nalias ll='ls -l'\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&bashrc).unwrap().permissions().mode() & 0o777,
                0o600,
                "mode kept"
            );
        }
    }

    #[test]
    fn a_file_kiln_created_goes_again_and_an_older_block_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let fish = dir.path().join("fish").join("conf.d").join("kiln.fish");
        let op = ShellProfileOp::new(fish.clone(), "source /k/env.fish");
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        assert!(fish.is_file());
        op.revert(&receipt, false).unwrap();
        assert!(!fish.exists(), "created by Kiln, empty again: removed");
        op.revert(&receipt, false).unwrap(); // idempotent

        // A block from an earlier install is restored as it was, not removed.
        let profile = dir.path().join(".profile");
        fs::write(
            &profile,
            upsert_block(b"umask 022\n", ". /old/env").unwrap(),
        )
        .unwrap();
        let op = ShellProfileOp::new(profile.clone(), BODY);
        let receipt = op.prepare().unwrap();
        op.apply(&receipt).unwrap();
        op.revert(&receipt, false).unwrap();
        assert_eq!(
            read_block(&fs::read(&profile).unwrap()).unwrap().as_deref(),
            Some(". /old/env")
        );

        let rebuilt = ShellProfileOp::from_params(&op.params()).unwrap();
        assert_eq!(rebuilt.params(), op.params());
    }
}
