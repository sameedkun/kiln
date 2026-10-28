//! Which PHP runs what ([ADR 0016]): the default version, per-site pins, and the version for a
//! folder (what the `php` / `composer` shims and `kiln which` use).
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md

use std::collections::BTreeMap;
use std::path::Path;

use kiln_core::Tld;
use kiln_sites::{Link, Resolved, Site};
use kiln_state::{PhpRecord, StateError, StateStore};

pub mod shim;

/// The `php.default` setting: the version that serves sites without a pin.
pub const DEFAULT_KEY: &str = "php.default";

/// Every site: links plus parked subfolders, with drivers (ADR 0015).
pub fn sites(store: &StateStore) -> Result<Resolved, StateError> {
    let links: Vec<Link> = store
        .sites()?
        .into_iter()
        .map(|s| Link {
            host: s.host,
            project: s.root,
            secure: s.secure,
        })
        .collect();
    let parked = store.parked()?;
    Ok(kiln_sites::resolve(&links, &parked, &Tld::default()))
}

/// Installed versions that are complete on disk, oldest first.
pub fn installed(store: &StateStore) -> Result<Vec<PhpRecord>, StateError> {
    Ok(store
        .php_versions()?
        .into_iter()
        .filter(|r| kiln_php::is_installed(&r.dir))
        .collect())
}

fn numbers(version: &str) -> Option<Vec<u32>> {
    version.split('.').map(|p| p.parse().ok()).collect()
}

/// Whether `spec` is a minor (`8.3`) or exact (`8.3.36`) version.
#[must_use]
pub fn is_version_spec(spec: &str) -> bool {
    numbers(spec).is_some_and(|n| (2..=3).contains(&n.len()))
}

/// The installed build for `spec`: exact for `8.3.36`, the newest `8.3.x` for `8.3`.
#[must_use]
pub fn pick<'a>(installed: &'a [PhpRecord], spec: &str) -> Option<&'a PhpRecord> {
    let want = numbers(spec.trim())?;
    installed
        .iter()
        .filter(|r| {
            numbers(&r.version).is_some_and(|have| match want.len() {
                2 => have.len() >= 2 && have[..2] == want[..],
                3 => have == want,
                _ => false,
            })
        })
        .max_by_key(|r| numbers(&r.version))
}

/// The default PHP: `php.default` if installed, else the newest installed.
pub fn default_php(
    store: &StateStore,
    installed: &[PhpRecord],
) -> Result<Option<PhpRecord>, StateError> {
    let wanted = store.setting(DEFAULT_KEY)?;
    Ok(wanted
        .as_deref()
        .and_then(|spec| pick(installed, spec))
        .or_else(|| installed.last())
        .cloned())
}

/// The PHP a site runs, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePhp {
    /// `None`: no PHP is installed at all.
    pub php: Option<PhpRecord>,
    /// The version the site asks for, if any: its `kiln isolate` pin, else its `kiln.toml`.
    pub pinned: Option<String>,
    /// The version comes from the project's `kiln.toml` (ADR 0027), not a pin on this machine.
    pub from_project: bool,
    /// That version isn't installed (or was removed), so the default is used.
    pub fell_back: bool,
}

/// The PHP for each site host (ADR 0027 §3): a `kiln isolate` pin on this machine (an alias
/// follows its main site's), else the project's `kiln.toml`, else the default.
pub fn site_versions(
    store: &StateStore,
    sites: &[Site],
    installed: &[PhpRecord],
) -> Result<BTreeMap<String, SitePhp>, StateError> {
    let pins = store.isolations()?;
    let default = default_php(store, installed)?;
    Ok(sites
        .iter()
        .map(|site| {
            let pin = pins.get(site.main_host().as_str()).cloned();
            let from_project =
                pin.is_none() && site.manifest.as_ref().is_some_and(|m| m.php.is_some());
            let pinned = pin.or_else(|| site.manifest.as_ref().and_then(|m| m.php.clone()));
            let chosen = pinned.as_deref().and_then(|spec| pick(installed, spec));
            let fell_back = pinned.is_some() && chosen.is_none();
            let php = chosen.cloned().or_else(|| default.clone());
            (
                site.host.to_string(),
                SitePhp {
                    php,
                    pinned,
                    from_project,
                    fell_back,
                },
            )
        })
        .collect())
}

/// The site whose project folder contains `dir` (the deepest one, if nested; the main site
/// rather than one of its aliases).
#[must_use]
pub fn site_for_dir<'a>(sites: &'a [Site], dir: &Path) -> Option<&'a Site> {
    let dir = normalise(dir);
    sites
        .iter()
        .filter(|s| dir.starts_with(normalise(&s.project)))
        .max_by_key(|s| (s.project.components().count(), s.alias_of.is_none()))
}

/// Case-insensitive on Windows, as the file system is.
fn normalise(path: &Path) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    if cfg!(windows) {
        std::path::PathBuf::from(text.to_lowercase())
    } else {
        std::path::PathBuf::from(text)
    }
}

/// What `php` would run in `dir`, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPhp {
    /// The site `dir` belongs to, if any.
    pub site: Option<String>,
    pub php: SitePhp,
}

/// The PHP for a folder: its site's version, else the default (ADR 0016 §9).
pub fn php_for_dir(store: &StateStore, dir: &Path) -> Result<DirPhp, StateError> {
    let resolved = sites(store)?;
    let installed = installed(store)?;
    match site_for_dir(&resolved.sites, dir) {
        Some(site) => {
            let mut versions = site_versions(store, std::slice::from_ref(site), &installed)?;
            Ok(DirPhp {
                site: Some(site.host.to_string()),
                php: versions.remove(site.host.as_str()).unwrap_or(SitePhp {
                    php: None,
                    pinned: None,
                    from_project: false,
                    fell_back: false,
                }),
            })
        }
        None => Ok(DirPhp {
            site: None,
            php: SitePhp {
                php: default_php(store, &installed)?,
                pinned: None,
                from_project: false,
                fell_back: false,
            },
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn record(version: &str) -> PhpRecord {
        PhpRecord {
            version: version.into(),
            dir: PathBuf::from(format!("/php/{version}")),
            archive: String::new(),
            sha256: String::new(),
        }
    }

    #[test]
    fn minor_specs_pick_the_newest_patch_and_exact_specs_match_exactly() {
        let installed = [
            record("8.1.34"),
            record("8.3.35"),
            record("8.3.36"),
            record("8.4.27"),
        ];
        assert_eq!(pick(&installed, "8.3").unwrap().version, "8.3.36");
        assert_eq!(pick(&installed, "8.3.35").unwrap().version, "8.3.35");
        assert_eq!(pick(&installed, " 8.4 ").unwrap().version, "8.4.27");
        assert!(pick(&installed, "8.2").is_none());
        assert!(pick(&installed, "8").is_none());
        assert!(pick(&installed, "8.3.99").is_none());
        assert!(is_version_spec("8.3") && is_version_spec("8.3.36"));
        assert!(!is_version_spec("8") && !is_version_spec("latest"));
    }

    #[test]
    fn the_deepest_project_containing_a_folder_is_its_site() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("Sites").join("app");
        let inner = outer.join("packages").join("admin");
        for p in [&outer, &inner] {
            std::fs::create_dir_all(p).unwrap();
            std::fs::write(p.join("index.php"), "").unwrap();
        }
        let links = [
            Link {
                host: "app.test".into(),
                project: outer.clone(),
                secure: false,
            },
            Link {
                host: "admin.test".into(),
                project: inner.clone(),
                secure: false,
            },
        ];
        let resolved = kiln_sites::resolve(&links, &[], &Tld::default());
        let host = |p: &Path| site_for_dir(&resolved.sites, p).map(|s| s.host.to_string());
        assert_eq!(host(&outer.join("routes")).as_deref(), Some("app.test"));
        assert_eq!(host(&inner.join("src")).as_deref(), Some("admin.test"));
        assert_eq!(host(dir.path()), None);
    }

    #[test]
    fn pins_win_and_a_missing_pinned_version_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = StateStore::open_in_memory().unwrap();
        // "Installed" builds: folders with this platform's binaries.
        for version in ["8.1.34", "8.4.27"] {
            let php = dir.path().join(version);
            std::fs::create_dir_all(&php).unwrap();
            std::fs::write(php.join(kiln_php::PHP_EXE), "").unwrap();
            std::fs::write(php.join(kiln_php::linux::PHP_FPM_BIN), "").unwrap();
            store
                .put_php(&PhpRecord {
                    version: version.into(),
                    dir: php,
                    archive: String::new(),
                    sha256: String::new(),
                })
                .unwrap();
        }
        for name in ["legacy", "modern", "gone"] {
            let project = dir.path().join("Sites").join(name);
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(project.join("index.php"), "").unwrap();
        }
        store.park(&dir.path().join("Sites")).unwrap();
        store.isolate("legacy.test", "8.1").unwrap();
        store.isolate("gone.test", "8.2").unwrap();

        let installed = installed(&store).unwrap();
        let resolved = sites(&store).unwrap();
        let versions = site_versions(&store, &resolved.sites, &installed).unwrap();
        let version = |host: &str| versions[host].php.as_ref().unwrap().version.clone();
        assert_eq!(version("legacy.test"), "8.1.34");
        assert_eq!(
            version("modern.test"),
            "8.4.27",
            "default = newest installed"
        );
        assert_eq!(version("gone.test"), "8.4.27");
        assert!(versions["gone.test"].fell_back);

        let here =
            php_for_dir(&store, &dir.path().join("Sites").join("legacy").join("app")).unwrap();
        assert_eq!(here.site.as_deref(), Some("legacy.test"));
        assert_eq!(here.php.php.unwrap().version, "8.1.34");
        let elsewhere = php_for_dir(&store, dir.path()).unwrap();
        assert_eq!(elsewhere.site, None);
        assert_eq!(elsewhere.php.php.unwrap().version, "8.4.27");

        store.set_setting(DEFAULT_KEY, "8.1.34").unwrap();
        assert_eq!(
            default_php(&store, &installed).unwrap().unwrap().version,
            "8.1.34"
        );
    }

    #[test]
    fn kiln_toml_sets_the_version_a_local_pin_beats_it_and_aliases_follow_their_site() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = StateStore::open_in_memory().unwrap();
        for version in ["8.1.34", "8.3.36", "8.4.27"] {
            let php = dir.path().join(version);
            std::fs::create_dir_all(&php).unwrap();
            std::fs::write(php.join(kiln_php::PHP_EXE), "").unwrap();
            std::fs::write(php.join(kiln_php::linux::PHP_FPM_BIN), "").unwrap();
            store
                .put_php(&PhpRecord {
                    version: version.into(),
                    dir: php,
                    archive: String::new(),
                    sha256: String::new(),
                })
                .unwrap();
        }
        let sites_dir = dir.path().join("Sites");
        for name in ["shop", "blog"] {
            std::fs::create_dir_all(sites_dir.join(name)).unwrap();
            std::fs::write(sites_dir.join(name).join("index.php"), "").unwrap();
            std::fs::write(
                sites_dir.join(name).join("kiln.toml"),
                "[site]\naliases = [\"admin-NAME\"]\n[php]\nversion = \"8.3\"\n"
                    .replace("NAME", name),
            )
            .unwrap();
        }
        store.park(&sites_dir).unwrap();
        store.isolate("blog.test", "8.1").unwrap();

        let installed = installed(&store).unwrap();
        let resolved = sites(&store).unwrap();
        let versions = site_versions(&store, &resolved.sites, &installed).unwrap();
        let version = |host: &str| versions[host].php.as_ref().unwrap().version.clone();
        assert_eq!(version("shop.test"), "8.3.36", "from kiln.toml");
        assert!(versions["shop.test"].from_project);
        assert_eq!(version("admin-shop.test"), "8.3.36", "the alias too");
        assert_eq!(version("blog.test"), "8.1.34", "the local pin wins");
        assert!(!versions["blog.test"].from_project);
        assert_eq!(
            version("admin-blog.test"),
            "8.1.34",
            "an alias follows its site's pin"
        );

        let here = php_for_dir(&store, &sites_dir.join("shop").join("app")).unwrap();
        assert_eq!(
            here.site.as_deref(),
            Some("shop.test"),
            "the site, not its alias"
        );
        assert_eq!(here.php.php.unwrap().version, "8.3.36");
    }
}
