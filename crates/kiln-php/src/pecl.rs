//! PECL extensions for Kiln's PHP on Windows ([ADR 0067]): the builds Kiln pins, from PHP's own
//! Windows downloads.
//!
//! PHP's PECL Windows builds publish no checksums, so the sums below were computed on a fresh
//! GitHub runner (the "Pin downloads" workflow) on 2026-11-14, and CI downloads the files
//! again and checks them.
//!
//! [ADR 0067]: ../../../docs/adr/0067-pecl-extensions.md

/// Where PHP keeps its PECL Windows builds.
pub const BASE_URL: &str = "https://downloads.php.net/~windows/pecl/releases/";

/// One pinned build of an extension, for one PHP minor and Windows build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build {
    /// e.g. `8.4`.
    pub php: &'static str,
    /// The PHP build it loads into, as windows.php.net names it: `nts-vs17-x64`.
    pub php_build: &'static str,
    pub archive: &'static str,
    pub sha256: &'static str,
}

/// An extension Kiln can add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Package {
    /// As `php -m` names it: `redis`.
    pub name: &'static str,
    /// What it is, for people.
    pub what: &'static str,
    pub version: &'static str,
    pub license: &'static str,
    pub builds: &'static [Build],
}

pub const PACKAGES: &[Package] = &[Package {
    name: "redis",
    what: "phpredis, a fast Redis client (REDIS_CLIENT=phpredis)",
    version: "6.3.0",
    license: "PHP-3.01",
    builds: &[
        Build {
            php: "8.2",
            php_build: "nts-vs16-x64",
            archive: "php_redis-6.3.0-8.2-nts-vs16-x64.zip",
            sha256: "b2b730b99b97352212b338c01f7dde577857e31c5f8e1d011ec2871e63f8f87c",
        },
        Build {
            php: "8.3",
            php_build: "nts-vs16-x64",
            archive: "php_redis-6.3.0-8.3-nts-vs16-x64.zip",
            sha256: "519fc1bdf54323d3ab08443c66f77e783d314c13b532e27cc8b390def7f81b60",
        },
        Build {
            php: "8.4",
            php_build: "nts-vs17-x64",
            archive: "php_redis-6.3.0-8.4-nts-vs17-x64.zip",
            sha256: "6db881ed172703962002d7e02dacc8ddb3f789904c648fee3e6ceec1319679cd",
        },
        Build {
            php: "8.5",
            php_build: "nts-vs17-x64",
            archive: "php_redis-6.3.0-8.5-nts-vs17-x64.zip",
            sha256: "481d6d1af45060ab41af6abe250faa270276fc47a493badc2e178024cdf6e255",
        },
    ],
}];

impl Package {
    /// The build for PHP `version` installed from `archive`
    /// (`php-8.4.27-nts-Win32-vs17-x64.zip`), or `None` when none is pinned for it.
    #[must_use]
    pub fn build_for(&self, version: &str, archive: &str) -> Option<&'static Build> {
        self.builds
            .iter()
            .find(|b| fits(b.php, b.php_build, version, archive))
    }

    #[must_use]
    pub fn url(&self, build: &Build) -> String {
        format!("{BASE_URL}{}/{}/{}", self.name, self.version, build.archive)
    }

    /// The file in the archive PHP loads: `php_redis.dll`.
    #[must_use]
    pub fn dll(&self) -> String {
        format!("php_{}.dll", self.name)
    }
}

/// The package called `name`.
#[must_use]
pub fn package(name: &str) -> Option<&'static Package> {
    PACKAGES.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// Whether a build for PHP `minor` and `php_build` (`nts-vs17-x64`) loads into PHP `version`
/// installed from `archive`: the same minor, thread safety and compiler.
#[must_use]
pub fn fits(minor: &str, php_build: &str, version: &str, archive: &str) -> bool {
    let installed: String = version.split('.').take(2).collect::<Vec<_>>().join(".");
    let archive = archive.to_ascii_lowercase();
    let (threads, rest) = php_build.split_once('-').unwrap_or_default();
    let compiler = rest.split('-').next().unwrap_or_default();
    minor == installed
        && archive.contains(&format!("-{threads}-"))
        && archive.contains(&format!("-{compiler}-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_well_formed() {
        for p in PACKAGES {
            for b in p.builds {
                assert_eq!(b.sha256.len(), 64, "{b:?}");
                assert!(
                    b.sha256
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                );
                assert!(
                    b.archive.contains(p.version) && b.archive.contains(b.php),
                    "{b:?}"
                );
                assert!(
                    b.archive.contains(b.php_build.split('-').nth(1).unwrap()),
                    "{b:?}"
                );
            }
        }
    }

    #[test]
    fn the_build_matches_the_php_build() {
        let redis = package("redis").unwrap();
        let b = redis
            .build_for("8.4.27", "php-8.4.27-1-nts-Win32-vs17-x64.zip")
            .unwrap();
        assert_eq!(
            redis.url(b),
            "https://downloads.php.net/~windows/pecl/releases/redis/6.3.0/php_redis-6.3.0-8.4-nts-vs17-x64.zip"
        );
        assert_eq!(redis.dll(), "php_redis.dll");
        assert_eq!(
            redis.build_for("8.4.27", "php-8.4.27-Win32-vs17-x64.zip"),
            None
        );
        assert_eq!(
            redis.build_for("8.1.31", "php-8.1.31-nts-Win32-vs16-x64.zip"),
            None
        );
        assert!(package("Redis").is_some() && package("imagick").is_none());
    }
}
