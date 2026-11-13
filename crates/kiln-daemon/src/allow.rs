//! Which `kiln` commands a client may run as a job, and how ([ADR 0053] §4).
//!
//! A job is what the user could run in a terminal, but only these commands: no uninstall
//! (it stops the daemon), no reads (they have methods), nothing interactive. A preview is
//! only for commands that ask before changing anything, so asking for one can never change
//! the system.
//!
//! [ADR 0053]: ../../../docs/adr/0053-api-v2-and-json-output.md

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Run up to the confirmation and stop there, with the preview.
    Preview,
    /// Run it, answering yes to its confirmation.
    Apply,
}

impl Mode {
    /// # Errors
    /// Anything but `preview` or `apply`.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "preview" => Ok(Self::Preview),
            "apply" => Ok(Self::Apply),
            other => Err(format!(
                "mode must be \"preview\" or \"apply\", not {other:?}"
            )),
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Apply => "apply",
        }
    }
}

/// The commands, by their words, and whether each asks before changing anything.
const COMMANDS: &[(&[&str], bool)] = &[
    (&["link"], false),
    (&["unlink"], false),
    (&["park"], false),
    (&["unpark"], false),
    (&["secure"], false),
    (&["unsecure"], false),
    (&["isolate"], false),
    (&["unisolate"], false),
    (&["proxy"], false),
    (&["unproxy"], false),
    (&["new"], false),
    (&["import", "herd"], false),
    (&["php", "install"], false),
    (&["php", "remove"], false),
    (&["php", "default"], false),
    (&["php", "update"], false),
    (&["php", "rollback"], false),
    (&["php", "workers"], false),
    (&["php", "ext", "on"], false),
    (&["php", "ext", "off"], false),
    (&["node", "install"], false),
    (&["node", "remove"], false),
    (&["node", "default"], false),
    (&["composer", "install"], false),
    (&["service", "add"], false),
    (&["service", "start"], false),
    (&["service", "stop"], false),
    (&["service", "restart"], false),
    (&["service", "port"], false),
    (&["service", "remove"], false),
    (&["services", "up"], false),
    (&["services", "down"], false),
    (&["env", "use"], true),
    (&["tool", "add"], false),
    (&["tool", "remove"], false),
    (&["db", "dump"], false),
    (&["db", "restore"], false),
    (&["undo"], true),
    (&["web", "install"], false),
    (&["web", "start"], false),
    (&["web", "stop"], false),
    (&["web", "restart"], false),
    (&["ca", "init"], false),
    (&["ca", "trust"], true),
    (&["hosts", "sync"], true),
    (&["shims", "install"], true),
    (&["app", "enable"], true),
    (&["app", "disable"], true),
    (&["daemon", "enable"], true),
    (&["daemon", "disable"], true),
];

/// Flags only the daemon decides, never a client.
const RESERVED: &[&str] = &["--json", "--confirmed", "--yes", "-y"];

/// Whether a client may run `kiln <args>` this way.
///
/// # Errors
/// Why not, in words for the app to show.
pub fn check(args: &[String], mode: Mode) -> Result<(), String> {
    if let Some(flag) = args
        .iter()
        .find(|a| RESERVED.contains(&a.split('=').next().unwrap_or_default()))
    {
        return Err(format!("{flag} is set by kilnd, not by the caller"));
    }
    let Some((words, asks)) = COMMANDS.iter().find(|(words, _)| {
        args.len() >= words.len() && words.iter().zip(args).all(|(w, a)| *w == a.as_str())
    }) else {
        let shown = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
        return Err(format!("`kiln {shown}` can't be run through the API"));
    };
    if mode == Mode::Preview && !asks {
        return Err(format!(
            "`kiln {}` makes its change without asking, so it has no preview to stop at",
            words.join(" ")
        ));
    }
    Ok(())
}

/// For `service add <kind>`: the `--project` folder it names, which the daemon checks is a Kiln
/// site's ([ADR 0060] §5). A kind that runs in a project must name one: without it, the
/// command would use the daemon's own working folder.
///
/// # Errors
/// A project kind without `--project`, or `--project` without a folder.
///
/// [ADR 0060]: ../../../docs/adr/0060-dev-processes.md
pub fn project_of_service_add(args: &[String]) -> Result<Option<&str>, String> {
    if args.len() < 3 || args[0] != "service" || args[1] != "add" {
        return Ok(None);
    }
    let mut project = None;
    let mut rest = args[3..].iter();
    while let Some(arg) = rest.next() {
        if arg == "--project" {
            project = Some(
                rest.next()
                    .map(String::as_str)
                    .ok_or("--project needs a folder")?,
            );
        } else if let Some(value) = arg.strip_prefix("--project=") {
            project = Some(value);
        }
    }
    let in_project = kiln_services::provider(&args[2])
        .is_some_and(kiln_services::ServiceProvider::runs_in_project);
    if in_project && project.is_none() {
        return Err(format!(
            "`kiln service add {}` through the API needs --project <a site's folder>",
            args[2]
        ));
    }
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_process_names_its_folder() {
        assert_eq!(
            project_of_service_add(&args("service add queue --project D:/Code/shop")),
            Ok(Some("D:/Code/shop"))
        );
        assert_eq!(
            project_of_service_add(&args("service add vite --project=D:/Code/shop")),
            Ok(Some("D:/Code/shop"))
        );
        assert!(project_of_service_add(&args("service add queue")).is_err());
        assert!(project_of_service_add(&args("service add queue --project")).is_err());
        assert_eq!(project_of_service_add(&args("service add mysql")), Ok(None));
        assert_eq!(project_of_service_add(&args("link shop")), Ok(None));
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn listed_commands_run_with_their_arguments() {
        assert!(check(&args("link shop --path D:/Code/shop --secure"), Mode::Apply).is_ok());
        assert!(check(&args("php ext on xdebug --php 8.4"), Mode::Apply).is_ok());
        assert!(check(&args("service remove mysql-8 --delete-data"), Mode::Apply).is_ok());
    }

    #[test]
    fn everything_else_is_refused() {
        for refused in [
            "",
            "uninstall --delete-data",
            "sites",
            "db mysql-8",
            "php ext list",
            "php",
            "daemon stop",
            "--help",
            "-V",
        ] {
            assert!(check(&args(refused), Mode::Apply).is_err(), "{refused}");
        }
    }

    #[test]
    fn the_daemons_own_flags_are_never_passed_in() {
        for refused in [
            "undo --yes",
            "undo -y",
            "env use mysql-8 --confirmed",
            "link x --json",
            "link x --json=true",
        ] {
            let error = check(&args(refused), Mode::Apply).unwrap_err();
            assert!(error.contains("set by kilnd"), "{refused}: {error}");
        }
    }

    #[test]
    fn a_preview_only_for_commands_that_ask_before_changing() {
        for asks in [
            "hosts sync",
            "ca trust",
            "undo 12",
            "env use redis-8",
            "daemon enable",
            "shims install",
        ] {
            assert!(check(&args(asks), Mode::Preview).is_ok(), "{asks}");
        }
        let error = check(&args("php install 8.4"), Mode::Preview).unwrap_err();
        assert!(error.contains("without asking"), "{error}");
        assert!(check(&args("services up"), Mode::Preview).is_err());
    }

    #[test]
    fn modes_parse() {
        assert_eq!(Mode::parse("preview"), Ok(Mode::Preview));
        assert_eq!(Mode::parse("apply"), Ok(Mode::Apply));
        assert!(Mode::parse("force").is_err());
    }
}
