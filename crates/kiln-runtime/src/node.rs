//! Which Node runs in a folder ([ADR 0028] §5): the nearest `kiln.toml`, `.nvmrc` or
//! `.node-version` up the tree, else the default.
//!
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use std::path::{Path, PathBuf};

use kiln_node::spec::from_version_file;
use kiln_node::{InstalledNode, Spec};

/// The `node.default` setting: the version used where no project asks for one.
pub const DEFAULT_KEY: &str = "node.default";
/// Where Kiln installs Node (under its local data).
pub const NODE_DIR: &str = "node";
/// The files that name a folder's Node version, in the order they are checked.
pub const VERSION_FILES: [&str; 2] = [".nvmrc", ".node-version"];

/// A version a project asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// The file that asks: a `kiln.toml`, `.nvmrc` or `.node-version`.
    pub file: PathBuf,
    /// As written.
    pub text: String,
    /// The spec, or why it can't be read.
    pub spec: Result<Spec, String>,
}

/// The version asked for in `dir` or the nearest folder above it that asks for one.
#[must_use]
pub fn wanted_for_dir(dir: &Path) -> Option<Wanted> {
    for folder in dir.ancestors() {
        // A kiln.toml that can't be read doesn't stop the search; `kiln doctor` reports it.
        if let Ok(Some(manifest)) = kiln_sites::manifest::read(folder)
            && let Some(version) = manifest.node
        {
            return Some(Wanted {
                file: folder.join(kiln_sites::manifest::FILE),
                spec: Spec::parse(&version),
                text: version,
            });
        }
        for name in VERSION_FILES {
            let file = folder.join(name);
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            if let Some(line) = from_version_file(&text) {
                return Some(Wanted {
                    spec: Spec::parse(line),
                    text: line.to_owned(),
                    file,
                });
            }
        }
    }
    None
}

fn info(node: &InstalledNode) -> ([u32; 3], Option<&str>) {
    (node.numbers, node.lts.as_deref())
}

/// The default Node: `node.default` if it matches an install, else the newest installed.
#[must_use]
pub fn default_node<'a>(
    installed: &'a [InstalledNode],
    default_spec: Option<&str>,
) -> Option<&'a InstalledNode> {
    default_spec
        .and_then(|spec| Spec::parse(spec).ok())
        .and_then(|spec| spec.newest(installed, info))
        .or_else(|| installed.last())
}

/// What `node` would run in a folder, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirNode {
    /// `None`: Kiln has no Node installed.
    pub node: Option<InstalledNode>,
    pub wanted: Option<Wanted>,
    /// A version was asked for, but none installed matches it (or it can't be read), so the
    /// default is used.
    pub fell_back: bool,
}

/// The Node for `dir`, among `installed` (oldest first).
#[must_use]
pub fn node_for_dir(
    installed: &[InstalledNode],
    default_spec: Option<&str>,
    dir: &Path,
) -> DirNode {
    let wanted = wanted_for_dir(dir);
    let chosen = wanted
        .as_ref()
        .and_then(|w| w.spec.as_ref().ok())
        .and_then(|spec| spec.newest(installed, info));
    DirNode {
        fell_back: wanted.is_some() && chosen.is_none(),
        node: chosen
            .or_else(|| default_node(installed, default_spec))
            .cloned(),
        wanted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(version: &str, lts: Option<&str>) -> InstalledNode {
        InstalledNode {
            version: version.into(),
            numbers: kiln_node::spec::parse_version(version).unwrap(),
            lts: lts.map(str::to_owned),
            dir: PathBuf::from(format!("/node/{version}")),
        }
    }

    #[test]
    fn the_nearest_file_wins_and_kiln_toml_beats_nvmrc() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("app");
        let deep = project.join("resources").join("js");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(wanted_for_dir(&deep).map(|w| w.text), None);

        std::fs::write(dir.path().join(".node-version"), "20\n").unwrap();
        std::fs::write(project.join(".nvmrc"), "# team\nlts/jod\n").unwrap();
        let wanted = wanted_for_dir(&deep).unwrap();
        assert_eq!(wanted.text, "lts/jod", "the nearest folder first");
        assert_eq!(wanted.file, project.join(".nvmrc"));

        std::fs::write(project.join("kiln.toml"), "[node]\nversion = \"22\"\n").unwrap();
        assert_eq!(wanted_for_dir(&deep).unwrap().text, "22", "kiln.toml first");

        std::fs::write(project.join("kiln.toml"), "[php]\nversion = \"8.3\"\n").unwrap();
        assert_eq!(
            wanted_for_dir(&deep).unwrap().text,
            "lts/jod",
            "a kiln.toml without [node] doesn't count"
        );
    }

    #[test]
    fn picks_the_wanted_version_or_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let installed = [
            node("20.20.2", Some("iron")),
            node("22.23.4", Some("jod")),
            node("24.1.0", None),
        ];
        let here = node_for_dir(&installed, None, dir.path());
        assert_eq!(here.node.unwrap().version, "24.1.0", "newest installed");
        assert!(!here.fell_back);
        let here = node_for_dir(&installed, Some("lts"), dir.path());
        assert_eq!(here.node.unwrap().version, "22.23.4", "node.default");

        std::fs::write(dir.path().join(".nvmrc"), "v20\n").unwrap();
        let here = node_for_dir(&installed, Some("lts"), dir.path());
        assert_eq!(here.node.unwrap().version, "20.20.2");

        std::fs::write(dir.path().join(".nvmrc"), "18\n").unwrap();
        let here = node_for_dir(&installed, Some("lts"), dir.path());
        assert!(here.fell_back);
        assert_eq!(here.node.unwrap().version, "22.23.4");

        std::fs::write(dir.path().join(".nvmrc"), "lts/-1\n").unwrap();
        let here = node_for_dir(&installed, None, dir.path());
        assert!(here.fell_back && here.wanted.unwrap().spec.is_err());

        assert_eq!(node_for_dir(&[], None, dir.path()).node, None);
    }
}
