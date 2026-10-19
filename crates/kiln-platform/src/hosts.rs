//! Editing Kiln's managed block in a hosts file.
//!
//! Guarantees, all covered by tests:
//! - Bytes outside Kiln's markers are preserved exactly, including line endings, comments and
//!   other tools' managed blocks (for example Herd's).
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
fn find_block(text: &str) -> Result<Option<std::ops::Range<usize>>, HostsError> {
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        match line.trim_end_matches(['\r', '\n']).trim() {
            l if l == BEGIN_MARKER => begins.push(offset),
            l if l == END_MARKER => ends.push(offset + line.len()),
            _ => {}
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

fn line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn render_block(entries: &[DnsEntry], eol: &str) -> Result<String, HostsError> {
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
    Ok(block)
}

/// Returns `text` with Kiln's block replaced by `entries`, or appended if there was none.
/// An empty `entries` list removes the block (see [`remove_block`]).
pub fn upsert_block(text: &str, entries: &[DnsEntry]) -> Result<String, HostsError> {
    if entries.is_empty() {
        return remove_block(text);
    }
    let eol = line_ending(text);
    let block = render_block(entries, eol)?;
    Ok(match find_block(text)? {
        Some(range) => format!("{}{block}{}", &text[..range.start], &text[range.end..]),
        None if text.is_empty() => block,
        // Exactly one line ending is added in both cases, which `remove_block` strips again:
        // a blank separator line after a terminated file, or the missing terminator otherwise.
        None => format!("{text}{eol}{block}"),
    })
}

/// Returns `text` without Kiln's block, undoing the separator [`upsert_block`] added.
pub fn remove_block(text: &str) -> Result<String, HostsError> {
    let Some(range) = find_block(text)? else {
        return Ok(text.to_owned());
    };
    let eol = line_ending(text);
    let mut before = &text[..range.start];
    let after = &text[range.end..];
    if after.is_empty() {
        // Strip the one line ending `upsert_block` added when appending, so append-then-remove
        // round-trips byte for byte.
        if let Some(stripped) = before.strip_suffix(eol) {
            before = stripped;
        }
    }
    Ok(format!("{before}{after}"))
}

/// Entries currently in Kiln's block, as `(ip, name)` text pairs. Used by `verify` and drift
/// detection.
pub fn read_block(text: &str) -> Result<Vec<(String, String)>, HostsError> {
    let Some(range) = find_block(text)? else {
        return Ok(Vec::new());
    };
    Ok(text[range]
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

    const WINDOWS_HOSTS: &str = "# Copyright (c) 1993-2009 Microsoft Corp.\r\n#\r\n# localhost name resolution is handled within DNS itself.\r\n#\t127.0.0.1       localhost\r\n\r\n# Other Tool generated Hosts. Do not change.\r\n127.0.0.1 other.test\r\n# End Other Tool generated Hosts\r\n";

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
        );
        assert!(
            !added.replace("\r\n", "").contains('\n'),
            "no bare LF in a CRLF file"
        );
    }

    #[test]
    fn replaces_existing_block_in_place() {
        let first = upsert_block(WINDOWS_HOSTS, &[entry("a.test")]).unwrap();
        let with_tail = format!("{first}10.0.0.5 nas.lan\r\n");
        let second = upsert_block(&with_tail, &[entry("c.test")]).unwrap();
        assert!(second.contains("127.0.0.1 c.test"));
        assert!(!second.contains("a.test"));
        assert!(second.ends_with("10.0.0.5 nas.lan\r\n"));
        assert_eq!(
            read_block(&second).unwrap(),
            vec![("127.0.0.1".into(), "c.test".into())]
        );
    }

    #[test]
    fn removal_round_trips_to_original_bytes() {
        for original in [
            WINDOWS_HOSTS,
            "127.0.0.1 localhost\n",
            "no trailing newline",
            "",
        ] {
            let updated = upsert_block(original, &[entry("a.test")]).unwrap();
            assert_eq!(remove_block(&updated).unwrap(), original, "{original:?}");
            assert_eq!(upsert_block(&updated, &[]).unwrap(), original);
        }
    }

    #[test]
    fn refuses_ambiguous_marker_layouts() {
        let block = upsert_block("", &[entry("a.test")]).unwrap();
        assert_eq!(
            upsert_block(&format!("{block}{block}"), &[entry("a.test")]),
            Err(HostsError::DuplicateBlock(2))
        );
        assert_eq!(
            remove_block(&format!("{BEGIN_MARKER}\n127.0.0.1 a.test\n")),
            Err(HostsError::UnterminatedBlock)
        );
        assert_eq!(
            remove_block(&format!("{END_MARKER}\n")),
            Err(HostsError::StrayEndMarker)
        );
    }

    #[test]
    fn only_loopback_addresses_are_written() {
        let mut lan = entry("a.test");
        lan.ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        assert!(matches!(
            upsert_block("", &[lan]),
            Err(HostsError::NotLoopback { .. })
        ));
        let mut v6 = entry("a.test");
        v6.ip = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert!(upsert_block("", &[v6]).unwrap().contains("::1 a.test"));
    }

    proptest! {
        #[test]
        fn foreign_content_survives_any_update_sequence(
            prefix in "[ -~\t]{0,40}(\r?\n[ -~\t]{0,40}){0,4}",
            batches in prop::collection::vec(prop::collection::vec("[a-z]{1,8}", 0..4), 1..5),
        ) {
            prop_assume!(!prefix.contains("kiln:"));
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
