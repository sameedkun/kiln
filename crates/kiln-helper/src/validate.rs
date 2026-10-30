//! Request validation inside the helper. The helper never trusts the client's checks; it
//! re-validates every field with the same domain types the rest of Kiln uses.

use kiln_core::{HostName, Tld};
use kiln_platform::DnsEntry;

use crate::proto::{HostEntry, MAX_ENTRIES};

/// Validates a `HostsSetBlock` request: TLD allowed (public TLDs refused), every name under
/// it, loopback addresses only, entry count bounded.
pub fn hosts_entries(tld: &str, entries: &[HostEntry]) -> Result<Vec<DnsEntry>, String> {
    if entries.len() > MAX_ENTRIES {
        return Err(format!(
            "{} entries exceed the limit of {MAX_ENTRIES}",
            entries.len()
        ));
    }
    let tld = Tld::recorded(tld).map_err(|e| e.to_string())?;
    entries
        .iter()
        .map(|entry| {
            if !entry.ip.is_loopback() {
                return Err(format!(
                    "{} maps to {}, which is not loopback",
                    entry.name, entry.ip
                ));
            }
            Ok(DnsEntry {
                name: HostName::new(&entry.name, &tld).map_err(|e| e.to_string())?,
                ip: entry.ip,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, ip: &str) -> HostEntry {
        HostEntry {
            name: name.into(),
            ip: ip.parse().unwrap(),
        }
    }

    #[test]
    fn accepts_loopback_names_under_the_tld() {
        let ok = hosts_entries(
            "test",
            &[
                entry("blog.test", "127.0.0.1"),
                entry("api.blog.test", "::1"),
            ],
        )
        .unwrap();
        assert_eq!(ok.len(), 2);
    }

    #[test]
    fn rejects_everything_else() {
        for (tld, e) in [
            ("com", entry("bank.com", "127.0.0.1")),  // public TLD
            ("test", entry("bank.com", "127.0.0.1")), // name outside the TLD
            ("test", entry("blog.test", "10.0.0.5")), // not loopback
            ("test", entry("blog.test 1.2.3.4", "127.0.0.1")), // injection attempt
            ("test", entry("", "127.0.0.1")),
        ] {
            assert!(
                hosts_entries(tld, std::slice::from_ref(&e)).is_err(),
                "{tld} {e:?}"
            );
        }
        let too_many = vec![entry("a.test", "127.0.0.1"); MAX_ENTRIES + 1];
        assert!(hosts_entries("test", &too_many).is_err());
    }
}
