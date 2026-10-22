//! Framework drivers: which folder of a project is its document root ([ADR 0015] §1–4).
//!
//! The rules come from each framework's documented project layout; no code from other tools
//! is used.
//!
//! [ADR 0015]: ../../../docs/adr/0015-parked-folders-and-drivers.md

use std::path::{Path, PathBuf};

/// A detected project type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    Laravel,
    Symfony,
    Drupal,
    Craft,
    WordPressBedrock,
    WordPress,
    PhpPublic,
    PhpWeb,
    Php,
    Static,
}

impl Driver {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Laravel => "laravel",
            Self::Symfony => "symfony",
            Self::Drupal => "drupal",
            Self::Craft => "craft",
            Self::WordPressBedrock => "wordpress-bedrock",
            Self::WordPress => "wordpress",
            Self::PhpPublic => "php-public",
            Self::PhpWeb => "php-web",
            Self::Php => "php",
            Self::Static => "static",
        }
    }

    /// Whether the site needs PHP (everything but [`Driver::Static`]).
    #[must_use]
    pub fn uses_php(self) -> bool {
        self != Self::Static
    }

    /// The document root inside `project`.
    #[must_use]
    pub fn document_root(self, project: &Path) -> PathBuf {
        match self {
            Self::Laravel | Self::Symfony | Self::PhpPublic => project.join("public"),
            Self::Drupal | Self::Craft | Self::WordPressBedrock | Self::PhpWeb => {
                project.join("web")
            }
            Self::WordPress | Self::Php | Self::Static => project.to_path_buf(),
        }
    }
}

/// Each rule: the driver, and the files that must all exist (relative to the project).
const RULES: &[(Driver, &[&str])] = &[
    (Driver::Laravel, &["artisan", "public/index.php"]),
    (Driver::Symfony, &["bin/console", "public/index.php"]),
    (Driver::Drupal, &["web/core/lib/Drupal.php"]),
    (Driver::Craft, &["craft", "web/index.php"]),
    (Driver::WordPressBedrock, &["web/wp-config.php"]),
    (Driver::WordPress, &["wp-config.php"]),
    (Driver::WordPress, &["wp-load.php"]),
    (Driver::PhpPublic, &["public/index.php"]),
    (Driver::PhpWeb, &["web/index.php"]),
    (Driver::Php, &["index.php"]),
];

/// The first rule whose files all exist in `project`, else [`Driver::Static`].
#[must_use]
pub fn detect(project: &Path) -> Driver {
    RULES
        .iter()
        .find(|(_, files)| files.iter().all(|f| project.join(f).is_file()))
        .map_or(Driver::Static, |(driver, _)| *driver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        dir
    }

    #[test]
    fn detects_each_framework_and_its_document_root() {
        let cases: &[(&[&str], Driver, &str)] = &[
            (
                &["artisan", "public/index.php", "composer.json"],
                Driver::Laravel,
                "public",
            ),
            (
                &["bin/console", "public/index.php"],
                Driver::Symfony,
                "public",
            ),
            (
                &["web/core/lib/Drupal.php", "web/index.php"],
                Driver::Drupal,
                "web",
            ),
            (&["craft", "web/index.php"], Driver::Craft, "web"),
            (
                &["web/wp-config.php", "web/index.php"],
                Driver::WordPressBedrock,
                "web",
            ),
            (&["wp-config.php", "index.php"], Driver::WordPress, ""),
            (&["wp-load.php"], Driver::WordPress, ""),
            (&["public/index.php"], Driver::PhpPublic, "public"),
            (&["web/index.php"], Driver::PhpWeb, "web"),
            (&["index.php"], Driver::Php, ""),
            (&["index.html", "app.js"], Driver::Static, ""),
            (&[], Driver::Static, ""),
        ];
        for (files, driver, root) in cases {
            let dir = project(files);
            assert_eq!(detect(dir.path()), *driver, "{files:?}");
            let expected = if root.is_empty() {
                dir.path().to_path_buf()
            } else {
                dir.path().join(root)
            };
            assert_eq!(driver.document_root(dir.path()), expected);
        }
    }

    #[test]
    fn laravels_public_folder_linked_directly_is_plain_php() {
        let dir = project(&["artisan", "public/index.php"]);
        assert_eq!(detect(&dir.path().join("public")), Driver::Php);
    }

    #[test]
    fn a_directory_named_like_a_marker_file_does_not_count() {
        let dir = project(&["artisan/readme.txt", "public/index.php"]);
        assert_eq!(detect(dir.path()), Driver::PhpPublic, "artisan is a folder");
    }

    #[test]
    fn only_static_sites_skip_php() {
        assert!(!Driver::Static.uses_php());
        assert!(Driver::WordPress.uses_php());
        assert_eq!(Driver::WordPressBedrock.name(), "wordpress-bedrock");
    }
}
