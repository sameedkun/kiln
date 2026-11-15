//! `kiln run <site> <command> [args...]` ([ADR 0084]): a site's own commands (`artisan`,
//! Kiln's Composer, a tool in `vendor/bin`) with the site's PHP, in its folder. Never another
//! program. On a terminal the command has the console; with `--json` each line is a `note`.
//! What it starts is in a kill-on-close job, so stopping `kiln` stops all of it.
//!
//! [ADR 0084]: ../../../docs/adr/0084-site-commands.md

use std::ffi::OsString;
use std::io::Read;
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

/// Whether `path` is a PHP script (`<?php`, or a shebang naming php): what Kiln runs with
/// PHP. A shell script (`sail`) or a `.bat` proxy isn't.
fn is_php_script(path: &Path) -> bool {
    let mut head = [0u8; 128];
    let read = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    let head = String::from_utf8_lossy(&head[..read]);
    let first = head.lines().next().unwrap_or_default();
    head.starts_with("<?php") || (first.starts_with("#!") && first.contains("php"))
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
    let tool =
        tool_name(name).filter(|n| is_php_script(&folder.join("vendor").join("bin").join(n)));
    tool.map(|n| Program::Tool(n.to_owned())).ok_or_else(|| {
        format!(
            "`{word}` isn't artisan, composer or a tool in {}'s vendor/bin",
            folder.display()
        )
    })
}

/// Whether an artisan command changes the database's shape or contents (ADR 0086): migrations
/// (not `migrate:status`), `db:wipe`, `db:seed`.
fn changes_database(args: &[String]) -> bool {
    args.first().is_some_and(|c| {
        (c == "migrate" || c.starts_with("migrate:")) && c != "migrate:status"
            || c == "db:wipe"
            || c == "db:seed"
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
            std::thread::spawn(move || crate::out::each_line(err, &mut crate::out::note))
        });
        if let Some(out) = child.stdout.take() {
            crate::out::each_line(out, &mut crate::out::note);
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
    if program == Program::Artisan && changes_database(args) {
        crate::snapshot::before(&folder, crate::snapshot::Reason::BeforeMigrate);
    }
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
    /// `phpunit.xml` sets no test database, so a test run with `RefreshDatabase` uses (and
    /// empties) the one `.env` names.
    tests_use_env_database: bool,
}

/// Whether `phpunit.xml` (or `.dist`) sets `DB_CONNECTION` or `DB_DATABASE` outside a comment.
fn tests_use_env_database(folder: &Path) -> bool {
    let Some(text) = ["phpunit.xml", "phpunit.xml.dist"]
        .iter()
        .find_map(|f| std::fs::read_to_string(folder.join(f)).ok())
    else {
        return false;
    };
    // Comments out, then look for the settings.
    let mut live = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("<!--") {
        live.push_str(&rest[..at]);
        rest = rest[at..]
            .find("-->")
            .map_or("", |end| &rest[at + end + 3..]);
    }
    live.push_str(rest);
    !live.contains("name=\"DB_CONNECTION\"") && !live.contains("name=\"DB_DATABASE\"")
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
        .filter(|n| {
            tool_name(n).is_some() && is_php_script(&folder.join("vendor").join("bin").join(n))
        })
        .collect();
    tools.sort();
    Found {
        artisan: folder.join("artisan").is_file(),
        scripts,
        tools,
        tests_use_env_database: tests_use_env_database(folder),
    }
}

/// One of the app's buttons; `warn` is said, and agreed to, before it runs.
#[derive(Debug, PartialEq, Eq)]
struct Suggestion {
    label: &'static str,
    args: Vec<&'static str>,
    warn: Option<&'static str>,
}

const TESTS_WARNING: &str = "phpunit.xml sets no test database, so tests that refresh the database run on the one .env names, and may empty it.";

/// The app's buttons: tests, style, analysis, then a few artisan reads.
fn suggestions(f: &Found) -> Vec<Suggestion> {
    let has = |t: &str| f.tools.iter().any(|x| x == t);
    let mut out: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
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
    out.into_iter()
        .map(|(label, args)| Suggestion {
            warn: (label == "Run the tests" && f.tests_use_env_database).then_some(TESTS_WARNING),
            label,
            args,
        })
        .collect()
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
            "suggested": suggested.iter().map(|s| serde_json::json!({"label": s.label, "args": s.args, "warn": s.warn})).collect::<Vec<_>>(),
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
    for s in &suggested {
        say!("  {}: kiln run {host} {}", s.label, s.args.join(" "));
        if let Some(warn) = s.warn {
            say!("    ({warn})");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHP_TOOL: &str = "#!/usr/bin/env php\n<?php\n// a Composer proxy\n";

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("vendor").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for f in ["pest", "pint", "phpstan"] {
            std::fs::write(bin.join(f), PHP_TOOL).unwrap();
        }
        std::fs::write(
            bin.join("pest.bat"),
            "@ECHO OFF\r\nphp \"%~dp0pest\" %*\r\n",
        )
        .unwrap();
        std::fs::write(bin.join("sail"), "#!/usr/bin/env bash\necho sail\n").unwrap();
        std::fs::write(dir.path().join("artisan"), "#!/usr/bin/env php\n<?php\n").unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"scripts":{"test":"pest","dev":"x","post-autoload-dump":"y","pre-install-cmd":"z"}}"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn only_artisan_composer_and_vendor_bin_php_tools_run() {
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
            "sail",
            "pest.bat",
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
    fn the_app_gets_scripts_php_tools_and_suggestions() {
        let dir = project();
        let f = found(dir.path());
        assert!(f.artisan);
        assert_eq!(f.scripts, ["dev", "test"]);
        assert_eq!(f.tools, ["pest", "phpstan", "pint"]);
        let all = suggestions(&f);
        let args: Vec<&Vec<&str>> = all.iter().map(|s| &s.args).collect();
        assert_eq!(*args[0], ["artisan", "test"]);
        assert_eq!(*args[1], ["pint", "--test"]);
        assert_eq!(*args[2], ["phpstan", "analyse", "--no-progress"]);
        assert!(args.contains(&&vec!["artisan", "migrate:status"]));
        // No phpunit.xml: nothing to say before the tests.
        assert_eq!(all[0].warn, None);
    }

    #[test]
    fn tests_on_the_envs_database_are_warned_about() {
        let dir = project();
        let xml = dir.path().join("phpunit.xml");
        std::fs::write(&xml, "<php>\n<env name=\"APP_ENV\" value=\"testing\"/>\n<!-- <env name=\"DB_CONNECTION\" value=\"sqlite\"/> -->\n</php>").unwrap();
        assert_eq!(suggestions(&found(dir.path()))[0].warn, Some(TESTS_WARNING));
        std::fs::write(&xml, "<php>\n<env name=\"DB_CONNECTION\" value=\"sqlite\"/>\n<env name=\"DB_DATABASE\" value=\":memory:\"/>\n</php>").unwrap();
        assert_eq!(suggestions(&found(dir.path()))[0].warn, None);
    }

    #[test]
    fn migrations_wipes_and_seeds_change_the_database_status_doesnt() {
        let yes =
            |w: &[&str]| changes_database(&w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        for c in [
            &["migrate"][..],
            &["migrate:fresh", "--seed"],
            &["migrate:rollback"],
            &["db:wipe"],
            &["db:seed"],
        ] {
            assert!(yes(c), "{c:?}");
        }
        for c in [&["migrate:status"][..], &["route:list"], &["about"], &[]] {
            assert!(!yes(c), "{c:?}");
        }
    }

    #[test]
    fn the_app_never_waits_on_a_question() {
        let dir = project();
        let php = PhpRecord {
            version: "8.4.1".into(),
            dir: dir.path().join("php"),
            archive: String::new(),
            sha256: String::new(),
        };
        let args = |c: &Command| -> Vec<String> {
            c.get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let run = |program: &Program, words: &[&str], json: bool| {
            let words: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
            args(&command(program, &words, dir.path(), &php, json).unwrap())
        };
        assert_eq!(
            run(&Program::Artisan, &["migrate"], true),
            ["artisan", "migrate", "--no-interaction"]
        );
        assert_eq!(
            run(&Program::Artisan, &["migrate", "-n"], true),
            ["artisan", "migrate", "-n"]
        );
        assert_eq!(
            run(&Program::Artisan, &["migrate"], false),
            ["artisan", "migrate"]
        );
        let pint: PathBuf = ["vendor", "bin", "pint"].iter().collect();
        assert_eq!(
            run(&Program::Tool("pint".into()), &["--test"], true),
            [pint.to_string_lossy().into_owned(), "--test".to_owned()]
        );
    }
}
