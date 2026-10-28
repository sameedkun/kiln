//! Node version specs ([ADR 0028] §4): what `kiln node install`, `.nvmrc` and `.node-version`
//! accept.
//!
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use std::fmt;

/// A Node version, as three numbers.
pub type Version = [u32; 3];

/// Parses `22.11.0` or `v22.11.0`.
#[must_use]
pub fn parse_version(text: &str) -> Option<Version> {
    let numbers = numbers(text)?;
    numbers.try_into().ok()
}

fn numbers(text: &str) -> Option<Vec<u32>> {
    let text = text.trim();
    let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
    text.split('.').map(|p| p.parse().ok()).collect()
}

/// Which releases a spec means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    /// `22`, `22.11` or `22.11.0` (a leading `v` allowed): the newest match.
    Version(Vec<u32>),
    /// `lts` / `lts/*` (`None`), or `lts/jod`: the newest LTS, or of that LTS line.
    Lts(Option<String>),
    /// `node`, `latest` or `current`: the newest release.
    Latest,
}

impl Spec {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        match lower.as_str() {
            "lts" | "lts/*" => return Ok(Self::Lts(None)),
            "node" | "latest" | "current" | "stable" => return Ok(Self::Latest),
            _ => {}
        }
        if let Some(name) = lower.strip_prefix("lts/") {
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase()) {
                return Ok(Self::Lts(Some(name.to_owned())));
            }
        } else if let Some(numbers) = numbers(text).filter(|n| (1..=3).contains(&n.len())) {
            return Ok(Self::Version(numbers));
        }
        Err(format!(
            "{text:?} isn't a Node version Kiln understands (e.g. 22, 22.11.0, lts, lts/jod)"
        ))
    }

    /// Whether a release (its version and LTS codename, lower case) is one this spec means.
    #[must_use]
    pub fn matches(&self, version: Version, lts: Option<&str>) -> bool {
        match self {
            Self::Version(want) => version[..want.len()] == want[..],
            Self::Lts(None) => lts.is_some(),
            Self::Lts(Some(name)) => lts == Some(name.as_str()),
            Self::Latest => true,
        }
    }

    /// The newest item that matches, given how to read its version and LTS codename.
    pub fn newest<'a, T>(
        &self,
        items: &'a [T],
        info: impl Fn(&T) -> (Version, Option<&str>),
    ) -> Option<&'a T> {
        items
            .iter()
            .filter(|item| {
                let (version, lts) = info(item);
                self.matches(version, lts)
            })
            .max_by_key(|item| info(item).0)
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(n) => {
                let parts: Vec<String> = n.iter().map(u32::to_string).collect();
                f.write_str(&parts.join("."))
            }
            Self::Lts(None) => f.write_str("lts"),
            Self::Lts(Some(name)) => write!(f, "lts/{name}"),
            Self::Latest => f.write_str("latest"),
        }
    }
}

/// The spec in an `.nvmrc` or `.node-version` file: its first line that isn't empty or a
/// comment.
#[must_use]
pub fn from_version_file(text: &str) -> Option<&str> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .find(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_lts_and_latest() {
        assert_eq!(Spec::parse("22"), Ok(Spec::Version(vec![22])));
        assert_eq!(Spec::parse("v22.11"), Ok(Spec::Version(vec![22, 11])));
        assert_eq!(
            Spec::parse(" 22.11.0\n"),
            Ok(Spec::Version(vec![22, 11, 0]))
        );
        assert_eq!(Spec::parse("lts/*"), Ok(Spec::Lts(None)));
        assert_eq!(Spec::parse("LTS"), Ok(Spec::Lts(None)));
        assert_eq!(Spec::parse("lts/Jod"), Ok(Spec::Lts(Some("jod".into()))));
        assert_eq!(Spec::parse("node"), Ok(Spec::Latest));
        for bad in ["", "lts/-1", "22.x", "1.2.3.4", "lts/", "iojs"] {
            assert!(Spec::parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(Spec::parse("v22.11").unwrap().to_string(), "22.11");
        assert_eq!(parse_version("v22.11.0"), Some([22, 11, 0]));
        assert_eq!(parse_version("22.11"), None);
    }

    #[test]
    fn picks_the_newest_match() {
        let releases = [
            ([20, 19, 0], Some("iron")),
            ([22, 10, 0], Some("jod")),
            ([22, 11, 0], Some("jod")),
            ([23, 1, 0], None),
        ];
        let newest = |spec: &str| {
            Spec::parse(spec)
                .unwrap()
                .newest(&releases, |r| (r.0, r.1))
                .map(|r| r.0)
        };
        assert_eq!(newest("22"), Some([22, 11, 0]));
        assert_eq!(newest("22.10"), Some([22, 10, 0]));
        assert_eq!(newest("lts"), Some([22, 11, 0]));
        assert_eq!(newest("lts/iron"), Some([20, 19, 0]));
        assert_eq!(newest("latest"), Some([23, 1, 0]));
        assert_eq!(newest("2"), None, "22 doesn't start with 2 as a number");
        assert_eq!(newest("24"), None);
    }

    #[test]
    fn version_files_skip_blank_lines_and_comments() {
        assert_eq!(
            from_version_file("\n# pinned\nv22.11.0 # LTS\n"),
            Some("v22.11.0")
        );
        assert_eq!(from_version_file("lts/*\r\n"), Some("lts/*"));
        assert_eq!(from_version_file("\n\n"), None);
    }
}
