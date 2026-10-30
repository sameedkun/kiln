//! Proxy targets ([ADR 0032] §2): the local dev server a proxy site forwards to. Loopback
//! only, so Kiln never forwards a `.test` name to another machine.
//!
//! [ADR 0032]: ../../../docs/adr/0032-proxy-sites.md

use std::fmt;
use std::net::IpAddr;

/// A loopback `http(s)://host:port`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyTarget {
    pub https: bool,
    /// `localhost`, an IPv4 loopback address, or `::1` (without brackets).
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    #[error("`{0}` isn't a port, host:port or http(s)://host:port")]
    Malformed(String),
    #[error("`{0}` isn't on this machine; a proxy target must be localhost, 127.0.0.1 or [::1]")]
    NotLoopback(String),
    #[error("`{0}` has a path, query or user name; give just http(s)://host:port")]
    Extra(String),
}

impl ProxyTarget {
    /// Reads `5173`, `localhost:5173`, `127.0.0.1:3000`, `[::1]:5173`,
    /// `http://localhost:5173` or `https://localhost:8443` (a trailing `/` is fine).
    pub fn parse(text: &str) -> Result<Self, TargetError> {
        let text = text.trim();
        let malformed = || TargetError::Malformed(text.to_owned());
        if let Ok(port) = text.parse::<u16>() {
            return Self::new(false, "localhost", port).ok_or_else(malformed);
        }
        let lower = text.to_ascii_lowercase();
        let (https, rest) = if let Some(rest) = lower.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = lower.strip_prefix("http://") {
            (false, rest)
        } else if lower.contains("://") {
            return Err(malformed());
        } else {
            (false, lower.as_str())
        };
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.contains(['/', '?', '#', '@']) {
            return Err(TargetError::Extra(text.to_owned()));
        }
        let (host, port) = if let Some(bracketed) = rest.strip_prefix('[') {
            let (host, after) = bracketed.split_once(']').ok_or_else(malformed)?;
            (host, after.strip_prefix(':'))
        } else {
            match rest.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (rest, None),
            }
        };
        let port = match port {
            Some(port) => port.parse::<u16>().map_err(|_| malformed())?,
            None if text.contains("://") => {
                if https {
                    443
                } else {
                    80
                }
            }
            None => return Err(malformed()),
        };
        if host.is_empty() {
            return Err(malformed());
        }
        if !is_loopback(host) {
            return Err(TargetError::NotLoopback(text.to_owned()));
        }
        Self::new(https, host, port).ok_or_else(malformed)
    }

    fn new(https: bool, host: &str, port: u16) -> Option<Self> {
        (port != 0).then(|| Self {
            https,
            host: host.to_owned(),
            port,
        })
    }

    /// What Caddy dials: `localhost:5173`, `[::1]:5173`.
    #[must_use]
    pub fn dial(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

impl fmt::Display for ProxyTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.https { "https" } else { "http" };
        write!(f, "{scheme}://{}", self.dial())
    }
}

fn is_loopback(host: &str) -> bool {
    host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> String {
        ProxyTarget::parse(text).unwrap().to_string()
    }

    #[test]
    fn ports_host_ports_and_urls_are_read() {
        assert_eq!(parsed("5173"), "http://localhost:5173");
        assert_eq!(parsed("localhost:3000"), "http://localhost:3000");
        assert_eq!(parsed("127.0.0.1:8000"), "http://127.0.0.1:8000");
        assert_eq!(parsed("127.1.2.3:8000"), "http://127.1.2.3:8000");
        assert_eq!(parsed("[::1]:5173"), "http://[::1]:5173");
        assert_eq!(parsed("HTTP://LocalHost:5173/"), "http://localhost:5173");
        assert_eq!(parsed("https://localhost:8443"), "https://localhost:8443");
        assert_eq!(parsed("https://localhost"), "https://localhost:443");
        assert_eq!(parsed("http://[::1]"), "http://[::1]:80");
        assert_eq!(
            ProxyTarget::parse("[::1]:5173").unwrap().dial(),
            "[::1]:5173"
        );
    }

    #[test]
    fn only_loopback_is_allowed() {
        for text in [
            "192.168.1.10:3000",
            "http://example.com:80",
            "https://10.0.0.1",
            "[::2]:80",
            "0.0.0.0:5173",
            "localhost.example.com:80",
        ] {
            assert!(
                matches!(ProxyTarget::parse(text), Err(TargetError::NotLoopback(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn malformed_and_extra_parts_are_refused() {
        for text in [
            "",
            "0",
            "70000",
            "localhost",
            "ftp://localhost:21",
            "localhost:x",
            ":80",
        ] {
            assert!(
                matches!(ProxyTarget::parse(text), Err(TargetError::Malformed(_))),
                "{text}"
            );
        }
        for text in [
            "http://localhost:5173/app",
            "http://localhost:5173?x=1",
            "http://user@localhost:5173",
        ] {
            assert!(
                matches!(ProxyTarget::parse(text), Err(TargetError::Extra(_))),
                "{text}"
            );
        }
    }
}
