use std::fmt;

use serde::{Deserialize, Serialize};

const MAX_LABEL: usize = 63;
const MAX_NAME: usize = 253;

/// The development TLDs a user can choose ([ADR 0033] §2): names set aside so that they
/// never resolve on the public internet (RFC 2606, and ICANN's `.internal`).
///
/// [ADR 0033]: ../../../docs/adr/0033-configurable-tld.md
pub const ALLOWED_TLDS: &[&str] = &["test", "internal", "example", "invalid"];

/// Top-level domains Kiln never accepts, even in what it recorded under earlier rules,
/// because they resolve on the public internet or carry special meaning for operating
/// systems and browsers.
const PUBLIC_OR_SPECIAL_TLDS: &[&str] = &[
    "com",
    "net",
    "org",
    "dev",
    "app",
    "io",
    "localhost",
    "local",
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NameError {
    #[error("name is empty")]
    Empty,
    #[error("name is longer than {MAX_NAME} characters")]
    TooLong,
    #[error("label `{0}` is empty or longer than {MAX_LABEL} characters")]
    BadLabelLength(String),
    #[error("label `{0}` may only contain a-z, 0-9 and inner hyphens")]
    BadLabelChars(String),
    #[error("`{0}` is not allowed as a development TLD")]
    ReservedTld(String),
    #[error("`{0}` isn't a reserved development TLD; use test, internal, example or invalid")]
    NotReservedTld(String),
    #[error("`{name}` does not end in the managed TLD `.{tld}`")]
    WrongTld { name: String, tld: String },
}

fn check_label(label: &str) -> Result<(), NameError> {
    if label.is_empty() || label.len() > MAX_LABEL {
        return Err(NameError::BadLabelLength(label.to_owned()));
    }
    let valid_chars = label
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid_chars || label.starts_with('-') || label.ends_with('-') {
        return Err(NameError::BadLabelChars(label.to_owned()));
    }
    Ok(())
}

/// The development TLD sites are served under, e.g. `test`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Tld(String);

impl Tld {
    /// A TLD the user chooses: one of [`ALLOWED_TLDS`].
    pub fn new(value: &str) -> Result<Self, NameError> {
        let tld = Self::recorded(value)?;
        if !ALLOWED_TLDS.contains(&tld.as_str()) {
            return Err(NameError::NotReservedTld(tld.0));
        }
        Ok(tld)
    }

    /// A TLD Kiln recorded itself (a journal entry, a root's name constraint, its setting),
    /// checked by the earlier rules: any label but a public or special one. Earlier versions
    /// let users choose such names, and Kiln must still undo and trust what they made
    /// ([ADR 0033] §6).
    ///
    /// [ADR 0033]: ../../../docs/adr/0033-configurable-tld.md
    pub fn recorded(value: &str) -> Result<Self, NameError> {
        let value = value.trim_start_matches('.').to_ascii_lowercase();
        check_label(&value)?;
        if PUBLIC_OR_SPECIAL_TLDS.contains(&value.as_str()) {
            return Err(NameError::ReservedTld(value));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Tld {
    fn default() -> Self {
        Self("test".to_owned())
    }
}

impl TryFrom<String> for Tld {
    type Error = NameError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Deserialising reads back what was written, so the earlier rules apply.
        Self::recorded(&value)
    }
}

impl From<Tld> for String {
    fn from(value: Tld) -> Self {
        value.0
    }
}

impl fmt::Display for Tld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A fully qualified, lowercase host name under a managed [`Tld`], e.g. `blog.test`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String")]
pub struct HostName(String);

impl HostName {
    /// Validates `name` as a DNS name ending in `.{tld}` with at least one label before it.
    pub fn new(name: &str, tld: &Tld) -> Result<Self, NameError> {
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if name.len() > MAX_NAME {
            return Err(NameError::TooLong);
        }
        let labels: Vec<&str> = name.split('.').collect();
        for label in &labels {
            check_label(label)?;
        }
        if labels.len() < 2 || labels.last() != Some(&tld.as_str()) {
            return Err(NameError::WrongTld {
                name,
                tld: tld.to_string(),
            });
        }
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<HostName> for String {
    fn from(value: HostName) -> Self {
        value.0
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn accepts_site_names_under_the_tld() {
        let tld = Tld::default();
        assert_eq!(
            HostName::new("Blog.TEST.", &tld).unwrap().as_str(),
            "blog.test"
        );
        assert!(HostName::new("api.my-app.test", &tld).is_ok());
    }

    #[test]
    fn rejects_names_outside_the_tld_or_malformed() {
        let tld = Tld::default();
        assert!(matches!(
            HostName::new("example.com", &tld),
            Err(NameError::WrongTld { .. })
        ));
        assert!(matches!(
            HostName::new("test", &tld),
            Err(NameError::WrongTld { .. })
        ));
        assert!(matches!(
            HostName::new("-bad.test", &tld),
            Err(NameError::BadLabelChars(_))
        ));
        assert!(matches!(
            HostName::new("a..test", &tld),
            Err(NameError::BadLabelLength(_))
        ));
        assert!(matches!(
            HostName::new("evil.test 1.2.3.4", &tld),
            Err(NameError::BadLabelChars(_))
        ));
        assert_eq!(HostName::new("", &tld), Err(NameError::Empty));
    }

    #[test]
    fn refuses_public_or_special_tlds() {
        for tld in ["com", "dev", "localhost", ".local"] {
            assert!(
                matches!(Tld::new(tld), Err(NameError::ReservedTld(_))),
                "{tld}"
            );
        }
        assert_eq!(Tld::new(".Internal").unwrap().as_str(), "internal");
    }

    #[test]
    fn only_reserved_tlds_can_be_chosen_but_earlier_ones_still_read_back() {
        for tld in ALLOWED_TLDS {
            assert_eq!(Tld::new(tld).unwrap().as_str(), *tld);
        }
        for tld in ["wip", "box", "lan"] {
            assert!(
                matches!(Tld::new(tld), Err(NameError::NotReservedTld(_))),
                "{tld}"
            );
            assert_eq!(Tld::recorded(tld).unwrap().as_str(), tld);
        }
        for tld in ["com", "local", "localhost"] {
            assert!(Tld::recorded(tld).is_err(), "{tld}");
        }
        let read: Tld = serde_json::from_str("\"wip\"").unwrap();
        assert_eq!(read.as_str(), "wip");
    }

    proptest! {
        #[test]
        fn validated_names_never_contain_whitespace_or_hash(s in "\\PC{0,80}") {
            if let Ok(name) = HostName::new(&s, &Tld::default()) {
                prop_assert!(!name.as_str().contains(|c: char| c.is_whitespace() || c == '#'));
                prop_assert_eq!(name.as_str().rsplit('.').next(), Some("test"));
            }
        }
    }
}
