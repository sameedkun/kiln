//! `kiln new <name>` ([ADR 0031]): a Laravel project made with Kiln's PHP and Composer, served
//! at once.
//!
//! [ADR 0031]: ../../../docs/adr/0031-kiln-new-laravel-projects.md

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use kiln_core::Tld;
use kiln_runtime::shim::{COMPOSER_PHAR, Tool};
use kiln_runtime::{DirPhp, SitePhp};
use kiln_sites::manifest;
use kiln_state::PhpRecord;

/// The Composer package `kiln new` creates projects from.
const SKELETON: &str = "laravel/laravel";

/// `create-project` arguments for `name`.
fn create_args(name: &str) -> Vec<OsString> {
    [
        "create-project",
        SKELETON,
        name,
        "--prefer-dist",
        "--no-interaction",
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
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

/// `kiln new <name> [--path <folder>] [--php <version>] [--secure] [--db <database>]`.
pub fn new(
    name: &str,
    parent: Option<PathBuf>,
    php: Option<&str>,
    secure: bool,
    db: Option<&str>,
) -> Result<(), String> {
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
    let local =
        kiln_state::default_local_dir().ok_or("cannot determine Kiln's local data directory")?;
    if !local.join(COMPOSER_PHAR).is_file() {
        println!("Composer isn't installed yet; installing it first.");
        crate::php::composer_install()?;
    }

    println!(
        "Creating {} with PHP {} and Composer ({SKELETON})...",
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
        create_args(&name),
    )?;
    command
        .current_dir(&parent)
        .env("PATH", path_with(&record.dir)?);
    #[cfg(windows)]
    {
        // Ctrl+C goes to Composer (same console); Kiln waits for it to stop.
        let _ = kiln_sys::win::survive_ctrl_c();
    }
    let status = command
        .status()
        .map_err(|e| format!("cannot run Composer: {e}"))?;
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

    if let Some(spec) = php {
        let toml = project.join(manifest::FILE);
        std::fs::write(&toml, manifest_for(spec))
            .map_err(|e| format!("cannot write {}: {e}", toml.display()))?;
        println!("Wrote {}: PHP {spec} for this project.", toml.display());
    }

    serve(&name, &tld, &project, secure)?;
    if let Some(instance) = db {
        wire_database(&instance, &project, &record)?;
    }
    println!(
        "Next: `cd {}`, then `npm install` and `npm run dev` for its front end.",
        project.display()
    );
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
    println!("{name}.{tld} already serves it (its folder is parked).");
    if secure {
        crate::web::set_secure(name, true)?;
    }
    if let Some(url) =
        crate::web::site_urls(&crate::web::open_store()?).get(&format!("{name}.{tld}"))
    {
        println!("URL: {url}");
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
    println!("Running its migrations on {instance}...");
    let status = std::process::Command::new(record.dir.join(kiln_php::PHP_EXE))
        .args(["artisan", "migrate", "--force"])
        .current_dir(project)
        .status()
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
        let args: Vec<String> = create_args("shop")
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
