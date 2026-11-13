//! Following a log ([ADR 0059] §3): what was written since the last read, by byte offset.
//!
//! [ADR 0059]: ../../../docs/adr/0059-log-viewer.md

use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;

/// The most read in one call: a follow catches up in steps rather than all at once.
pub const MOST: u64 = 1 << 20;

/// What a read of a log gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    /// Complete lines, oldest first.
    pub lines: Vec<String>,
    /// Where the next read starts: just after the last complete line read.
    pub offset: u64,
    /// The file is shorter than the offset asked for: it was rolled or emptied, and these
    /// lines are from its start.
    pub reset: bool,
}

/// Reads a log. Without `after`: its last `lines` complete lines. With `after`: the complete
/// lines written since that offset (at most [`MOST`] bytes; the rest on the next call). A line
/// still being written stays for the next call. A missing file is empty.
pub fn read(path: &Path, after: Option<u64>, lines: usize) -> std::io::Result<Tail> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Ok(Tail {
                lines: Vec::new(),
                offset: 0,
                reset: after.is_some_and(|a| a > 0),
            });
        }
        Err(e) => return Err(e),
    };
    let len = file.metadata()?.len();
    let reset = after.is_some_and(|a| a > len);
    let (start, cut_first) = match after {
        Some(a) if a <= len => (a, false),
        Some(_) => (0, false),
        None => {
            let start = len.saturating_sub(MOST);
            (start, start > 0)
        }
    };
    let end = if after.is_some() {
        len.min(start + MOST)
    } else {
        len
    };
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; usize::try_from(end - start).unwrap_or(0)];
    file.read_exact(&mut bytes)?;
    // Only complete lines: the rest is read again next time.
    let mut complete = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    // A whole chunk without a line end would never move on: take it as one (cut) line.
    if complete == 0 && after.is_some() && end - start == MOST {
        complete = bytes.len();
    }
    let mut skip = 0;
    if cut_first {
        // Started mid-file: the first line is likely cut.
        skip = bytes[..complete]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(complete, |i| i + 1);
    }
    let text = String::from_utf8_lossy(&bytes[skip..complete]);
    let mut all: Vec<String> = text
        .lines()
        .map(|l| l.trim_end_matches('\r').to_owned())
        .collect();
    if after.is_none() || reset {
        all.drain(..all.len().saturating_sub(lines));
    }
    Ok(Tail {
        lines: all,
        offset: start + complete as u64,
        reset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn append(path: &Path, text: &str) {
        File::options()
            .create(true)
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    #[test]
    fn the_last_lines_then_only_what_was_written_since() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("a.log");
        append(&log, "1\n2\n3\n");
        let first = read(&log, None, 2).unwrap();
        assert_eq!(first.lines, ["2", "3"]);
        assert_eq!(first.offset, 6);
        assert!(!first.reset);

        assert_eq!(read(&log, Some(first.offset), 200).unwrap().lines.len(), 0);
        append(&log, "4\r\n5\n");
        let next = read(&log, Some(first.offset), 200).unwrap();
        assert_eq!(next.lines, ["4", "5"]);
        assert_eq!(next.offset, 11);
    }

    #[test]
    fn a_line_still_being_written_waits() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("a.log");
        append(&log, "done\nhalf");
        let first = read(&log, None, 10).unwrap();
        assert_eq!(first.lines, ["done"]);
        append(&log, " and more\n");
        assert_eq!(
            read(&log, Some(first.offset), 10).unwrap().lines,
            ["half and more"]
        );
    }

    #[test]
    fn a_rolled_log_starts_again() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("a.log");
        append(&log, "a long first line\nanother\n");
        let first = read(&log, None, 10).unwrap();
        std::fs::write(&log, "new\n").unwrap();
        let next = read(&log, Some(first.offset), 10).unwrap();
        assert!(next.reset);
        assert_eq!(next.lines, ["new"]);
        assert_eq!(next.offset, 4);
    }

    #[test]
    fn a_line_longer_than_one_read_moves_on() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("a.log");
        append(&log, &"x".repeat(usize::try_from(MOST).unwrap() + 10));
        let first = read(&log, Some(0), 10).unwrap();
        assert_eq!(first.offset, MOST);
        assert_eq!(first.lines.len(), 1);
        append(&log, "\nnext\n");
        let rest = read(&log, Some(first.offset), 10).unwrap();
        assert_eq!(rest.lines, ["xxxxxxxxxx", "next"]);
    }

    #[test]
    fn a_missing_log_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let tail = read(&dir.path().join("none.log"), None, 10).unwrap();
        assert_eq!(tail.lines.len(), 0);
        assert_eq!(tail.offset, 0);
    }
}
