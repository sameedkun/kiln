//! `kiln new <name>` ([ADR 0031], [ADR 0070]): a project made with Kiln's PHP and Composer from
//! one of Kiln's templates (Laravel, its starter kits, Symfony, Bedrock, an empty site), served
//! at once.
//!
//! [ADR 0031]: ../../../docs/adr/0031-kiln-new-laravel-projects.md
//! [ADR 0070]: ../../../docs/adr/0070-new-site-templates.md

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use kiln_core::Tld;
use kiln_runtime::shim::{COMPOSER_PHAR, Tool};
use kiln_runtime::{DirPhp, SitePhp};
use kiln_sites::manifest;
use kiln_state::PhpRecord;

/// What `kiln new` can start a project from ([ADR 0070]).
///
/// [ADR 0070]: ../../../docs/adr/0070-new-site-templates.md
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Template {
    /// The name `--template` takes.
    pub id: &'static str,
    /// What the app shows.
    pub label: &'static str,
    /// The Composer package created from; `None` for an empty folder.
    pub package: Option<&'static str>,
    /// A Laravel app: `--db` applies, and its migrations run.
    pub laravel: bool,
    /// Its front end is built once with Kiln's Node (`npm install`, `npm run build`), so the
    /// first page loads without a Vite dev server.
    pub builds_front_end: bool,
}

/// Every template, the default first. Packages are fixed here; nothing from the command line
/// reaches Composer but the project's name.
pub const TEMPLATES: &[Template] = &[
    Template {
        id: "laravel",
        label: "Laravel",
        package: Some("laravel/laravel"),
        laravel: true,
        builds_front_end: false,
    },
    Template {
        id: "react",
        label: "Laravel with React (starter kit)",
        package: Some("laravel/react-starter-kit"),
        laravel: true,
        builds_front_end: true,
    },
    Template {
        id: "vue",
        label: "Laravel with Vue (starter kit)",
        package: Some("laravel/vue-starter-kit"),
        laravel: true,
        builds_front_end: true,
    },
    Template {
        id: "livewire",
        label: "Laravel with Livewire (starter kit)",
        package: Some("laravel/livewire-starter-kit"),
        laravel: true,
        builds_front_end: true,
    },
    Template {
        id: "symfony",
        label: "Symfony",
        package: Some("symfony/skeleton"),
        laravel: false,
        builds_front_end: false,
    },
    Template {
        id: "bedrock",
        label: "WordPress (Bedrock)",
        package: Some("roots/bedrock"),
        laravel: false,
        builds_front_end: false,
    },
    Template {
        id: "empty",
        label: "An empty PHP site",
        package: None,
        laravel: false,
        builds_front_end: false,
    },
];

/// The template `id` names.
pub fn template(id: &str) -> Result<&'static Template, String> {
    TEMPLATES.iter().find(|t| t.id == id).ok_or_else(|| {
        let ids: Vec<&str> = TEMPLATES.iter().map(|t| t.id).collect();
        format!("there is no template `{id}`; Kiln has {}", ids.join(", "))
    })
}

/// `create-project` arguments for `package` into `name`.
fn create_args(package: &str, name: &str) -> Vec<OsString> {
    [
        "create-project",
        package,
        name,
        "--prefer-dist",
        "--no-interaction",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

/// The page an empty site starts with.
const EMPTY_INDEX: &str = "<?php\n// Served by Kiln. Replace this with your site.\n?>\n<!doctype html>\n<meta charset=\"utf-8\">\n<title><?= htmlspecialchars(basename(dirname(__DIR__))) ?></title>\n<h1>It works.</h1>\n<p>PHP <?= PHP_VERSION ?>, served by Kiln from <code><?= htmlspecialchars(__DIR__) ?></code>.</p>\n";

/// Builds a starter kit's front end once with Kiln's Node: `npm install`, `npm run build`.
/// Without Kiln's Node, or on failure, it says what to run instead: the project itself is
/// made either way.
fn build_front_end(project: &Path) {
    let manual = || {
        say!(
            "Its front end isn't built yet: run `npm install` and `npm run build` (or `npm run dev`) in {}.",
            project.display()
        );
    };
    let Some(local) = kiln_state::default_local_dir() else {
        return manual();
    };
    let nodes = kiln_node::installed(&local.join(kiln_runtime::node::NODE_DIR));
    let default_spec = crate::web::open_store()
        .ok()
        .and_then(|s| s.setting(kiln_runtime::node::DEFAULT_KEY).ok().flatten());
    let Some(node) =
        kiln_runtime::node::node_for_dir(&nodes, default_spec.as_deref(), project).node
    else {
        say!("Kiln has no Node yet (`kiln node install lts`).");
        return manual();
    };
    let exe = node.dir.join(if cfg!(windows) {
        "node.exe"
    } else {
        "bin/node"
    });
    let npm = if cfg!(windows) {
        node.dir.join("node_modules/npm/bin/npm-cli.js")
    } else {
        node.dir.join("lib/node_modules/npm/bin/npm-cli.js")
    };
    let Ok(path) = path_with(exe.parent().unwrap_or(&node.dir)) else {
        return manual();
    };
    for args in [&["install"][..], &["run", "build"][..]] {
        crate::out::step(&format!(
            "Running npm {} with Node {}",
            args.join(" "),
            node.version
        ));
        let mut command = std::process::Command::new(&exe);
        command
            .arg(&npm)
            .args(args)
            .current_dir(project)
            .env("PATH", &path);
        match crate::out::run_shown(&mut command) {
            Ok(status) if status.success() => {}
            Ok(status) => {
                say!(
                    "npm {} failed (exit code {}).",
                    args.join(" "),
                    status.code().unwrap_or(1)
                );
                return manual();
            }
            Err(e) => {
                say!("cannot run npm: {e}");
                return manual();
            }
        }
    }
    say!("Built its front end with Node {}.", node.version);
}

/// The site name `name` gives, which is also the folder's name: a single lowercase label.
fn project_name(name: &str, tld: &Tld) -> Result<String, String> {
    let host = crate::web::host_name_under(name, tld)?;
    let label = host
        .as_str()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_owned();
    if host.as_str().split('.').count() != 2 {
        return Err(format!(
            "`{name}` would be a subdomain; a project name is one label, such as `shop`"
        ));
    }
    Ok(label)
}

/// The `kiln.toml` `kiln new --php` writes.
fn manifest_for(php: &str) -> String {
    manifest::render(
        &manifest::Manifest {
            php: Some(php.to_owned()),
            ..manifest::Manifest::default()
        },
        "Kiln project settings (ADR 0027), written by `kiln new`.",
    )
}

/// The PHP to create the project with: the one `spec` names, else the default.
fn chosen_php(spec: Option<&str>) -> Result<PhpRecord, String> {
    let store = crate::web::open_store()?;
    let installed = kiln_runtime::installed(&store).map_err(|e| e.to_string())?;
    if installed.is_empty() {
        return Err("no PHP installed by Kiln yet; run `kiln php install 8.4` first".into());
    }
    match spec {
        Some(spec) => kiln_runtime::pick(&installed, spec)
            .cloned()
            .ok_or_else(|| {
                format!("PHP {spec} isn't installed by Kiln (`kiln php install {spec}`)")
            }),
        None => kiln_runtime::default_php(&store, &installed)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no PHP installed by Kiln yet".to_owned()),
    }
}

/// `PATH` with `dir` first.
fn path_with(dir: &Path) -> Result<OsString, String> {
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&rest)))
        .map_err(|e| e.to_string())
}

/// The instance `--db` names: an instance's own name, or a kind (`pgsql` for `postgres`),
/// meaning its first running instance ([ADR 0045] §5).
///
/// [ADR 0045]: ../../../docs/adr/0045-env-wiring.md
fn db_instance(spec: &str) -> Result<String, String> {
    let services = crate::web::open_store()?
        .services()
        .map_err(|e| e.to_string())?;
    if services.iter().any(|s| s.name == spec) {
        return Ok(spec.to_owned());
    }
    let kind = if spec == "pgsql" { "postgres" } else { spec };
    if !matches!(kind, "mysql" | "mariadb" | "postgres") {
        return Err(format!(
            "--db takes mysql, mariadb, pgsql or an instance's name; there is no instance `{spec}`"
        ));
    }
    services
        .iter()
        .find(|s| s.kind == kind && s.enabled)
        .map(|s| s.name.clone())
        .ok_or_else(|| format!("no running {kind} instance: `kiln service add {kind}` first"))
}

/// `kiln new <name> [--template <id>] [--path <folder>] [--php <version>] [--secure] [--db <database>]`.
pub fn new(
    name: &str,
    template_id: &str,
    parent: Option<PathBuf>,
    php: Option<&str>,
    secure: bool,
    db: Option<&str>,
) -> Result<(), String> {
    let template = template(template_id)?;
    if db.is_some() && !template.laravel {
        return Err(format!(
            "--db is for Laravel projects; {} sets up its database itself",
            template.label
        ));
    }
    let db = db.map(db_instance).transpose()?;
    let tld = crate::web::current_tld()?;
    let name = project_name(name, &tld)?;
    let parent = match parent {
        Some(parent) => parent,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let parent = std::fs::canonicalize(&parent)
        .map_err(|e| format!("cannot use {}: {e}", parent.display()))?;
    let parent = crate::web::strip_verbatim(parent);
    let project = parent.join(&name);
    if project.exists() {
        return Err(format!(
            "{} already exists; pick another name or folder",
            project.display()
        ));
    }
    let record = chosen_php(php)?;
    if let Some(package) = template.package {
        create_with_composer(package, &name, &parent, &project, &record)?;
    } else {
        let public = project.join("public");
        std::fs::create_dir_all(&public)
            .map_err(|e| format!("cannot create {}: {e}", public.display()))?;
        std::fs::write(public.join("index.php"), EMPTY_INDEX)
            .map_err(|e| format!("cannot write {}: {e}", public.join("index.php").display()))?;
        say!(
            "Created {} with a first page in public/index.php.",
            project.display()
        );
    }

    if let Some(spec) = php {
        let toml = project.join(manifest::FILE);
        std::fs::write(&toml, manifest_for(spec))
            .map_err(|e| format!("cannot write {}: {e}", toml.display()))?;
        say!("Wrote {}: PHP {spec} for this project.", toml.display());
    }

    serve(&name, &tld, &project, secure)?;
    if let Some(instance) = db {
        wire_database(&instance, &project, &record)?;
    }
    if template.builds_front_end {
        build_front_end(&project);
    } else if template.id == "laravel" {
        say!(
            "Next: `cd {}`, then `npm install` and `npm run dev` for its front end.",
            project.display()
        );
    }
    Ok(())
}

/// Runs `composer create-project <package> <name>` in `parent` with `record`'s PHP, installing
/// Composer first if Kiln has none yet.
fn create_with_composer(
    package: &str,
    name: &str,
    parent: &Path,
    project: &Path,
    record: &PhpRecord,
) -> Result<(), String> {
    let local =
        kiln_state::default_local_dir().ok_or("cannot determine Kiln's local data directory")?;
    if !local.join(COMPOSER_PHAR).is_file() {
        say!("Composer isn't installed yet; installing it first.");
        crate::php::composer_install()?;
    }

    say!(
        "Creating {} with PHP {} and Composer ({package})...",
        project.display(),
        record.version
    );
    let here = DirPhp {
        site: None,
        php: SitePhp {
            php: Some(record.clone()),
            pinned: None,
            from_project: false,
            fell_back: false,
        },
    };
    let mut command = kiln_runtime::shim::command(
        Tool::Composer,
        &here,
        &local.join(COMPOSER_PHAR),
        create_args(package, name),
    )?;
    command
        .current_dir(parent)
        .env("PATH", path_with(&record.dir)?);
    #[cfg(windows)]
    {
        // Ctrl+C goes to Composer (same console); Kiln waits for it to stop.
        let _ = kiln_sys::win::survive_ctrl_c();
    }
    // Its output is shown; with --json, as notes (ADR 0053 §1).
    let status =
        crate::out::run_shown(&mut command).map_err(|e| format!("cannot run Composer: {e}"))?;
    if !status.success() {
        let left = if project.exists() {
            format!(
                "; {} may be incomplete: delete it before trying again",
                project.display()
            )
        } else {
            String::new()
        };
        return Err(format!(
            "Composer failed (exit code {}){left}",
            status.code().unwrap_or(1)
        ));
    }
    Ok(())
}

/// Serves the new project as `<name>.<tld>`: linked, or already served from a parked folder.
fn serve(name: &str, tld: &Tld, project: &Path, secure: bool) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let served = kiln_runtime::sites(&store)
        .map_err(|e| e.to_string())?
        .sites
        .iter()
        .any(|s| s.alias_of.is_none() && s.project == project);
    drop(store);
    if !served {
        return crate::web::link(Some(name.to_owned()), Some(project.to_path_buf()), secure);
    }
    say!("{name}.{tld} already serves it (its folder is parked).");
    if secure {
        crate::web::set_secure(name, true)?;
    }
    if let Some(url) =
        crate::web::site_urls(&crate::web::open_store()?).get(&format!("{name}.{tld}"))
    {
        say!("URL: {url}");
    }
    Ok(())
}

/// Points the new project at `instance` and runs its migrations there.
fn wire_database(instance: &str, project: &Path, record: &PhpRecord) -> Result<(), String> {
    crate::env::use_instance(
        instance,
        Some(project.to_path_buf()),
        None,
        false,
        false,
        false,
        true,
    )?;
    say!("Running its migrations on {instance}...");
    let mut migrate = std::process::Command::new(record.dir.join(kiln_php::PHP_EXE));
    migrate
        .args(["artisan", "migrate", "--force"])
        .current_dir(project);
    let status = crate::out::run_shown(&mut migrate)
        .map_err(|e| format!("cannot run php artisan migrate: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "the migrations failed (exit code {}); the project is created and wired, so fix the cause and run `php artisan migrate`",
            status.code().unwrap_or(1)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_name_is_one_site_label() {
        assert_eq!(project_name("Shop", &Tld::default()).unwrap(), "shop");
        assert_eq!(project_name("shop.test", &Tld::default()).unwrap(), "shop");
        assert_eq!(project_name("my-app2", &Tld::default()).unwrap(), "my-app2");
        let internal = Tld::new("internal").unwrap();
        assert_eq!(project_name("shop.internal", &internal).unwrap(), "shop");
        assert!(project_name("shop.test", &internal).is_err());
        assert!(project_name("api.shop", &Tld::default()).is_err());
        assert!(project_name("bad_name", &Tld::default()).is_err());
        assert!(project_name("", &Tld::default()).is_err());
    }

    #[test]
    fn composer_creates_the_skeleton_without_prompts() {
        let args: Vec<String> = create_args("laravel/laravel", "shop")
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "create-project",
                "laravel/laravel",
                "shop",
                "--prefer-dist",
                "--no-interaction"
            ]
        );
    }

    #[test]
    fn templates_name_fixed_packages_and_the_default_comes_first() {
        assert_eq!(TEMPLATES[0].id, "laravel");
        assert_eq!(
            template("react").unwrap().package,
            Some("laravel/react-starter-kit")
        );
        assert!(template("react").unwrap().builds_front_end);
        assert_eq!(template("empty").unwrap().package, None);
        assert!(!template("symfony").unwrap().laravel);
        assert!(template("--evil").is_err());
        let mut ids: Vec<&str> = TEMPLATES.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), TEMPLATES.len());
    }

    #[test]
    fn php_choice_is_kept_in_kiln_toml() {
        let text = manifest_for("8.3");
        assert_eq!(manifest::parse(&text).unwrap().php.as_deref(), Some("8.3"));
    }

    #[test]
    fn the_php_folder_goes_first_on_path() {
        let dir = PathBuf::from(if cfg!(windows) {
            r"C:\php\8.4"
        } else {
            "/php/8.4"
        });
        let path = path_with(&dir).unwrap();
        assert_eq!(std::env::split_paths(&path).next(), Some(dir));
    }
}
