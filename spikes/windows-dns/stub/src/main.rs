//! Spike 02 throwaway DNS stub. `dns-stub [tld] [bind-addr...]`
//! Defaults: tld `test`, binds `127.0.0.1:53` and `[::1]:53`.
//! A/AAAA for `*.tld` (and `tld` itself) → 127.0.0.1 / ::1; other types under the tld → NOERROR
//! with no answers; any other name → REFUSED. UDP only (what the Windows DNS client uses here).

use std::net::UdpSocket;
use std::thread;

fn parse_qname(msg: &[u8]) -> Option<(String, usize)> {
    let mut pos = 12;
    let mut labels = Vec::new();
    loop {
        let len = *msg.get(pos)? as usize;
        pos += 1;
        if len == 0 {
            break;
        }
        if len & 0xC0 != 0 {
            return None; // no compression in questions
        }
        labels.push(String::from_utf8_lossy(msg.get(pos..pos + len)?).to_ascii_lowercase());
        pos += len;
    }
    Some((labels.join("."), pos))
}

fn answer(query: &[u8], tld: &str) -> Option<Vec<u8>> {
    if query.len() < 12 || u16::from_be_bytes([query[4], query[5]]) != 1 {
        return None;
    }
    let (name, end) = parse_qname(query)?;
    let qtype = u16::from_be_bytes([*query.get(end)?, *query.get(end + 1)?]);
    let question = &query[12..end + 4];
    let ours = name == tld || name.ends_with(&format!(".{tld}"));

    let mut resp = Vec::with_capacity(64);
    resp.extend_from_slice(&query[0..2]); // id
    let rd = query[2] & 0x01;
    let rcode: u8 = if ours { 0 } else { 5 }; // NOERROR / REFUSED
    resp.push(0x84 | rd); // QR=1, AA=1, RD copied
    resp.push(rcode);
    resp.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    let rdata: Option<&[u8]> = match (ours, qtype) {
        (true, 1) => Some(&[127, 0, 0, 1]),
        (true, 28) => Some(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
        _ => None,
    };
    resp.extend_from_slice(&u16::from(rdata.is_some()).to_be_bytes()); // ancount
    resp.extend_from_slice(&[0, 0, 0, 0]); // nscount, arcount
    resp.extend_from_slice(question);
    if let Some(rdata) = rdata {
        resp.extend_from_slice(&[0xC0, 0x0C]); // pointer to qname
        resp.extend_from_slice(&qtype.to_be_bytes());
        resp.extend_from_slice(&1u16.to_be_bytes()); // IN
        resp.extend_from_slice(&5u32.to_be_bytes()); // TTL 5 s
        resp.extend_from_slice(&u16::try_from(rdata.len()).unwrap().to_be_bytes());
        resp.extend_from_slice(rdata);
    }
    Some(resp)
}

fn serve(sock: &UdpSocket, tld: &str) {
    let mut buf = [0u8; 512];
    loop {
        let Ok((n, peer)) = sock.recv_from(&mut buf) else { continue };
        if let Some(resp) = answer(&buf[..n], tld) {
            let _ = sock.send_to(&resp, peer);
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let tld = args.next().unwrap_or_else(|| "test".into());
    let mut binds: Vec<String> = args.collect();
    if binds.is_empty() {
        binds = vec!["127.0.0.1:53".into(), "[::1]:53".into()];
    }
    let mut handles = Vec::new();
    for addr in binds {
        match UdpSocket::bind(&addr) {
            Ok(sock) => {
                println!("listening={addr}");
                let tld = tld.clone();
                handles.push(thread::spawn(move || serve(&sock, &tld)));
            }
            Err(e) => {
                println!("bind_error={addr} {e}");
                std::process::exit(2);
            }
        }
    }
    for h in handles {
        let _ = h.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(u8::try_from(label.len()).unwrap());
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    #[test]
    fn answers_wildcards_under_tld_and_refuses_others() {
        let a = answer(&query("deep.blog.test", 1), "test").unwrap();
        assert_eq!(a[3] & 0x0F, 0);
        assert_eq!(u16::from_be_bytes([a[6], a[7]]), 1);
        assert_eq!(&a[a.len() - 4..], &[127, 0, 0, 1]);
        let r = answer(&query("example.com", 1), "test").unwrap();
        assert_eq!(r[3] & 0x0F, 5);
        let mx = answer(&query("blog.test", 15), "test").unwrap();
        assert_eq!(u16::from_be_bytes([mx[6], mx[7]]), 0);
        assert!(answer(&query("nottest", 1), "test").unwrap()[3] & 0x0F == 5);
    }
}
