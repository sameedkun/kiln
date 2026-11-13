//! Xdebug on demand ([ADR 0061]): the pinned Windows builds, and the `php.ini` a debug worker
//! loads.
//!
//! Xdebug is the Xdebug License 1.03 (based on the PHP License 3.01). Kiln doesn't ship it: it
//! downloads the build for a PHP when asked, checked against the pins below, which were
//! taken from xdebug.org's download page on 2026-11-13 and are checked again by CI.
//!
//! [ADR 0061]: ../../../docs/adr/0061-xdebug.md

/// The Xdebug release Kiln pins.
pub const VERSION: &str = "3.5.3";

/// Where xdebug.org keeps its files.
pub const BASE_URL: &str = "https://xdebug.org/files/";

/// The port Xdebug connects to: the one editors listen on by default (VS Code's PHP Debug,
/// `PhpStorm`).
pub const CLIENT_PORT: u16 = 9003;

/// One pinned build of Xdebug, for one PHP minor and Windows build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build {
    /// e.g. `8.4`.
    pub php: &'static str,
    /// The PHP build it loads into, as windows.php.net names it: `nts-vs17-x64`.
    pub php_build: &'static str,
    pub file: &'static str,
    pub sha256: &'static str,
}

/// Non-thread-safe x64 builds, as Kiln's PHP is.
pub const BUILDS: &[Build] = &[
    Build {
        php: "8.2",
        php_build: "nts-vs16-x64",
        file: "php_xdebug-3.5.3-8.2-nts-vs16-x86_64.dll",
        sha256: "b449eda7eeefc80c651fac334582c5f0877cd72cfe55fc1c5f0f98bf6a4edb94",
    },
    Build {
        php: "8.3",
        php_build: "nts-vs16-x64",
        file: "php_xdebug-3.5.3-8.3-nts-vs16-x86_64.dll",
        sha256: "4c80487592484c36d21a7006a53738dfc07884fc6867720ddc5c11559371cdce",
    },
    Build {
        php: "8.4",
        php_build: "nts-vs17-x64",
        file: "php_xdebug-3.5.3-8.4-nts-vs17-x86_64.dll",
        sha256: "a94d4fcfe4086a300b2decaf7c04e59f4981aa5e56992d48b55c5e49dfd9b9ee",
    },
    Build {
        php: "8.5",
        php_build: "nts-vs17-x64",
        file: "php_xdebug-3.5.3-8.5-nts-vs17-x86_64.dll",
        sha256: "0a0d52bf276dad7e5f90ac30a5110b3119d291b20c118be0cb7f5752064f1811",
    },
];

impl Build {
    #[must_use]
    pub fn url(&self) -> String {
        format!("{BASE_URL}{}", self.file)
    }
}

/// The build for PHP `version` installed from `archive` (`php-8.4.27-nts-Win32-vs17-x64.zip`),
/// or `None` when Xdebug has none Kiln pins for it.
#[must_use]
pub fn build_for(version: &str, archive: &str) -> Option<&'static Build> {
    let minor: String = version.split('.').take(2).collect::<Vec<_>>().join(".");
    let archive = archive.to_ascii_lowercase();
    BUILDS.iter().find(|b| {
        let (threads, rest) = b.php_build.split_once('-').unwrap_or_default();
        let compiler = rest.split('-').next().unwrap_or_default();
        b.php == minor
            && archive.contains(&format!("-{threads}-"))
            && archive.contains(&format!("-{compiler}-"))
    })
}

/// The `php.ini` of a debug worker: the PHP's own, followed by Xdebug loaded from `dll`, on
/// demand only (a trigger in the request), connecting to an editor on this computer. JIT
/// is off: Xdebug and the JIT don't run together.
#[must_use]
pub fn debug_ini(base: &str, dll: &std::path::Path) -> String {
    use std::fmt::Write;
    let mut text = base.trim_end().to_owned();
    let _ = write!(
        text,
        "\n\n; Kiln's debug worker (ADR 0061): written by Kiln on each start, not to be edited.\n\
         [xdebug]\n\
         zend_extension=\"{}\"\n\
         xdebug.mode=debug\n\
         xdebug.start_with_request=trigger\n\
         xdebug.client_host=127.0.0.1\n\
         xdebug.client_port={CLIENT_PORT}\n\
         opcache.jit=off\n",
        dll.display()
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_well_formed() {
        for b in BUILDS {
            assert_eq!(b.sha256.len(), 64, "{b:?}");
            assert!(
                b.sha256
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            );
            assert!(b.file.contains(VERSION) && b.file.contains(b.php), "{b:?}");
            assert!(b.file.starts_with("php_xdebug-") && b.file.contains("x86_64."));
        }
    }

    #[test]
    fn the_build_matches_the_php_build() {
        let b = build_for("8.4.27", "php-8.4.27-nts-Win32-vs17-x64.zip").unwrap();
        assert_eq!(b.file, "php_xdebug-3.5.3-8.4-nts-vs17-x86_64.dll");
        assert_eq!(
            b.url(),
            "https://xdebug.org/files/php_xdebug-3.5.3-8.4-nts-vs17-x86_64.dll"
        );
        assert_eq!(
            build_for("8.3.35", "php-8.3.35-nts-Win32-vs16-x64.zip").map(|b| b.php),
            Some("8.3")
        );
        // A thread-safe PHP, an unpinned version, or another compiler: none.
        assert_eq!(build_for("8.4.27", "php-8.4.27-Win32-vs17-x64.zip"), None);
        assert_eq!(
            build_for("8.1.31", "php-8.1.31-nts-Win32-vs16-x64.zip"),
            None
        );
        assert_eq!(
            build_for("8.4.27", "php-8.4.27-nts-Win32-vs16-x64.zip"),
            None
        );
    }

    #[test]
    fn the_debug_ini_is_the_php_ini_then_xdebug_on_trigger() {
        let ini = debug_ini("memory_limit=512M\n", std::path::Path::new("C:/k/x.dll"));
        assert!(ini.starts_with("memory_limit=512M\n"));
        assert!(ini.contains("zend_extension=\"C:/k/x.dll\""));
        assert!(ini.contains("xdebug.mode=debug\n"));
        assert!(ini.contains("xdebug.start_with_request=trigger\n"));
        assert!(ini.contains("xdebug.client_host=127.0.0.1\n"));
        assert!(ini.contains("opcache.jit=off\n"));
    }
}
