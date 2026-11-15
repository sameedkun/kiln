//! `kiln run <site> <command> [args...]` ([ADR 0084]): a site's own commands (`artisan`,
//! Kiln's Composer, a tool in `vendor/bin`) with the site's PHP, in its folder. Never another
//! program. On a terminal the command has the console; with `--json` each line is a `note`.
//! What it starts is in a kill-on-close job, so stopping `kiln` stops all of it.
//!
//! [ADR 0084]: ../../../docs/adr/0084-site-commands.md

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use kiln_state::PhpRecord;

/// What `kiln run` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Program {
    Artisan,
    Composer,
    /// A tool in `vendor/bin`, by its file name.
    Tool(String),
}

/// A tool's name as Kiln takes it: letters, digits, `.`, `_`, `-`, nothing that leaves the
/// folder.
fn tool_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    ok.then_some(name)
}

/// The program the first word names, if the project has it.
fn program(word: &str, folder: &Path) -> Result<Program, String> {
    match word {
        "artisan" if folder.join("artisan").is_file() => return Ok(Program::Artisan),
        "artisan" => return Err(format!("{} has no artisan", folder.display())),
        "composer" => return Ok(Program::Composer),
        _ => {}
    }
    let name = word
        .strip_prefix("vendor/bin/")
        .or_else(|| word.strip_prefix(r"vendor\bin\"))
        .unwrap_or(word);
    let tool = tool_name(name).filter(|n| folder.join("vendor").join("bin").join(n).is_file());
    tool.map(|n| Program::Tool(n.to_owned())).ok_or_else(|| {
        format!(
            "`{word}` isn't artisan, composer or a tool in {}'s vendor/bin",
            folder.display()
        )
    })
}

/// Whether the arguments already say `--no-interaction`.
fn quiet_already(args: &[String]) -> bool {
    args.iter().any(|a| a == "-n" || a == "--no-interaction")
}

/// The command to run.
fn command(
    program: &Program,
    args: &[String],
    folder: &Path,
    php: &PhpRecord,
    json: bool,
) -> Result<Command, String> {
    let mut args: Vec<String> = args.to_vec();
    if json && matches!(program, Program::Artisan | Program::Composer) && !quiet_already(&args) {
        args.push("--no-interaction".into());
    }
    let mut command = match program {
        Program::Composer => crate::new::composer_command(
            args.into_iter().map(OsString::from).collect(),
            folder,
            php,
        )?,
        Program::Artisan | Program::Tool(_) => {
            let script: PathBuf = match program {
                Program::Tool(name) => ["vendor", "bin", name].iter().collect(),
                _ => PathBuf::from("artisan"),
            };
            let mut command = Command::new(php.dir.join(kiln_php::PHP_EXE));
            command
                .arg(script)
                .args(args)
                .current_dir(folder)
                .env("PATH", crate::new::path_with(&php.dir)?);
            command
        }
    };
    command.current_dir(folder);
    Ok(command)
}

/// Runs it: on a terminal with the console; with `--json`, each line a note, standard input
/// empty. In a kill-on-close job on Windows.
fn run_tree(mut command: Command) -> Result<ExitStatus, String> {
    let json = crate::out::json();
    if json {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
    } else {
        #[cfg(windows)]
        {
            // Ctrl+C reaches the command (same console); Kiln waits for it to stop.
            let _ = kiln_sys::win::survive_ctrl_c();
        }
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start it: {e}"))?;
    #[cfg(windows)]
    let _tree = kiln_sys::win::KillOnCloseJob::new()
        .and_then(|job| job.assign(&child).map(|()| job))
        .ok();
    if json {
        let stderr = child.stderr.take().map(|err| {
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    crate::out::note(&line);
                }
            })
        });
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                crate::out::note(&line);
            }
        }
        if let Some(thread) = stderr {
            let _ = thread.join();
        }
    }
    child.wait().map_err(|e| e.to_string())
}

/// The site's folder and PHP.
fn site(site: &str) -> Result<(String, PathBuf, PhpRecord), String> {
    let store = crate::web::open_store()?;
    let tld = kiln_runtime::tld(&store).map_err(|e| e.to_string())?;
    let host = crate::web::host_name_under(site, &tld)?;
    let found = crate::web::resolved(&store)?
        .sites
        .into_iter()
        .find(|s| s.host == host)
        .ok_or_else(|| format!("Kiln doesn't serve {host}"))?;
    let php = kiln_runtime::php_for_dir(&store, &found.project)
        .map_err(|e| e.to_string())?
        .php
        .php
        .ok_or_else(|| format!("Kiln has no PHP for {host}: `kiln php install` first"))?;
    Ok((host.to_string(), found.project, php))
}

/// `kiln run <site> <command> [args...]`.
pub fn run(name: &str, words: &[String]) -> Result<(), String> {
    let Some((first, args)) = words.split_first() else {
        return Err(
            "say what to run: artisan, composer or a vendor/bin tool (`kiln run <site> --list`)"
                .into(),
        );
    };
    let (host, folder, php) = site(name)?;
    let program = program(first, &folder)?;
    let shown = std::iter::once(first.as_str())
        .chain(args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    crate::out::step(&format!("{shown} in {host} (PHP {})", php.version));
    let status = run_tree(command(&program, args, &folder, &php, crate::out::json())?)?;
    let exit = status.code();
    if crate::out::json() {
        crate::out::data(serde_json::json!({ "host": host, "command": shown, "exit": exit }));
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{shown}` exited with code {}",
            exit.map_or_else(|| "?".to_owned(), |c| c.to_string())
        ))
    }
}

/// What a site can run, for `--list` and the app's buttons.
#[derive(Debug, Default, PartialEq, Eq)]
struct Found {
    artisan: bool,
    scripts: Vec<String>,
    tools: Vec<String>,
}

fn found(folder: &Path) -> Found {
    let scripts = std::fs::read_to_string(folder.join("composer.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|c| c["scripts"].as_object().cloned())
        .map(|s| {
            s.keys()
                // Composer's own hooks (post-autoload-dump, pre-install-cmd) aren't to run.
                .filter(|k| !k.starts_with("pre-") && !k.starts_with("post-"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut tools: Vec<String> = std::fs::read_dir(folder.join("vendor").join("bin"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.to_ascii_lowercase().ends_with(".bat") && tool_name(n).is_some())
        .collect();
    tools.sort();
    Found {
        artisan: folder.join("artisan").is_file(),
        scripts,
        tools,
    }
}

/// The app's buttons: tests, style, analysis, then a few artisan reads.
fn suggestions(f: &Found) -> Vec<(&'static str, Vec<&'static str>)> {
    let has = |t: &str| f.tools.iter().any(|x| x == t);
    let mut out = Vec::new();
    if f.artisan && (has("pest") || has("phpunit")) {
        out.push(("Run the tests", vec!["artisan", "test"]));
    } else if has("pest") {
        out.push(("Run the tests", vec!["pest"]));
    } else if has("phpunit") {
        out.push(("Run the tests", vec!["phpunit"]));
    }
    if has("pint") {
        out.push(("Check the code style (Pint)", vec!["pint", "--test"]));
    }
    if has("phpstan") {
        out.push((
            "Analyse with PHPStan",
            vec!["phpstan", "analyse", "--no-progress"],
        ));
    }
    if f.artisan {
        out.push(("Migrations", vec!["artisan", "migrate:status"]));
        out.push(("Routes", vec!["artisan", "route:list"]));
        out.push(("About the app", vec!["artisan", "about"]));
    }
    out
}

/// `kiln run <site> --list`.
pub fn list(name: &str) -> Result<(), String> {
    let (host, folder, _) = site(name)?;
    let f = found(&folder);
    let suggested = suggestions(&f);
    if crate::out::json() {
        crate::out::data(serde_json::json!({
            "host": host,
            "artisan": f.artisan,
            "scripts": f.scripts,
            "tools": f.tools,
            "suggested": suggested.iter().map(|(label, args)| serde_json::json!({"label": label, "args": args})).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    say!("{host}: artisan {}", if f.artisan { "yes" } else { "no" });
    if !f.scripts.is_empty() {
        say!(
            "Composer scripts (`composer <name>`): {}",
            f.scripts.join(", ")
        );
    }
    if !f.tools.is_empty() {
        say!("vendor/bin: {}", f.tools.join(", "));
    }
    for (label, args) in &suggested {
        say!("  {label}: kiln run {host} {}", args.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for f in ["pest", "pest.bat", "pint", "pint.bat", "phpstan"] {
            std::fs::write(bin.join(f), "").unwrap();
        }
        std::fs::write(dir.path().join("artisan"), "").unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"scripts":{"test":"pest","dev":"x","post-autoload-dump":"y","pre-install-cmd":"z"}}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn only_artisan_composer_and_vendor_bin_tools_run() {
        let dir = project();
        let p = dir.path();
        assert_eq!(program("artisan", p), Ok(Program::Artisan));
        assert_eq!(program("composer", p), Ok(Program::Composer));
        assert_eq!(program("pest", p), Ok(Program::Tool("pest".into())));
        assert_eq!(
            program("vendor/bin/pint", p),
            Ok(Program::Tool("pint".into()))
        );
        assert_eq!(
            program(r"vendor\bin\pint", p),
            Ok(Program::Tool("pint".into()))
        );
        for refused in [
            "php",
            "phpunit",
            "cmd",
            "../../evil",
            "vendor/bin/../x",
            r"C:\x.exe",
            ".hidden",
            "",
        ] {
            assert!(program(refused, p).is_err(), "{refused}");
        }
        let bare = tempfile::tempdir().unwrap();
        assert!(
            program("artisan", bare.path())
                .unwrap_err()
                .contains("has no artisan")
        );
    }

    #[test]
    fn the_app_gets_scripts_tools_and_suggestions() {
        let dir = project();
        let f = found(dir.path());
        assert!(f.artisan);
        assert_eq!(f.scripts, ["dev", "test"]);
        assert_eq!(f.tools, ["pest", "phpstan", "pint"]);
        let labels: Vec<Vec<&str>> = suggestions(&f).into_iter().map(|(_, a)| a).collect();
        assert_eq!(labels[0], ["artisan", "test"]);
        assert_eq!(labels[1], ["pint", "--test"]);
        assert_eq!(labels[2], ["phpstan", "analyse", "--no-progress"]);
        assert!(labels.contains(&vec!["artisan", "migrate:status"]));
    }

    #[test]
    fn the_app_never_waits_on_a_question() {
        let php = PhpRecord {
            version: "8.4.1".into(),
            dir: PathBuf::from(r"C:\php"),
            archive: String::new(),
            sha256: String::new(),
        };
        let dir = project();
        let args = |c: &Command| -> Vec<String> {
            c.get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let artisan = command(
            &Program::Artisan,
            &["migrate".into()],
            dir.path(),
            &php,
            true,
        )
        .unwrap();
        assert_eq!(args(&artisan), ["artisan", "migrate", "--no-interaction"]);
        let given = command(
            &Program::Artisan,
            &["migrate".into(), "-n".into()],
            dir.path(),
            &php,
            true,
        )
        .unwrap();
        assert_eq!(args(&given), ["artisan", "migrate", "-n"]);
        let terminal = command(
            &Program::Artisan,
            &["migrate".into()],
            dir.path(),
            &php,
            false,
        )
        .unwrap();
        assert_eq!(args(&terminal), ["artisan", "migrate"]);
        let pint = command(
            &Program::Tool("pint".into()),
            &["--test".into()],
            dir.path(),
            &php,
            true,
        )
        .unwrap();
        assert_eq!(
            args(&pint),
            [
                Path::new("vendor")
                    .join("bin")
                    .join("pint")
                    .to_string_lossy()
                    .into_owned(),
                "--test".into()
            ]
        );
    }
}
