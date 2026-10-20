//! Editing Kiln's managed block in a hosts file.
//!
//! The API works on raw bytes: hosts files are not guaranteed to be UTF-8 (a comment written in
//! a legacy Windows code page is enough), and Kiln must be able to edit them without touching
//! those bytes.
//!
//! Guarantees, all covered by tests:
//! - Bytes outside Kiln's markers are preserved exactly, including line endings, comments,
//!   non-UTF-8 content, a UTF-8 BOM and other tools' managed blocks (for example Herd's).
//! - The block uses the file's existing line ending (CRLF on Windows).
//! - Removing the block after inserting it into a file that had none returns the original
//!   bytes.
//! - Malformed or duplicated markers are refused instead of guessed at.
//! - Only loopback addresses can be written.

use std::fmt::Write as _;

use crate::DnsEntry;

pub const BEGIN_MARKER: &str =
    "# kiln:begin - managed by Kiln; edits inside this block are overwritten";
pub const END_MARKER: &str = "# kiln:end";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HostsError {
    #[error("hosts file contains {0} Kiln begin markers; fix the file by hand before continuing")]
    DuplicateBlock(usize),
    #[error("hosts file has a Kiln begin marker without a matching end marker")]
    UnterminatedBlock,
    #[error("hosts file has a Kiln end marker without a begin marker")]
    StrayEndMarker,
    #[error("{name} maps to {ip}, but Kiln only writes loopback addresses")]
    NotLoopback { name: String, ip: std::net::IpAddr },
}

/// Byte range of the managed block, including both marker lines and the end marker's newline.
fn find_block(text: &[u8]) -> Result<Option<std::ops::Range<usize>>, HostsError> {
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    let mut offset = 0;
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
        ([_], []) => Err(HostsError::UnterminatedBlock),
        ([] | [_], _) => Err(HostsError::StrayEndMarker),
        (many, _) => Err(HostsError::DuplicateBlock(many.len())),
    }
}

fn line_ending(text: &[u8]) -> &'static [u8] {
    if text.windows(2).any(|w| w == b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    }
}

/// The line ending an existing block was written with (its end-marker line's terminator).
/// Re-deriving it from the whole file is wrong: foreign content ending in a lone '\r' plus our
/// "\n" separator would look like CRLF on the next update.
fn block_line_ending(text: &[u8], block: &std::ops::Range<usize>) -> &'static [u8] {
    if text[..block.end].ends_with(b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    }
}

fn render_block(entries: &[DnsEntry], eol: &[u8]) -> Result<Vec<u8>, HostsError> {
    let eol = std::str::from_utf8(eol).expect("line endings are ASCII");
    let mut sorted = entries.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut block = format!("{BEGIN_MARKER}{eol}");
    for entry in &sorted {
        if !entry.ip.is_loopback() {
            return Err(HostsError::NotLoopback {
                name: entry.name.to_string(),
                ip: entry.ip,
            });
        }
        let _ = write!(block, "{} {}{eol}", entry.ip, entry.name);
    }
    block.push_str(END_MARKER);
    block.push_str(eol);
    Ok(block.into_bytes())
}

/// Returns `text` with Kiln's block replaced by `entries`, or appended if there was none.
/// An empty `entries` list removes the block (see [`remove_block`]).
pub fn upsert_block(text: &[u8], entries: &[DnsEntry]) -> Result<Vec<u8>, HostsError> {
    if entries.is_empty() {
        return remove_block(text);
    }
    if let Some(range) = find_block(text)? {
        let block = render_block(entries, block_line_ending(text, &range))?;
        return Ok([&text[..range.start], &block, &text[range.end..]].concat());
    }
    let eol = line_ending(text);
    let block = render_block(entries, eol)?;
    if text.is_empty() {
        return Ok(block);
    }
    // Exactly one line ending is added, which `remove_block` strips again: a blank separator
    // line after a terminated file, or the missing terminator otherwise.
    Ok([text, eol, &block].concat())
}

/// Returns `text` without Kiln's block, undoing the separator [`upsert_block`] added.
pub fn remove_block(text: &[u8]) -> Result<Vec<u8>, HostsError> {
    let Some(range) = find_block(text)? else {
        return Ok(text.to_vec());
    };
    let eol = block_line_ending(text, &range);
    let mut before = &text[..range.start];
    let after = &text[range.end..];
    if after.is_empty() {
        // Strip the one line ending `upsert_block` added when appending, so append-then-remove
        // round-trips byte for byte.
        if let Some(stripped) = before.strip_suffix(eol) {
            before = stripped;
        }
    }
    Ok([before, after].concat())
}

/// Entries currently in Kiln's block, as `(ip, name)` text pairs. Used by `verify` and drift
/// detection. Kiln only writes ASCII inside its block; anything else there is reported lossily.
pub fn read_block(text: &[u8]) -> Result<Vec<(String, String)>, HostsError> {
    let Some(range) = find_block(text)? else {
        return Ok(Vec::new());
    };
    Ok(String::from_utf8_lossy(&text[range])
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            Some((parts.next()?.to_owned(), parts.next()?.to_owned()))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_core::{HostName, Tld};
    use proptest::prelude::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    fn entry(name: &str) -> DnsEntry {
        DnsEntry {
            name: HostName::new(name, &Tld::default()).unwrap(),
            ip: LOCAL,
        }
    }

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
    }

    const WINDOWS_HOSTS: &[u8] = b"# Copyright (c) 1993-2009 Microsoft Corp.\r\n#\r\n# localhost name resolution is handled within DNS itself.\r\n#\t127.0.0.1       localhost\r\n\r\n# Other Tool generated Hosts. Do not change.\r\n127.0.0.1 other.test\r\n# End Other Tool generated Hosts\r\n";

    #[test]
    fn appends_block_with_crlf_and_preserves_everything_else() {
        let updated = upsert_block(WINDOWS_HOSTS, &[entry("b.test"), entry("a.test")]).unwrap();
        assert!(updated.starts_with(WINDOWS_HOSTS));
        let added = &updated[WINDOWS_HOSTS.len()..];
        assert_eq!(
            added,
            format!(
                "\r\n{BEGIN_MARKER}\r\n127.0.0.1 a.test\r\n127.0.0.1 b.test\r\n{END_MARKER}\r\n"
            )
            .as_bytes()
        );
        let bare_lf = added
            .iter()
            .enumerate()
            .any(|(i, &b)| b == b'\n' && (i == 0 || added[i - 1] != b'\r'));
        assert!(!bare_lf, "no bare LF in a CRLF file");
    }

    #[test]
    fn replaces_existing_block_in_place() {
        let first = upsert_block(WINDOWS_HOSTS, &[entry("a.test")]).unwrap();
        let with_tail = [first.as_slice(), b"10.0.0.5 nas.lan\r\n"].concat();
        let second = upsert_block(&with_tail, &[entry("c.test")]).unwrap();
        assert!(contains(&second, "127.0.0.1 c.test"));
        assert!(!contains(&second, "a.test"));
        assert!(second.ends_with(b"10.0.0.5 nas.lan\r\n"));
        assert_eq!(
            read_block(&second).unwrap(),
            vec![("127.0.0.1".into(), "c.test".into())]
        );
    }

    #[test]
    fn removal_round_trips_to_original_bytes() {
        let originals: [&[u8]; 4] = [
            WINDOWS_HOSTS,
            b"127.0.0.1 localhost\n",
            b"no trailing newline",
            b"",
        ];
        for original in originals {
            let updated = upsert_block(original, &[entry("a.test")]).unwrap();
            assert_eq!(remove_block(&updated).unwrap(), original);
            assert_eq!(upsert_block(&updated, &[]).unwrap(), original);
        }
    }

    /// Regression for spike 02: a Latin-1 comment (byte 0xE9) and a UTF-8 BOM must not stop
    /// Kiln from editing the file, and must survive untouched.
    #[test]
    fn edits_files_with_non_utf8_bytes_and_bom() {
        let original = [
            &[0xEF, 0xBB, 0xBF][..],
            WINDOWS_HOSTS,
            b"# caf\xE9 server\r\n",
        ]
        .concat();
        assert!(std::str::from_utf8(&original).is_err());
        let updated = upsert_block(&original, &[entry("a.test")]).unwrap();
        assert!(updated.starts_with(&original));
        assert_eq!(read_block(&updated).unwrap().len(), 1);
        assert_eq!(remove_block(&updated).unwrap(), original);
    }

    #[test]
    fn refuses_ambiguous_marker_layouts() {
        let block = upsert_block(b"", &[entry("a.test")]).unwrap();
        assert_eq!(
            upsert_block(&[block.as_slice(), &block].concat(), &[entry("a.test")]),
            Err(HostsError::DuplicateBlock(2))
        );
        assert_eq!(
            remove_block(format!("{BEGIN_MARKER}\n127.0.0.1 a.test\n").as_bytes()),
            Err(HostsError::UnterminatedBlock)
        );
        assert_eq!(
            remove_block(format!("{END_MARKER}\n").as_bytes()),
            Err(HostsError::StrayEndMarker)
        );
    }

    #[test]
    fn only_loopback_addresses_are_written() {
        let mut lan = entry("a.test");
        lan.ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        assert!(matches!(
            upsert_block(b"", &[lan]),
            Err(HostsError::NotLoopback { .. })
        ));
        let mut v6 = entry("a.test");
        v6.ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert!(contains(&upsert_block(b"", &[v6]).unwrap(), "::1 a.test"));
    }

    proptest! {
        /// Arbitrary foreign bytes (any encoding, any line endings) survive any update sequence
        /// and are restored exactly when the block is removed.
        #[test]
        fn foreign_bytes_survive_any_update_sequence(
            prefix in prop::collection::vec(any::<u8>(), 0..200),
            batches in prop::collection::vec(prop::collection::vec("[a-z]{1,8}", 0..4), 1..5),
        ) {
            prop_assume!(!prefix.windows(5).any(|w| w == b"kiln:"));
            let mut text = prefix.clone();
            for names in batches {
                let entries: Vec<_> = names.iter().map(|n| entry(&format!("{n}.test"))).collect();
                text = upsert_block(&text, &entries).unwrap();
                prop_assert!(text.starts_with(&prefix));
            }
            prop_assert_eq!(remove_block(&text).unwrap(), prefix);
        }
    }
}
