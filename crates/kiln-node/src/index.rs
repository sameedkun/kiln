//! nodejs.org's release list ([ADR 0028] §1): `dist/index.json`, and which archive Kiln uses
//! on this platform.
//!
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use crate::spec::{Spec, Version, parse_version};

pub const DIST: &str = "https://nodejs.org/dist/";
pub const INDEX_URL: &str = "https://nodejs.org/dist/index.json";
/// The index is about 400 KB today.
pub const INDEX_LIMIT: u64 = 8 * 1024 * 1024;
/// Older lines are past end of life, and their archives are laid out differently.
pub const OLDEST_MAJOR: u32 = 18;

/// A build for one operating system and CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    /// The name in `index.json`'s `files`.
    pub index_file: &'static str,
    /// The part of the archive name after the version, e.g. `win-x64`.
    pub suffix: &'static str,
    pub extension: &'static str,
}

pub const WIN_X64: Platform = Platform {
    index_file: "win-x64-zip",
    suffix: "win-x64",
    extension: "zip",
};
pub const LINUX_X64: Platform = Platform {
    index_file: "linux-x64",
    suffix: "linux-x64",
    extension: "tar.gz",
};
pub const LINUX_ARM64: Platform = Platform {
    index_file: "linux-arm64",
    suffix: "linux-arm64",
    extension: "tar.gz",
};

impl Platform {
    /// This machine's, if Kiln supports it.
    #[must_use]
    pub fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Some(WIN_X64),
            ("linux", "x86_64") => Some(LINUX_X64),
            ("linux", "aarch64") => Some(LINUX_ARM64),
            _ => None,
        }
    }

    /// The archive's top folder: `node-v22.11.0-win-x64`.
    #[must_use]
    pub fn top_folder(&self, version: &str) -> String {
        format!("node-v{version}-{}", self.suffix)
    }

    #[must_use]
    pub fn archive(&self, version: &str) -> String {
        format!("{}.{}", self.top_folder(version), self.extension)
    }
}

/// One release in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRelease {
    /// Without the `v`: `22.11.0`.
    pub version: String,
    pub numbers: Version,
    /// The LTS codename in lower case (`jod`), for LTS releases.
    pub lts: Option<String>,
    pub date: String,
    /// The `files` list: which platforms were built.
    pub files: Vec<String>,
}

impl NodeRelease {
    #[must_use]
    pub fn has(&self, platform: Platform) -> bool {
        self.files.iter().any(|f| f == platform.index_file)
    }

    /// The release's folder on nodejs.org.
    #[must_use]
    pub fn folder_url(&self) -> String {
        format!("{DIST}v{}/", self.version)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IndexError {
    #[error("unexpected nodejs.org release index: {0}")]
    Malformed(String),
    #[error("no Node release matches {spec}{hint}")]
    NoMatch { spec: String, hint: String },
}

/// The releases in `index.json`, from Node [`OLDEST_MAJOR`] on.
pub fn parse(json: &str) -> Result<Vec<NodeRelease>, IndexError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| IndexError::Malformed(e.to_string()))?;
    let list = value
        .as_array()
        .ok_or_else(|| IndexError::Malformed("not a list".into()))?;
    let mut releases = Vec::new();
    for item in list {
        let Some(numbers) = item["version"].as_str().and_then(parse_version) else {
            return Err(IndexError::Malformed(format!("bad version in {item}")));
        };
        if numbers[0] < OLDEST_MAJOR {
            continue;
        }
        releases.push(NodeRelease {
            version: format!("{}.{}.{}", numbers[0], numbers[1], numbers[2]),
            numbers,
            // `false` for releases that aren't LTS, else the codename.
            lts: item["lts"].as_str().map(str::to_ascii_lowercase),
            date: item["date"].as_str().unwrap_or("").to_owned(),
            files: item["files"]
                .as_array()
                .map(|files| {
                    files
                        .iter()
                        .filter_map(|f| f.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    if releases.is_empty() {
        return Err(IndexError::Malformed("no releases".into()));
    }
    Ok(releases)
}

/// The newest release matching `spec` that was built for `platform`.
pub fn resolve<'a>(
    releases: &'a [NodeRelease],
    spec: &Spec,
    platform: Platform,
) -> Result<&'a NodeRelease, IndexError> {
    let built: Vec<&NodeRelease> = releases.iter().filter(|r| r.has(platform)).collect();
    let found = spec.newest(&built, |r| (r.numbers, r.lts.as_deref()));
    found.copied().ok_or_else(|| IndexError::NoMatch {
        spec: spec.to_string(),
        hint: match spec {
            Spec::Version(n) if n[0] < OLDEST_MAJOR => {
                format!(" (Kiln installs Node {OLDEST_MAJOR} and later)")
            }
            _ => " (see `kiln node available`)".to_owned(),
        },
    })
}

/// The newest release of each major line, newest line first: what `kiln node available`
/// shows.
#[must_use]
pub fn newest_per_line(releases: &[NodeRelease], platform: Platform) -> Vec<&NodeRelease> {
    let mut lines: Vec<&NodeRelease> = Vec::new();
    for release in releases.iter().filter(|r| r.has(platform)) {
        match lines
            .iter_mut()
            .find(|r| r.numbers[0] == release.numbers[0])
        {
            Some(newest) if newest.numbers < release.numbers => *newest = release,
            Some(_) => {}
            None => lines.push(release),
        }
    }
    lines.sort_by_key(|r| std::cmp::Reverse(r.numbers));
    lines
}

/// The SHA-256 for `archive` in a `SHASUMS256.txt`.
#[must_use]
pub fn checksum_for<'a>(shasums: &'a str, archive: &str) -> Option<&'a str> {
    shasums.lines().find_map(|line| {
        let (hash, name) = line.split_once("  ")?;
        (name.trim_end() == archive).then_some(hash)
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const SAMPLE: &str = r#"[
{"version":"v24.1.0","date":"2026-10-21","files":["linux-x64","win-x64-zip"],"lts":false},
{"version":"v22.23.4","date":"2026-10-21","files":["linux-arm64","linux-x64","win-x64-zip"],"lts":"Jod"},
{"version":"v22.23.3","date":"2026-10-01","files":["linux-arm64","linux-x64","win-x64-zip"],"lts":"Jod"},
{"version":"v20.20.2","date":"2026-09-01","files":["linux-x64","win-x64-zip"],"lts":"Iron"},
{"version":"v16.20.2","date":"2023-08-08","files":["linux-x64","win-x64-zip"],"lts":"Gallium"}
]"#;

    #[test]
    fn reads_the_index_and_drops_old_lines() {
        let releases = parse(SAMPLE).unwrap();
        assert_eq!(releases.len(), 4, "Node 16 is left out");
        assert_eq!(releases[1].version, "22.23.4");
        assert_eq!(releases[1].lts.as_deref(), Some("jod"));
        assert_eq!(releases[0].lts, None);
        assert!(parse("{}").is_err());
        assert!(parse(r#"[{"version":"x"}]"#).is_err());
    }

    #[test]
    fn resolves_for_a_platform() {
        let releases = parse(SAMPLE).unwrap();
        let resolve = |spec: &str, platform| {
            resolve(&releases, &Spec::parse(spec).unwrap(), platform).map(|r| r.version.clone())
        };
        assert_eq!(resolve("lts", WIN_X64).unwrap(), "22.23.4");
        assert_eq!(resolve("22.23.3", LINUX_X64).unwrap(), "22.23.3");
        assert_eq!(
            resolve("latest", LINUX_ARM64).unwrap(),
            "22.23.4",
            "24 has no arm64 build"
        );
        assert_eq!(resolve("lts/iron", WIN_X64).unwrap(), "20.20.2");
        let err = resolve("16", WIN_X64).unwrap_err();
        assert!(err.to_string().contains("Node 18 and later"), "{err}");
        let lines: Vec<_> = newest_per_line(&releases, WIN_X64)
            .iter()
            .map(|r| r.version.as_str())
            .collect();
        assert_eq!(lines, ["24.1.0", "22.23.4", "20.20.2"]);
    }

    #[test]
    fn names_archives_and_finds_their_checksums() {
        assert_eq!(WIN_X64.archive("22.23.4"), "node-v22.23.4-win-x64.zip");
        assert_eq!(
            LINUX_X64.archive("22.23.4"),
            "node-v22.23.4-linux-x64.tar.gz"
        );
        let shasums = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures")
                .join("SHASUMS256-v22.23.4.txt"),
        )
        .unwrap();
        assert_eq!(
            checksum_for(&shasums, "node-v22.23.4-win-x64.zip"),
            Some("cbc74b5e0982c9dbb41525fd8fb5dcd32fbee237863f7829072f4639a1ce5f20")
        );
        assert_eq!(checksum_for(&shasums, "node-v22.23.4-win-x64"), None);
    }
}
