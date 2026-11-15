//! `kiln editor` and `kiln code` ([ADR 0085]): the user's editor, found in its usual places,
//! opening a site's folder or a file in it at a line. Only an editor's own `.exe` is started,
//! never a `.cmd` or `.bat`, so no shell reads the path.
//!
//! [ADR 0085]: ../../../docs/adr/0085-open-in-editor.md

use std::path::{Path, PathBuf};
use std::process::Command;

/// The setting holding the chosen editor's id.
pub const KEY: &str = "editor";

/// An editor Kiln knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    /// How it opens a file at a line.
    line: LineStyle,
    /// Where it installs: (environment variable, path under it).
    places: &'static [(&'static str, &'static str)],
    /// A `JetBrains` IDE: `%ProgramFiles%\JetBrains\<prefix> <version>\bin\<exe>`.
    jetbrains: Option<(&'static str, &'static str)>,
    /// Its program on `PATH`, and for a `.cmd` shim on `PATH`, the `.exe` beside it.
    on_path: &'static [&'static str],
    shims: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineStyle {
    /// `-g file:line`
    Goto,
    /// `--line N file`
    LineFlag,
    /// `file:line`
    Suffix,
}

/// Kiln's editors, in the order one is picked when none is chosen.
pub const KINDS: &[Kind] = &[
    Kind {
        id: "vscode",
        name: "VS Code",
        line: LineStyle::Goto,
        places: &[
            ("LOCALAPPDATA", r"Programs\Microsoft VS Code\Code.exe"),
            ("ProgramFiles", r"Microsoft VS Code\Code.exe"),
        ],
        jetbrains: None,
        on_path: &[],
        shims: &[("code.cmd", r"..\Code.exe")],
    },
    Kind {
        id: "cursor",
        name: "Cursor",
        line: LineStyle::Goto,
        places: &[("LOCALAPPDATA", r"Programs\cursor\Cursor.exe")],
        jetbrains: None,
        on_path: &[],
        shims: &[("cursor.cmd", r"..\..\..\Cursor.exe")],
    },
    Kind {
        id: "phpstorm",
        name: "PhpStorm",
        line: LineStyle::LineFlag,
        places: &[("LOCALAPPDATA", r"Programs\PhpStorm\bin\phpstorm64.exe")],
        jetbrains: Some(("PhpStorm", "phpstorm64.exe")),
        on_path: &["phpstorm64.exe"],
        shims: &[],
    },
    Kind {
        id: "zed",
        name: "Zed",
        line: LineStyle::Suffix,
        places: &[("LOCALAPPDATA", r"Programs\Zed\Zed.exe")],
        jetbrains: None,
        on_path: &["zed.exe"],
        shims: &[],
    },
    Kind {
        id: "sublime",
        name: "Sublime Text",
        line: LineStyle::Suffix,
        places: &[
            ("ProgramFiles", r"Sublime Text\subl.exe"),
            ("ProgramFiles", r"Sublime Text 3\subl.exe"),
        ],
        jetbrains: None,
        on_path: &["subl.exe"],
        shims: &[],
    },
];

/// An editor found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub kind: Kind,
    pub exe: PathBuf,
}

/// What detection may look at: environment variables, files, folders. Real or made up.
pub trait Machine {
    fn var(&self, name: &str) -> Option<String>;
    fn is_file(&self, path: &Path) -> bool;
    /// Names in a folder.
    fn list(&self, dir: &Path) -> Vec<String>;
}

struct Real;

impl Machine for Real {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
    fn list(&self, dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .collect()
    }
}

fn under(base: &str, relative: &str) -> PathBuf {
    let mut path = PathBuf::from(base);
    for part in relative.split('\\') {
        if part == ".." {
            path.pop();
        } else {
            path.push(part);
        }
    }
    path
}

/// Where `kind` is, if anywhere.
fn find(kind: &Kind, m: &dyn Machine) -> Option<PathBuf> {
    for (var, relative) in kind.places {
        if let Some(base) = m.var(var) {
            let path = under(&base, relative);
            if m.is_file(&path) {
                return Some(path);
            }
        }
    }
    if let (Some((prefix, exe)), Some(base)) = (kind.jetbrains, m.var("ProgramFiles")) {
        let root = PathBuf::from(base).join("JetBrains");
        let mut versions: Vec<String> = m
            .list(&root)
            .into_iter()
            .filter(|n| n.starts_with(prefix))
            .collect();
        // The newest first: `PhpStorm 2025.2` sorts after `PhpStorm 2024.3`.
        versions.sort();
        for version in versions.iter().rev() {
            let path = root.join(version).join("bin").join(exe);
            if m.is_file(&path) {
                return Some(path);
            }
        }
    }
    let path = m.var("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        for name in kind.on_path {
            let exe = dir.join(name);
            if m.is_file(&exe) {
                return Some(exe);
            }
        }
        for (shim, exe) in kind.shims {
            if m.is_file(&dir.join(shim)) {
                let exe = under(&dir.to_string_lossy(), exe);
                if m.is_file(&exe) {
                    return Some(exe);
                }
            }
        }
    }
    None
}

/// Every editor found, in Kiln's order.
pub fn detect(m: &dyn Machine) -> Vec<Found> {
    KINDS
        .iter()
        .filter_map(|kind| find(kind, m).map(|exe| Found { kind: *kind, exe }))
        .collect()
}

/// The editor to use: the chosen one if found, else the first found.
#[must_use]
pub fn pick<'a>(found: &'a [Found], chosen: Option<&str>) -> Option<&'a Found> {
    chosen
        .and_then(|id| found.iter().find(|f| f.kind.id == id))
        .or_else(|| found.first())
}

/// The arguments that open `file` at `line`, or a folder.
fn args(kind: &Kind, target: &Path, line: Option<u32>) -> Vec<String> {
    let path = target.to_string_lossy().into_owned();
    match (line, kind.line) {
        (None, _) => vec![path],
        (Some(n), LineStyle::Goto) => vec!["-g".into(), format!("{path}:{n}")],
        (Some(n), LineStyle::LineFlag) => vec!["--line".into(), n.to_string(), path],
        (Some(n), LineStyle::Suffix) => vec![format!("{path}:{n}")],
    }
}

/// `file.php:42`, `file.php(42)` or `file.php`, as a path and a line.
fn goto(text: &str) -> (String, Option<u32>) {
    if let Some(rest) = text.strip_suffix(')')
        && let Some((path, n)) = rest.rsplit_once('(')
        && let Ok(n) = n.parse()
    {
        return (path.to_owned(), Some(n));
    }
    match text.rsplit_once(':') {
        // `C:\x.php` has a colon too: only digits after the last one are a line.
        Some((path, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => {
            (path.to_owned(), n.parse().ok())
        }
        _ => (text.to_owned(), None),
    }
}

/// `file` (relative to `folder`, or absolute) as a path inside `folder`, resolved.
fn inside(folder: &Path, file: &str) -> Result<PathBuf, String> {
    let joined = folder.join(file);
    let full = std::fs::canonicalize(&joined)
        .map(crate::web::strip_verbatim)
        .map_err(|e| format!("cannot open {}: {e}", joined.display()))?;
    let root = std::fs::canonicalize(folder)
        .map(crate::web::strip_verbatim)
        .map_err(|e| e.to_string())?;
    if full.starts_with(&root) {
        Ok(full)
    } else {
        Err(format!(
            "{} isn't inside the site's folder {}",
            full.display(),
            root.display()
        ))
    }
}

/// Starts the editor on its own: no console, not waited for.
fn launch(exe: &Path, args: &[String]) -> Result<(), String> {
    let mut command = Command::new(exe);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))
}

fn chosen() -> Option<String> {
    crate::web::open_store()
        .ok()
        .and_then(|s| s.setting(KEY).ok().flatten())
}

/// `kiln editor`: the editors found, the chosen one, the one in use.
#[allow(clippy::unnecessary_wraps)] // as every command: `run` dispatches on Result
pub fn show() -> Result<(), String> {
    let found = detect(&Real);
    let chosen = chosen();
    let using = pick(&found, chosen.as_deref());
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "chosen": chosen,
            "using": using.map(|f| f.kind.id),
            "found": found.iter().map(|f| serde_json::json!({"id": f.kind.id, "name": f.kind.name, "exe": f.exe})).collect::<Vec<_>>(),
            "known": KINDS.iter().map(|k| serde_json::json!({"id": k.id, "name": k.name})).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    if found.is_empty() {
        say!("No editor found (Kiln looks for VS Code, Cursor, PhpStorm, Zed and Sublime Text).");
    }
    for f in &found {
        let mark = if using.is_some_and(|u| u.kind.id == f.kind.id) {
            "*"
        } else {
            " "
        };
        say!(
            "{mark} {:<9} {:<13} {}",
            f.kind.id,
            f.kind.name,
            f.exe.display()
        );
    }
    if let Some(id) = chosen.filter(|id| !found.iter().any(|f| f.kind.id == *id)) {
        say!("{id} is chosen but wasn't found; the first found is used.");
    }
    Ok(())
}

/// `kiln editor use <id|auto>`.
pub fn choose(id: &str) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    if id == "auto" {
        store.remove_setting(KEY).map_err(|e| e.to_string())?;
        say!("Kiln picks the editor it finds first again.");
        return Ok(());
    }
    let kind = KINDS.iter().find(|k| k.id == id).ok_or_else(|| {
        format!(
            "Kiln doesn't know the editor {id:?}: {} or auto",
            KINDS.iter().map(|k| k.id).collect::<Vec<_>>().join(", ")
        )
    })?;
    store.set_setting(KEY, kind.id).map_err(|e| e.to_string())?;
    if find(kind, &Real).is_some() {
        say!("Sites open in {} now.", kind.name);
    } else {
        say!(
            "Sites open in {} once Kiln finds it (it isn't in its usual places now).",
            kind.name
        );
    }
    Ok(())
}

/// `kiln code [<site>] [--goto <file>[:<line>]]`.
pub fn code(site: Option<&str>, goto_text: Option<&str>) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let resolved = crate::web::resolved(&store)?;
    let folder = if let Some(name) = site {
        let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
        let host = crate::web::host_name_under(name, &tld)?;
        resolved
            .sites
            .iter()
            .find(|s| s.host == host)
            .map(|s| s.project.clone())
            .ok_or_else(|| format!("Kiln doesn't serve {host}"))?
    } else {
        let here = std::fs::canonicalize(std::env::current_dir().map_err(|e| e.to_string())?)
            .map(crate::web::strip_verbatim)
            .map_err(|e| e.to_string())?;
        resolved
            .sites
            .iter()
            .filter(|s| here.starts_with(&s.project))
            .max_by_key(|s| s.project.as_os_str().len())
            .map(|s| s.project.clone())
            .ok_or("this folder isn't in one of Kiln's sites; name the site")?
    };
    let chosen = store.setting(KEY).ok().flatten();
    drop(store);
    let found = detect(&Real);
    let editor = pick(&found, chosen.as_deref())
        .ok_or("no editor found: Kiln looks for VS Code, Cursor, PhpStorm, Zed and Sublime Text")?;
    let (target, line) = if let Some(text) = goto_text {
        let (file, line) = goto(text);
        (inside(&folder, &file)?, line)
    } else {
        (folder, None)
    };
    launch(&editor.exe, &args(&editor.kind, &target, line))?;
    say!(
        "Opened {}{} in {}.",
        target.display(),
        line.map(|n| format!(" at line {n}")).unwrap_or_default(),
        editor.kind.name
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Default)]
    struct Fake {
        vars: BTreeMap<&'static str, String>,
        files: BTreeSet<PathBuf>,
    }

    impl Machine for Fake {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
        fn list(&self, dir: &Path) -> Vec<String> {
            self.files
                .iter()
                .filter_map(|f| {
                    f.strip_prefix(dir)
                        .ok()?
                        .components()
                        .next()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        }
    }

    fn fake() -> Fake {
        let mut m = Fake::default();
        m.vars.insert("LOCALAPPDATA", "L".into());
        m.vars.insert("ProgramFiles", "P".into());
        m
    }

    #[test]
    fn editors_are_found_in_their_places_newest_jetbrains_first() {
        let mut m = fake();
        m.files
            .insert(under("L", r"Programs\Microsoft VS Code\Code.exe"));
        m.files
            .insert(under("P", r"JetBrains\PhpStorm 2024.3\bin\phpstorm64.exe"));
        m.files
            .insert(under("P", r"JetBrains\PhpStorm 2025.2\bin\phpstorm64.exe"));
        let found = detect(&m);
        let ids: Vec<&str> = found.iter().map(|f| f.kind.id).collect();
        assert_eq!(ids, ["vscode", "phpstorm"]);
        assert_eq!(
            found[1].exe,
            under("P", r"JetBrains\PhpStorm 2025.2\bin\phpstorm64.exe")
        );
        assert_eq!(pick(&found, None).unwrap().kind.id, "vscode");
        assert_eq!(pick(&found, Some("phpstorm")).unwrap().kind.id, "phpstorm");
        assert_eq!(pick(&found, Some("zed")).unwrap().kind.id, "vscode");
    }

    #[test]
    fn a_cmd_shim_on_path_leads_to_the_exe_never_the_cmd() {
        let mut m = fake();
        let bin = PathBuf::from("V").join("bin");
        m.vars.insert(
            "PATH",
            std::env::join_paths([bin.clone()])
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        m.files.insert(bin.join("code.cmd"));
        assert!(detect(&m).is_empty(), "no Code.exe beside it: nothing");
        m.files.insert(PathBuf::from("V").join("Code.exe"));
        assert_eq!(detect(&m)[0].exe, PathBuf::from("V").join("Code.exe"));
    }

    #[test]
    fn each_editor_opens_a_line_its_own_way() {
        let file = Path::new("app").join("X.php");
        let shown = file.to_string_lossy().into_owned();
        let by = |id: &str| *KINDS.iter().find(|k| k.id == id).unwrap();
        assert_eq!(
            args(&by("vscode"), &file, Some(42)),
            ["-g".to_owned(), format!("{shown}:42")]
        );
        assert_eq!(
            args(&by("phpstorm"), &file, Some(42)),
            ["--line".to_owned(), "42".into(), shown.clone()]
        );
        assert_eq!(args(&by("zed"), &file, Some(42)), [format!("{shown}:42")]);
        assert_eq!(args(&by("sublime"), &file, None), [shown]);
    }

    #[test]
    fn a_line_is_read_from_either_way_php_writes_it() {
        assert_eq!(
            goto(r"C:\s\app\X.php:42"),
            (r"C:\s\app\X.php".into(), Some(42))
        );
        assert_eq!(
            goto(r"C:\s\vendor\y.php(17)"),
            (r"C:\s\vendor\y.php".into(), Some(17))
        );
        assert_eq!(goto(r"C:\s\app\X.php"), (r"C:\s\app\X.php".into(), None));
        assert_eq!(goto("routes/web.php"), ("routes/web.php".into(), None));
    }

    #[test]
    fn only_files_inside_the_site_open() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("shop");
        std::fs::create_dir_all(site.join("app")).unwrap();
        std::fs::write(site.join("app").join("X.php"), "").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "").unwrap();
        assert!(
            inside(&site, "app/X.php")
                .unwrap()
                .ends_with(Path::new("app").join("X.php"))
        );
        let absolute = site.join("app").join("X.php");
        assert!(inside(&site, &absolute.to_string_lossy()).is_ok());
        assert!(
            inside(&site, "../secret.txt")
                .unwrap_err()
                .contains("isn't inside")
        );
        assert!(
            inside(&site, "app/missing.php")
                .unwrap_err()
                .contains("cannot open")
        );
    }
}
