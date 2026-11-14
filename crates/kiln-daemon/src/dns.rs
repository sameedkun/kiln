//! Names without the hosts file ([ADR 0078]): kilnd answers DNS questions for Kiln's TLD on
//! UDP 127.0.0.1:53, once Windows has been told (one Name Resolution Policy rule, added with
//! the user's consent) to ask it for that TLD. Every name under the TLD is this PC; any other
//! type of question under it gets an empty answer (so IPv6 and HTTPS-record lookups fall back
//! to the address); a name outside it, "no such name". Loopback only (principle 3).
//!
//! [ADR 0078]: ../../../docs/adr/0078-names-without-the-hosts-file.md

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

/// How long an answer may be kept by the asker.
const TTL: u32 = 60;
/// The most a question may be (a classic UDP message).
const MOST: usize = 512;

/// A DNS question for one name, as far as Kiln reads one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Question {
    id: [u8; 2],
    /// "Recursion desired", echoed back.
    rd: bool,
    name: String,
    qtype: u16,
    qclass: u16,
    /// Where the question section ends (the answer copies it).
    end: usize,
}

/// Reads a standard query with exactly one question, or `None` for anything else (a
/// response, another opcode, a truncated or malformed message, a compression pointer in the
/// question). Never panics: every read is bounds-checked.
fn parse(message: &[u8]) -> Option<Question> {
    if message.len() < 12 || message.len() > MOST {
        return None;
    }
    let flags = message[2];
    // QR must be 0 (a query) and the opcode 0 (a standard query).
    if flags & 0x80 != 0 || (flags >> 3) & 0x0f != 0 {
        return None;
    }
    let qdcount = u16::from_be_bytes([message[4], message[5]]);
    if qdcount != 1 {
        return None;
    }
    let mut at = 12;
    let mut labels: Vec<String> = Vec::new();
    let mut length = 0usize;
    loop {
        let size = usize::from(*message.get(at)?);
        if size == 0 {
            at += 1;
            break;
        }
        // A compression pointer (0xC0) or a reserved form: not in a question Kiln reads.
        if size > 63 {
            return None;
        }
        let label = message.get(at + 1..at + 1 + size)?;
        length += size + 1;
        if length > 255 {
            return None;
        }
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        at += 1 + size;
    }
    let qtype = u16::from_be_bytes([*message.get(at)?, *message.get(at + 1)?]);
    let qclass = u16::from_be_bytes([*message.get(at + 2)?, *message.get(at + 3)?]);
    Some(Question {
        id: [message[0], message[1]],
        rd: flags & 0x01 != 0,
        name: labels.join("."),
        qtype,
        qclass,
        end: at + 4,
    })
}

/// Whether `name` is `tld` or under it.
fn ours(name: &str, tld: &str) -> bool {
    name == tld || name.ends_with(&format!(".{tld}"))
}

/// The answer to `message` for Kiln's `tld`, or `None` to drop it (not a question Kiln reads).
#[must_use]
pub fn answer(message: &[u8], tld: &str) -> Option<Vec<u8>> {
    let q = parse(message)?;
    let mine = ours(&q.name, tld);
    // An address for an IN A question under the TLD; nothing else gets records.
    let address = mine && q.qtype == 1 && q.qclass == 1;
    let mut out = Vec::with_capacity(q.end + 16);
    out.extend_from_slice(&q.id);
    // QR, authoritative, RD as asked; no recursion available.
    out.push(0x84 | u8::from(q.rd));
    // NOERROR for ours (empty when no address), NXDOMAIN otherwise.
    out.push(if mine { 0x00 } else { 0x03 });
    out.extend_from_slice(&[0, 1, 0, u8::from(address), 0, 0, 0, 0]);
    out.extend_from_slice(&message[12..q.end]);
    if address {
        // A pointer to the name at offset 12, type A, class IN, the TTL, 4 bytes: 127.0.0.1.
        out.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1]);
        out.extend_from_slice(&TTL.to_be_bytes());
        out.extend_from_slice(&[0, 4]);
        out.extend_from_slice(&Ipv4Addr::LOCALHOST.octets());
    }
    Some(out)
}

/// The setting that turns the responder on: set when Windows' rule is added, cleared when it
/// goes (`kiln dns on|off`, ADR 0078).
pub const ON_KEY: &str = "dns.wildcard";

/// Answers on UDP 127.0.0.1:53 while `on()` says so, reading Kiln's TLD from `tld` (it can
/// change while kilnd runs). Returns when turned off, or when the socket fails (another
/// program holds 127.0.0.1:53, say).
pub fn serve(tld: &dyn Fn() -> String, on: &dyn Fn() -> bool) -> std::io::Result<()> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 53)))?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; MOST];
    let mut domain = tld();
    loop {
        match socket.recv_from(&mut buf) {
            Ok((size, from)) => {
                if let Some(reply) = answer(&buf[..size], &domain) {
                    let _ = socket.send_to(&reply, from);
                }
            }
            // Quiet for a while: is it still wanted, and under which TLD?
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if !on() {
                    return Ok(());
                }
                domain = tld();
            }
            // A peer that went away (Windows reports it on the next receive): carry on.
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A query for `name` of `qtype`, class IN, recursion desired.
    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut m = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            m.push(u8::try_from(label.len()).unwrap());
            m.extend_from_slice(label.as_bytes());
        }
        m.push(0);
        m.extend_from_slice(&qtype.to_be_bytes());
        m.extend_from_slice(&[0, 1]);
        m
    }

    #[test]
    fn a_name_under_the_tld_is_this_pc() {
        let reply = answer(&query("Shop.Test", 1), "test").unwrap();
        assert_eq!(&reply[..2], &[0x12, 0x34], "same id");
        assert_eq!(reply[2] & 0x80, 0x80, "a response");
        assert_eq!(reply[3] & 0x0f, 0, "NOERROR");
        assert_eq!(&reply[6..8], &[0, 1], "one answer");
        assert_eq!(&reply[reply.len() - 4..], &[127, 0, 0, 1]);
        let deeper = answer(&query("api.shop.test", 1), "test").unwrap();
        assert_eq!(&deeper[deeper.len() - 4..], &[127, 0, 0, 1]);
    }

    #[test]
    fn other_types_under_the_tld_are_empty_and_other_names_dont_exist() {
        for qtype in [28, 65] {
            let reply = answer(&query("shop.test", qtype), "test").unwrap();
            assert_eq!(reply[3] & 0x0f, 0, "NOERROR for type {qtype}");
            assert_eq!(&reply[6..8], &[0, 0], "no answer for type {qtype}");
        }
        let elsewhere = answer(&query("example.com", 1), "test").unwrap();
        assert_eq!(elsewhere[3] & 0x0f, 3, "NXDOMAIN");
        // "attest" isn't under "test".
        assert_eq!(answer(&query("attest", 1), "test").unwrap()[3] & 0x0f, 3);
    }

    #[test]
    fn responses_truncated_messages_and_pointers_are_dropped() {
        let mut response = query("shop.test", 1);
        response[2] |= 0x80;
        assert_eq!(answer(&response, "test"), None);
        assert_eq!(answer(&query("shop.test", 1)[..20], "test"), None);
        let mut pointer = vec![
            0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 0xC0, 0x0C, 0, 1, 0, 1,
        ];
        assert_eq!(answer(&pointer, "test"), None);
        pointer[4] = 0;
        pointer[5] = 2;
        assert_eq!(answer(&pointer, "test"), None, "two questions");
    }

    proptest! {
        /// Whatever arrives, kilnd doesn't panic, and an answer echoes the question's id.
        #[test]
        fn any_bytes_are_answered_or_dropped_without_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
            if let Some(reply) = answer(&bytes, "test") {
                prop_assert_eq!(&reply[..2], &bytes[..2]);
                prop_assert!(reply.len() <= MOST + 16);
            }
        }

        #[test]
        fn any_name_under_the_tld_gets_loopback(label in "[a-z0-9-]{1,30}") {
            let reply = answer(&query(&format!("{label}.test"), 1), "test").unwrap();
            prop_assert_eq!(&reply[reply.len() - 4..], &[127, 0, 0, 1]);
        }
    }
}
