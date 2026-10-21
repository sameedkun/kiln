//! The windows.php.net release index (`releases.json`): the latest patch of each minor
//! version, with the SHA-256 of every build ([ADR 0013] §8–9).
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

use kiln_fetch::{Expected, plain_name};
use serde_json::Value;

/// Where the index and the archives it names live. `windows.php.net/downloads/releases/`
/// redirects here.
pub const BASE_URL: &str = "https://downloads.php.net/~windows/releases/";
pub const INDEX_URL: &str = "https://downloads.php.net/~windows/releases/releases.json";
/// The index is about 25 KB; anything far larger isn't it.
pub const INDEX_LIMIT: u64 = 2 * 1024 * 1024;

/// One installable PHP build: x64, non-thread-safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpRelease {
    /// `8.4`
    pub minor: String,
    /// `8.4.27`
    pub version: String,
    /// Build key in the index, e.g. `nts-vs17-x64`.
    pub build: String,
    /// Plain file name in [`BASE_URL`], e.g. `php-8.4.27-nts-Win32-vs17-x64.zip`.
    pub archive: String,
    pub sha256: Expected,
}

impl PhpRelease {
    #[must_use]
    pub fn url(&self) -> String {
        format!("{BASE_URL}{}", self.archive)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    #[error("the PHP release index isn't valid JSON: {0}")]
    NotJson(String),
    #[error("the PHP release index has an unexpected shape: {0}")]
    Shape(String),
    #[error("{0:?} isn't a PHP version (expected e.g. 8.4 or 8.4.27)")]
    BadSpec(String),
    #[error("PHP {requested} isn't in the release index. Installable versions: {available}")]
    Unknown {
        requested: String,
        available: String,
    },
    #[error(
        "PHP {requested} can't be verified: windows.php.net publishes checksums only for the \
         latest patch of each version. Install {latest} instead (`kiln php install {minor}`)."
    )]
    NotVerifiable {
        requested: String,
        latest: String,
        minor: String,
    },
}

fn shape(what: impl Into<String>) -> IndexError {
    IndexError::Shape(what.into())
}

/// `8.4` → (8, 4); `8.4.27` → (8, 4, 27) as numbers, for ordering.
fn numbers(version: &str) -> Option<Vec<u32>> {
    version.split('.').map(|p| p.parse().ok()).collect()
}

/// Parses the index into the x64 NTS build of each version, oldest first. Entries without such
/// a build are skipped; anything malformed in an entry that has one is an error.
pub fn parse(json: &str) -> Result<Vec<PhpRelease>, IndexError> {
    let root: Value = serde_json::from_str(json).map_err(|e| IndexError::NotJson(e.to_string()))?;
    let entries = root
        .as_object()
        .ok_or_else(|| shape("top level isn't an object"))?;
    let mut releases = Vec::new();
    for (minor, entry) in entries {
        if numbers(minor).is_none_or(|n| n.len() != 2) {
            continue;
        }
        let version = entry["version"]
            .as_str()
            .ok_or_else(|| shape(format!("{minor}: no version")))?;
        if numbers(version).is_none_or(|n| n.len() != 3)
            || !version.starts_with(&format!("{minor}."))
        {
            return Err(shape(format!("{minor}: odd version {version:?}")));
        }
        // nts-vc15-x64 (7.4), nts-vs16-x64 (8.0–8.3), nts-vs17-x64 (8.4+): take the newest
        // toolchain if several are listed.
        let Some((build, detail)) = entry
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| key.starts_with("nts-v") && key.ends_with("-x64"))
            .max_by_key(|(key, _)| key.as_str())
        else {
            continue;
        };
        let archive = detail["zip"]["path"]
            .as_str()
            .ok_or_else(|| shape(format!("{minor} {build}: no zip path")))?;
        plain_name(archive).map_err(|e| shape(format!("{minor} {build}: {e}")))?;
        let sha256 = detail["zip"]["sha256"]
            .as_str()
            .ok_or_else(|| shape(format!("{minor} {build}: no sha256")))?;
        let sha256 =
            Expected::sha256(sha256).map_err(|e| shape(format!("{minor} {build}: {e}")))?;
        releases.push(PhpRelease {
            minor: minor.clone(),
            version: version.to_owned(),
            build: build.clone(),
            archive: archive.to_owned(),
            sha256,
        });
    }
    releases.sort_by_key(|r| numbers(&r.version));
    Ok(releases)
}

/// Finds the release for `spec`: `8.4` (that minor's latest patch) or `8.4.27` (only if it is
/// the latest patch, since older ones can't be verified).
pub fn resolve<'a>(releases: &'a [PhpRelease], spec: &str) -> Result<&'a PhpRelease, IndexError> {
    let spec = spec.trim();
    let parts = numbers(spec).ok_or_else(|| IndexError::BadSpec(spec.to_owned()))?;
    if !(2..=3).contains(&parts.len()) {
        return Err(IndexError::BadSpec(spec.to_owned()));
    }
    let minor = format!("{}.{}", parts[0], parts[1]);
    let Some(release) = releases.iter().find(|r| r.minor == minor) else {
        return Err(IndexError::Unknown {
            requested: spec.to_owned(),
            available: releases
                .iter()
                .map(|r| r.minor.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        });
    };
    if parts.len() == 3 && numbers(&release.version) != Some(parts) {
        return Err(IndexError::NotVerifiable {
            requested: spec.to_owned(),
            latest: release.version.clone(),
            minor,
        });
    }
    Ok(release)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A trimmed copy of the real index's shape (2026-10-21).
    pub const SAMPLE: &str = r#"{
        "7.4": {"version": "7.4.33",
            "nts-vc15-x64": {"zip": {"path": "php-7.4.33-nts-Win32-vc15-x64.zip",
                "sha256": "1111111111111111111111111111111111111111111111111111111111111111"}},
            "ts-vc15-x64": {"zip": {"path": "php-7.4.33-Win32-vc15-x64.zip",
                "sha256": "2222222222222222222222222222222222222222222222222222222222222222"}},
            "source": {"path": "php-7.4.33-src.zip", "sha256": "33"}},
        "8.4": {"version": "8.4.27",
            "nts-vs17-x64": {"mtime": "2026-09-30T12:00:00+00:00",
                "zip": {"path": "php-8.4.27-nts-Win32-vs17-x64.zip", "size": "31.5MB",
                "sha256": "93BD8A815A52156606FB906D0573437908C61F96E2CC52C03A51E36A41BF5236"},
                "debug_pack": {"path": "x", "sha256": "y"}},
            "nts-vs17-x86": {"zip": {"path": "php-8.4.27-nts-Win32-vs17-x86.zip",
                "sha256": "4444444444444444444444444444444444444444444444444444444444444444"}}},
        "8.3": {"version": "8.3.36",
            "nts-vs16-x64": {"zip": {"path": "php-8.3.36-nts-Win32-vs16-x64.zip",
                "sha256": "5555555555555555555555555555555555555555555555555555555555555555"}}},
        "8.6": {"version": "8.6.0", "ts-vs17-x64": {"zip": {"path": "p.zip", "sha256": "66"}}}
    }"#;

    #[test]
    fn picks_the_x64_nts_build_of_each_version_in_order() {
        let releases = parse(SAMPLE).unwrap();
        let versions: Vec<_> = releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(
            versions,
            ["7.4.33", "8.3.36", "8.4.27"],
            "8.6 has no NTS x64 build"
        );
        let php84 = &releases[2];
        assert_eq!(php84.build, "nts-vs17-x64");
        assert_eq!(php84.archive, "php-8.4.27-nts-Win32-vs17-x64.zip");
        assert_eq!(
            php84.sha256.hex(),
            "93bd8a815a52156606fb906d0573437908c61f96e2cc52c03a51e36a41bf5236"
        );
        assert_eq!(
            php84.url(),
            "https://downloads.php.net/~windows/releases/php-8.4.27-nts-Win32-vs17-x64.zip"
        );
        assert_eq!(releases[0].build, "nts-vc15-x64");
    }

    #[test]
    fn resolves_minor_and_latest_patch_but_not_older_patches() {
        let releases = parse(SAMPLE).unwrap();
        assert_eq!(resolve(&releases, "8.4").unwrap().version, "8.4.27");
        assert_eq!(resolve(&releases, " 8.4.27 ").unwrap().version, "8.4.27");
        assert!(matches!(
            resolve(&releases, "8.4.26"),
            Err(IndexError::NotVerifiable { latest, .. }) if latest == "8.4.27"
        ));
        assert!(matches!(
            resolve(&releases, "8.2"),
            Err(IndexError::Unknown { available, .. }) if available == "7.4, 8.3, 8.4"
        ));
        for bad in ["8", "eight", "8.4.27.1", ""] {
            assert!(
                matches!(resolve(&releases, bad), Err(IndexError::BadSpec(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_index_pointing_outside_its_folder_is_refused() {
        let evil = SAMPLE.replace(
            "php-8.4.27-nts-Win32-vs17-x64.zip",
            "../../evil/php-8.4.27-nts-Win32-vs17-x64.zip",
        );
        assert!(matches!(parse(&evil), Err(IndexError::Shape(_))));
        let bad_hash = SAMPLE.replace("93BD8A815A", "not-a-hash");
        assert!(matches!(parse(&bad_hash), Err(IndexError::Shape(_))));
        assert!(matches!(parse("<html>"), Err(IndexError::NotJson(_))));
    }
}
