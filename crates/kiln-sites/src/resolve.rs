//! The sites Kiln serves: links plus the subfolders of parked folders ([ADR 0015] §5–8).
//!
//! [ADR 0015]: ../../../docs/adr/0015-parked-folders-and-drivers.md

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kiln_core::{HostName, Tld};

use crate::driver::{Driver, detect};
use crate::manifest::Manifest;

/// A linked site, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub host: String,
    pub project: PathBuf,
    pub secure: bool,
}

/// Where a site comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Link,
    /// A subfolder of this parked folder.
    Parked(PathBuf),
}

/// A site ready to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub host: HostName,
    pub project: PathBuf,
    pub driver: Driver,
    pub document_root: PathBuf,
    /// From Kiln's state, or `secure = true` in the project's `kiln.toml`.
    pub secure: bool,
    pub source: Source,
    /// The project's `kiln.toml`, if it has a usable one (ADR 0027).
    pub manifest: Option<Manifest>,
    /// For a name from `kiln.toml`'s `aliases`: the site it is another name for.
    pub alias_of: Option<HostName>,
}

impl Site {
    /// The site whose settings (such as a `kiln isolate` pin) this one follows: itself, or
    /// the main site for an alias.
    #[must_use]
    pub fn main_host(&self) -> &HostName {
        self.alias_of.as_ref().unwrap_or(&self.host)
    }
}

/// A parked subfolder that isn't served, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub folder: PathBuf,
    pub reason: String,
}

/// Every site, sorted by host, plus parked subfolders that were left out and problems with
/// projects' `kiln.toml` files (shown to the user; the sites are still served).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub sites: Vec<Site>,
    pub skipped: Vec<Skipped>,
    pub warnings: Vec<String>,
}

fn site(host: HostName, project: PathBuf, secure: bool, source: Source) -> Site {
    let driver = detect(&project);
    Site {
        document_root: driver.document_root(&project),
        host,
        project,
        driver,
        secure,
        source,
        manifest: None,
        alias_of: None,
    }
}

/// Reads each site's `kiln.toml` (ADR 0027 §2–4): `secure = true` turns HTTPS on, and each
/// alias becomes another site for the same folder unless that name is taken.
fn apply_manifests(by_host: &mut BTreeMap<String, Site>, tld: &Tld, warnings: &mut Vec<String>) {
    let mut aliases = Vec::new();
    for site in by_host.values_mut() {
        let manifest = match crate::manifest::read(&site.project) {
            Ok(Some(manifest)) => manifest,
            Ok(None) => continue,
            Err(e) => {
                warnings.push(format!("{}: {e} (the file is ignored)", site.host));
                continue;
            }
        };
        for warning in &manifest.warnings {
            warnings.push(format!(
                "{} ({}): {warning}",
                site.host,
                crate::manifest::FILE
            ));
        }
        if manifest.secure == Some(true) {
            site.secure = true;
        }
        for alias in &manifest.aliases {
            let name = alias.to_ascii_lowercase();
            let fqdn = if name.ends_with(&format!(".{tld}")) {
                name
            } else {
                format!("{name}.{tld}")
            };
            match HostName::new(&fqdn, tld) {
                Ok(host) => aliases.push((host, site.host.clone())),
                Err(e) => warnings.push(format!("{}: alias {alias:?} skipped: {e}", site.host)),
            }
        }
        site.manifest = Some(manifest);
    }
    for (host, main) in aliases {
        if by_host.contains_key(host.as_str()) {
            warnings.push(format!(
                "{main}: alias {host} skipped: that name is already a site"
            ));
            continue;
        }
        let mut alias = by_host[main.as_str()].clone();
        alias.host = host.clone();
        alias.alias_of = Some(main);
        by_host.insert(host.to_string(), alias);
    }
}

/// Merges `links` with the subfolders of `parked` (in the order they were parked). A link
/// wins over a parked folder of the same name; an earlier parked folder wins over a later one.
#[must_use]
pub fn resolve(links: &[Link], parked: &[PathBuf], tld: &Tld) -> Resolved {
    let mut by_host: BTreeMap<String, Site> = BTreeMap::new();
    let mut skipped = Vec::new();

    for link in links {
        match HostName::new(&link.host, tld) {
            Ok(host) => {
                by_host.insert(
                    host.to_string(),
                    site(host, link.project.clone(), link.secure, Source::Link),
                );
            }
            Err(e) => skipped.push(Skipped {
                folder: link.project.clone(),
                reason: format!("linked as {:?}, which is no longer valid: {e}", link.host),
            }),
        }
    }

    for dir in parked {
        let mut folders = match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect::<Vec<_>>(),
            Err(e) => {
                skipped.push(Skipped {
                    folder: dir.clone(),
                    reason: format!("parked folder can't be read: {e}"),
                });
                continue;
            }
        };
        folders.sort();
        for folder in folders {
            let Some(name) = folder
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_owned)
            else {
                skipped.push(Skipped {
                    folder,
                    reason: "its name isn't valid Unicode".into(),
                });
                continue;
            };
            if name.starts_with('.') {
                continue; // .git, .idea, …: never sites, not worth reporting
            }
            let host = match HostName::new(&format!("{}.{tld}", name.to_ascii_lowercase()), tld) {
                Ok(host) => host,
                Err(e) => {
                    skipped.push(Skipped {
                        folder,
                        reason: format!("{name:?} can't be a site name: {e}"),
                    });
                    continue;
                }
            };
            if let Some(existing) = by_host.get(host.as_str()) {
                let winner = match &existing.source {
                    Source::Link => "a link".to_owned(),
                    Source::Parked(other) => format!("{} (parked earlier)", other.display()),
                };
                if existing.project != folder {
                    skipped.push(Skipped {
                        folder,
                        reason: format!("{host} is already served from {winner}"),
                    });
                }
                continue;
            }
            by_host.insert(
                host.to_string(),
                site(host, folder, false, Source::Parked(dir.clone())),
            );
        }
    }

    let mut warnings = Vec::new();
    apply_manifests(&mut by_host, tld, &mut warnings);
    Resolved {
        sites: by_host.into_values().collect(),
        skipped,
        warnings,
    }
}

/// The parked folder `project` belongs to, if any.
#[must_use]
pub fn parked_parent<'a>(project: &Path, parked: &'a [PathBuf]) -> Option<&'a PathBuf> {
    parked
        .iter()
        .find(|dir| project.parent() == Some(dir.as_path()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    fn hosts(resolved: &Resolved) -> Vec<&str> {
        resolved.sites.iter().map(|s| s.host.as_str()).collect()
    }

    #[test]
    fn every_parked_subfolder_is_a_site_with_its_driver() {
        let dir = tempfile::tempdir().unwrap();
        let sites = dir.path().join("Sites");
        touch(&sites.join("blog").join("artisan"));
        touch(&sites.join("blog").join("public").join("index.php"));
        touch(&sites.join("Shop").join("wp-config.php"));
        touch(&sites.join("docs").join("index.html"));
        touch(&sites.join(".git").join("HEAD"));
        touch(&sites.join("my_app").join("index.php"));
        touch(&sites.join("notes.txt"));

        let resolved = resolve(&[], std::slice::from_ref(&sites), &Tld::default());
        assert_eq!(hosts(&resolved), ["blog.test", "docs.test", "shop.test"]);
        let blog = &resolved.sites[0];
        assert_eq!(blog.driver, Driver::Laravel);
        assert_eq!(blog.document_root, sites.join("blog").join("public"));
        assert_eq!(blog.source, Source::Parked(sites.clone()));
        assert_eq!(resolved.sites[1].driver, Driver::Static);
        assert_eq!(resolved.sites[2].driver, Driver::WordPress);
        assert_eq!(resolved.skipped.len(), 1, "{:?}", resolved.skipped);
        assert!(
            resolved.skipped[0].folder.ends_with("my_app"),
            "underscore isn't a valid label"
        );
    }

    #[test]
    fn links_win_and_earlier_parked_folders_win() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, elsewhere) = (
            dir.path().join("a"),
            dir.path().join("b"),
            dir.path().join("x"),
        );
        for d in [&a, &b] {
            touch(&d.join("blog").join("index.php"));
            touch(&d.join("api").join("index.php"));
        }
        touch(&elsewhere.join("index.html"));
        let links = [Link {
            host: "api.test".into(),
            project: elsewhere.clone(),
            secure: true,
        }];
        let resolved = resolve(&links, &[a.clone(), b.clone()], &Tld::default());
        assert_eq!(hosts(&resolved), ["api.test", "blog.test"]);
        assert_eq!(resolved.sites[0].project, elsewhere);
        assert!(resolved.sites[0].secure);
        assert_eq!(resolved.sites[0].source, Source::Link);
        assert_eq!(
            resolved.sites[1].project,
            a.join("blog"),
            "parked first wins"
        );
        assert_eq!(resolved.skipped.len(), 3, "{:?}", resolved.skipped);
    }

    #[test]
    fn a_secured_parked_site_is_a_link_to_the_same_folder_and_not_reported() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("blog").join("index.php"));
        let links = [Link {
            host: "blog.test".into(),
            project: dir.path().join("blog"),
            secure: true,
        }];
        let resolved = resolve(&links, &[dir.path().to_path_buf()], &Tld::default());
        assert_eq!(resolved.sites.len(), 1);
        assert!(resolved.sites[0].secure);
        assert!(resolved.skipped.is_empty(), "{:?}", resolved.skipped);
        assert_eq!(
            parked_parent(&dir.path().join("blog"), &[dir.path().to_path_buf()]),
            Some(&dir.path().to_path_buf())
        );
    }

    #[test]
    fn an_unreadable_parked_folder_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let resolved = resolve(&[], &[dir.path().join("gone")], &Tld::default());
        assert_eq!(resolved.sites, Vec::new());
        assert_eq!(resolved.skipped.len(), 1);
    }

    #[test]
    fn kiln_toml_turns_https_on_and_adds_aliases_for_the_same_folder() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("shop").join("public").join("index.php"));
        touch(&dir.path().join("shop").join("artisan"));
        touch(&dir.path().join("blog").join("index.php"));
        fs::write(
            dir.path().join("shop").join("kiln.toml"),
            "[site]\nsecure = true\naliases = [\"admin\", \"api.test\", \"blog\", \"bad name\"]\n[php]\nversion = \"8.3\"\n",
        )
        .unwrap();
        let resolved = resolve(&[], &[dir.path().to_path_buf()], &Tld::default());
        let hosts: Vec<&str> = resolved.sites.iter().map(|s| s.host.as_str()).collect();
        assert_eq!(hosts, ["admin.test", "api.test", "blog.test", "shop.test"]);
        let shop = resolved
            .sites
            .iter()
            .find(|s| s.host.as_str() == "shop.test")
            .unwrap();
        assert!(shop.secure, "secure = true");
        assert_eq!(shop.manifest.as_ref().unwrap().php.as_deref(), Some("8.3"));
        let admin = &resolved.sites[0];
        assert_eq!(
            admin.alias_of.as_ref().map(HostName::as_str),
            Some("shop.test")
        );
        assert_eq!(admin.main_host().as_str(), "shop.test");
        assert_eq!(
            admin.document_root, shop.document_root,
            "same folder, same driver"
        );
        assert!(admin.secure);
        let blog = resolved
            .sites
            .iter()
            .find(|s| s.host.as_str() == "blog.test")
            .unwrap();
        assert_eq!(
            blog.alias_of, None,
            "an alias never takes another site's name"
        );
        assert_eq!(resolved.warnings.len(), 2, "{:?}", resolved.warnings);
    }

    #[test]
    fn a_broken_kiln_toml_is_reported_and_the_site_still_served() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("shop").join("index.php"));
        fs::write(
            dir.path().join("shop").join("kiln.toml"),
            "[php]\nversion = 8.3\n",
        )
        .unwrap();
        let resolved = resolve(&[], &[dir.path().to_path_buf()], &Tld::default());
        assert_eq!(resolved.sites.len(), 1);
        assert_eq!(resolved.sites[0].manifest, None);
        assert!(
            resolved.warnings[0].contains("must be quoted"),
            "{:?}",
            resolved.warnings
        );
    }
}
