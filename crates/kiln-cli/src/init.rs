//! `kiln init [<folder>]` ([ADR 0080]): a project cloned from Git set up and served in one
//! step. Each step is skipped when the folder shows it done, so running it again picks up
//! where it stopped.
//!
//! [ADR 0080]: ../../../docs/adr/0080-setting-up-a-cloned-project.md

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use kiln_sites::manifest;
use kiln_state::PhpRecord;

/// What `kiln init` was asked for.
#[allow(clippy::struct_excessive_bools)] // one per flag
#[derive(Debug, Default)]
pub struct Options {
    pub dir: Option<PathBuf>,
    pub db: Option<String>,
    pub php: Option<String>,
    pub secure: bool,
    pub seed: bool,
    pub no_build: bool,
    pub yes: bool,
}

/// A JavaScript lock file, which decides the package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Lock {
    #[default]
    None,
    Npm,
    /// pnpm, Yarn or Bun: Kiln runs npm only, so it says what to run.
    Other(&'static str),
}

/// What the folder holds, read once before the plan.
#[allow(clippy::struct_excessive_bools)] // one per fact
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Found {
    served: bool,
    manifest: bool,
    composer: bool,
    vendor: bool,
    artisan: bool,
    env: bool,
    env_example: bool,
    /// `APP_KEY` in `.env` has a value.
    app_key: bool,
    /// The SQLite file the `.env` (or, without one, `.env.example`) names, if it uses SQLite
    /// and the file isn't there.
    sqlite_missing: Option<PathBuf>,
    /// `kiln.toml` lists services, and whether one is a database.
    services: bool,
    services_database: bool,
    /// `storage/app/public` is there and `public/storage` isn't.
    storage_link_missing: bool,
    package_json: bool,
    node_modules: bool,
    lock: Lock,
    build_script: bool,
}

/// One step of the setup, in the order run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Link { secure: bool },
    WritePhp(String),
    ComposerInstall,
    CopyEnv,
    KeyGenerate,
    MakeSqlite(PathBuf),
    ServicesUp,
    UseDb(String),
    Migrate { seed: bool },
    StorageLink,
    Npm { ci: bool, build: bool },
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link { secure: false } => write!(f, "serve it: kiln link"),
            Self::Link { secure: true } => write!(f, "serve it over HTTPS: kiln link --secure"),
            Self::WritePhp(v) => write!(f, "write PHP {v} to kiln.toml"),
            Self::ComposerInstall => write!(f, "composer install --no-interaction"),
            Self::CopyEnv => write!(f, "copy .env.example to .env"),
            Self::KeyGenerate => write!(f, "php artisan key:generate --force"),
            Self::MakeSqlite(path) => write!(f, "create the SQLite database {}", path.display()),
            Self::ServicesUp => write!(f, "start the services kiln.toml lists, wired into .env"),
            Self::UseDb(db) => write!(f, "point .env at {db}: kiln env use {db}"),
            Self::Migrate { seed: false } => write!(f, "php artisan migrate --force"),
            Self::Migrate { seed: true } => write!(f, "php artisan migrate --force --seed"),
            Self::StorageLink => write!(f, "php artisan storage:link"),
            Self::Npm { ci, build } => {
                write!(f, "npm {}", if *ci { "ci" } else { "install" })?;
                if *build {
                    write!(f, ", then npm run build")?;
                }
                Ok(())
            }
        }
    }
}

/// The steps for what's `found`, and notes on what's left to the user.
fn plan(found: &Found, options: &Options) -> (Vec<Step>, Vec<String>) {
    let mut steps = Vec::new();
    let mut notes = Vec::new();
    if let Some(php) = &options.php {
        steps.push(Step::WritePhp(php.clone()));
    }
    if !found.served {
        steps.push(Step::Link {
            secure: options.secure,
        });
    }
    if found.composer && !found.vendor {
        steps.push(Step::ComposerInstall);
    }
    let laravel = found.artisan;
    let copies_env = laravel && !found.env && found.env_example;
    if copies_env {
        steps.push(Step::CopyEnv);
    }
    if laravel && (copies_env || (found.env && !found.app_key)) {
        steps.push(Step::KeyGenerate);
    }
    let mut wired = false;
    if let Some(db) = &options.db {
        steps.push(Step::UseDb(db.clone()));
        wired = true;
    } else if let Some(path) = found.sqlite_missing.as_ref().filter(|_| laravel) {
        steps.push(Step::MakeSqlite(path.clone()));
        wired = true;
    }
    if found.services {
        steps.push(Step::ServicesUp);
        wired |= found.services_database;
    }
    if laravel {
        if wired {
            steps.push(Step::Migrate { seed: options.seed });
        } else {
            notes.push(
                "Its database isn't one this run set up, so the migrations aren't run: `php artisan migrate` once .env points where you want (or `--db mysql`)."
                    .to_owned(),
            );
        }
        if found.storage_link_missing {
            steps.push(Step::StorageLink);
        }
    }
    if found.package_json && !found.node_modules && !options.no_build {
        match found.lock {
            Lock::Other(tool) => notes.push(format!(
                "Its front end uses {tool} (its lock file): run `{tool} install` and `{tool} run build` yourself."
            )),
            lock => steps.push(Step::Npm {
                ci: lock == Lock::Npm,
                build: found.build_script,
            }),
        }
    }
    (steps, notes)
}

/// The value of `key` in a dotenv text: the last line setting it, unquoted.
fn env_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("export ").unwrap_or(line);
            let (k, v) = rest.split_once('=')?;
            (k.trim() == key).then(|| {
                let v = v.trim();
                v.strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                    .unwrap_or(v)
                    .to_owned()
            })
        })
        .next_back()
}

/// The SQLite file a dotenv text names, if it uses SQLite: `DB_DATABASE` when it's an
/// absolute path, else Laravel's default, `database/database.sqlite`.
fn sqlite_file(dir: &Path, env: &str) -> Option<PathBuf> {
    if env_value(env, "DB_CONNECTION")? != "sqlite" {
        return None;
    }
    match env_value(env, "DB_DATABASE").filter(|v| !v.is_empty()) {
        Some(path) if Path::new(&path).is_absolute() => Some(PathBuf::from(path)),
        // A relative name is relative to wherever PHP runs: left alone.
        Some(_) => None,
        None => Some(dir.join("database").join("database.sqlite")),
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Reads the folder.
fn find(dir: &Path, served: bool) -> Result<Found, String> {
    let env_text = read(&dir.join(".env"));
    let env = dir.join(".env").is_file();
    let env_example = dir.join(".env.example").is_file();
    let source = if env {
        env_text.clone()
    } else {
        read(&dir.join(".env.example"))
    };
    let wants = manifest::read(dir)?.map(|m| m.services).unwrap_or_default();
    let package = read(&dir.join("package.json"));
    let build_script = serde_json::from_str::<serde_json::Value>(&package)
        .is_ok_and(|p| p["scripts"]["build"].is_string());
    let lock = if dir.join("package-lock.json").is_file() {
        Lock::Npm
    } else if dir.join("pnpm-lock.yaml").is_file() {
        Lock::Other("pnpm")
    } else if dir.join("yarn.lock").is_file() {
        Lock::Other("yarn")
    } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() {
        Lock::Other("bun")
    } else {
        Lock::None
    };
    Ok(Found {
        served,
        manifest: dir.join(manifest::FILE).is_file(),
        composer: dir.join("composer.json").is_file(),
        vendor: dir.join("vendor").join("autoload.php").is_file(),
        artisan: dir.join("artisan").is_file(),
        env,
        env_example,
        app_key: env_value(&env_text, "APP_KEY").is_some_and(|v| !v.is_empty()),
        sqlite_missing: sqlite_file(dir, &source).filter(|p| !p.exists()),
        services: !wants.is_empty(),
        services_database: wants.iter().any(|w| {
            matches!(
                w.kind.as_str(),
                "mysql" | "mariadb" | "postgres" | "pgsql" | "postgresql"
            )
        }),
        storage_link_missing: dir.join("storage").join("app").join("public").is_dir()
            && std::fs::symlink_metadata(dir.join("public").join("storage")).is_err(),
        package_json: !package.is_empty(),
        node_modules: dir.join("node_modules").is_dir(),
        lock,
        build_script,
    })
}

/// Whether a site of Kiln's serves `dir`.
fn served(dir: &Path) -> Result<bool, String> {
    let store = crate::web::open_store()?;
    Ok(kiln_runtime::sites(&store)
        .map_err(|e| e.to_string())?
        .sites
        .iter()
        .any(|s| s.alias_of.is_none() && s.project == dir))
}

/// The PHP Kiln picks for `dir`, once it's served.
fn php_for(dir: &Path) -> Result<PhpRecord, String> {
    let store = crate::web::open_store()?;
    kiln_runtime::php_for_dir(&store, dir)
        .map_err(|e| e.to_string())?
        .php
        .php
        .ok_or_else(|| "no PHP installed by Kiln yet; run `kiln php install 8.4` first".to_owned())
}

/// Runs `php artisan <args> --no-interaction` in `dir`.
fn artisan(record: &PhpRecord, dir: &Path, args: &[&str]) -> Result<(), String> {
    crate::out::step(&format!("php artisan {}", args.join(" ")));
    let mut command = std::process::Command::new(record.dir.join(kiln_php::PHP_EXE));
    command
        .arg("artisan")
        .args(args)
        .arg("--no-interaction")
        .current_dir(dir)
        .env("PATH", crate::new::path_with(&record.dir)?);
    let status =
        crate::out::run_shown(&mut command).map_err(|e| format!("cannot run php artisan: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "php artisan {} failed (exit code {})",
            args.join(" "),
            status.code().unwrap_or(1)
        ))
    }
}

/// `kiln init`.
pub fn init(options: &Options) -> Result<(), String> {
    let dir = match &options.dir {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let dir =
        std::fs::canonicalize(&dir).map_err(|e| format!("cannot use {}: {e}", dir.display()))?;
    let dir = crate::web::strip_verbatim(dir);
    let is_project = ["composer.json", "artisan", "index.php"]
        .iter()
        .any(|f| dir.join(f).is_file())
        || dir.join("public").is_dir();
    if !is_project {
        return Err(format!(
            "{} doesn't look like a PHP project (no composer.json, artisan, index.php or public folder)",
            dir.display()
        ));
    }
    let found = find(&dir, served(&dir)?)?;
    if options.php.is_some() && found.manifest {
        return Err(format!(
            "{} already has a {}: set `php` there instead of --php",
            dir.display(),
            manifest::FILE
        ));
    }
    let db = options
        .db
        .as_deref()
        .map(crate::new::db_instance)
        .transpose()?;
    if db.is_some() && !found.artisan {
        return Err("--db is for Laravel projects (it wires .env and runs the migrations)".into());
    }
    if db.is_some() && !found.env && !found.env_example {
        return Err(format!(
            "{} has no .env or .env.example to point at the database",
            dir.display()
        ));
    }
    let options = Options {
        db,
        dir: Some(dir.clone()),
        php: options.php.clone(),
        ..*options
    };
    let (steps, notes) = plan(&found, &options);
    if steps.is_empty() {
        say!("{} is set up: nothing to do.", dir.display());
        for note in &notes {
            say!("{note}");
        }
        return Ok(());
    }
    say!("To set up {}:", dir.display());
    for (n, step) in steps.iter().enumerate() {
        say!("  {}. {step}", n + 1);
    }
    say!("Composer, npm and artisan run the project's own scripts.");
    if !options.yes && !crate::apply::confirm("Set it up?") {
        say!("Nothing was changed.");
        return Ok(());
    }
    run(&dir, &steps)?;
    for note in &notes {
        say!("{note}");
    }
    let store = crate::web::open_store()?;
    let url = kiln_runtime::sites(&store)
        .map_err(|e| e.to_string())?
        .sites
        .iter()
        .find(|s| s.alias_of.is_none() && s.project == dir)
        .and_then(|s| crate::web::site_urls(&store).get(s.host.as_str()).cloned());
    match url {
        Some(url) => say!("Set up: {url}"),
        None => say!("Set up."),
    }
    Ok(())
}

/// Runs `steps` in order. Linking, Composer, the key, the services and the migrations stop
/// it; the storage link and the front end leave a note.
fn run(dir: &Path, steps: &[Step]) -> Result<(), String> {
    let mut record: Option<PhpRecord> = None;
    let mut php = || -> Result<PhpRecord, String> {
        if let Some(record) = &record {
            return Ok(record.clone());
        }
        let found = php_for(dir)?;
        record = Some(found.clone());
        Ok(found)
    };
    for step in steps {
        match step {
            Step::WritePhp(version) => {
                let toml = dir.join(manifest::FILE);
                std::fs::write(&toml, crate::new::manifest_for(version))
                    .map_err(|e| format!("cannot write {}: {e}", toml.display()))?;
                say!("Wrote {}: PHP {version} for this project.", toml.display());
            }
            Step::Link { secure } => {
                crate::web::link(None, Some(dir.to_path_buf()), *secure)?;
            }
            Step::ComposerInstall => {
                let record = php()?;
                say!(
                    "Installing its packages with PHP {} and Composer...",
                    record.version
                );
                let args: Vec<OsString> = ["install", "--no-interaction"]
                    .into_iter()
                    .map(OsString::from)
                    .collect();
                let status = crate::new::composer(args, dir, &record)?;
                if !status.success() {
                    return Err(format!(
                        "composer install failed (exit code {}); fix the cause and run `kiln init` again",
                        status.code().unwrap_or(1)
                    ));
                }
            }
            Step::CopyEnv => {
                let to = dir.join(".env");
                if to.exists() {
                    continue;
                }
                std::fs::copy(dir.join(".env.example"), &to)
                    .map_err(|e| format!("cannot write {}: {e}", to.display()))?;
                say!("Wrote .env from .env.example.");
            }
            Step::KeyGenerate => artisan(&php()?, dir, &["key:generate", "--force"])?,
            Step::MakeSqlite(path) => make_sqlite(path)?,
            Step::ServicesUp => {
                let wants = manifest::read(dir)?.map(|m| m.services).unwrap_or_default();
                crate::services_up::up_in(dir, &wants, true)?;
            }
            Step::UseDb(instance) => {
                crate::env::use_instance(
                    instance,
                    Some(dir.to_path_buf()),
                    None,
                    false,
                    false,
                    false,
                    true,
                )?;
            }
            Step::Migrate { seed } => {
                let args: &[&str] = if *seed {
                    &["migrate", "--force", "--seed"]
                } else {
                    &["migrate", "--force"]
                };
                artisan(&php()?, dir, args).map_err(|e| {
                    format!("{e}; fix the cause and run `kiln init` again (what's done is kept)")
                })?;
            }
            Step::StorageLink => {
                if let Err(e) = artisan(&php()?, dir, &["storage:link"]) {
                    say!("{e}: run `php artisan storage:link` later.");
                }
            }
            Step::Npm { ci, build } => front_end(dir, *ci, *build),
        }
    }
    Ok(())
}

/// An empty SQLite database file, as Laravel's installer makes.
fn make_sqlite(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    say!("Created the SQLite database {}.", path.display());
    Ok(())
}

/// npm's install (`ci` with a lock file) and build with Kiln's Node; a note if it can't.
fn front_end(dir: &Path, ci: bool, build: bool) {
    let install: &[&str] = if ci { &["ci"] } else { &["install"] };
    let mut steps = vec![install];
    if build {
        steps.push(&["run", "build"]);
    }
    if !crate::new::npm(dir, &steps) {
        say!(
            "Its front end isn't set up: run `npm {}`{} in {}.",
            install[0],
            if build { " and `npm run build`" } else { "" },
            dir.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laravel() -> Found {
        Found {
            composer: true,
            artisan: true,
            env_example: true,
            package_json: true,
            lock: Lock::Npm,
            build_script: true,
            storage_link_missing: true,
            ..Found::default()
        }
    }

    #[test]
    fn a_fresh_clone_gets_every_step_in_order() {
        let (steps, notes) = plan(
            &laravel(),
            &Options {
                db: Some("mysql-8.4".into()),
                ..Options::default()
            },
        );
        assert_eq!(
            steps,
            vec![
                Step::Link { secure: false },
                Step::ComposerInstall,
                Step::CopyEnv,
                Step::KeyGenerate,
                Step::UseDb("mysql-8.4".into()),
                Step::Migrate { seed: false },
                Step::StorageLink,
                Step::Npm {
                    ci: true,
                    build: true
                },
            ]
        );
        assert_eq!(notes, Vec::<String>::new());
    }

    #[test]
    fn a_set_up_project_has_nothing_to_do() {
        let found = Found {
            served: true,
            vendor: true,
            env: true,
            app_key: true,
            storage_link_missing: false,
            node_modules: true,
            ..laravel()
        };
        let (steps, notes) = plan(&found, &Options::default());
        assert!(steps.is_empty(), "{steps:?}");
        // The database wasn't set up here, so the migrations are left to the user.
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn migrations_run_only_against_a_database_this_run_set_up() {
        // A .env that came with the clone: never migrated against.
        let found = Found {
            env: true,
            app_key: true,
            ..laravel()
        };
        let (steps, _) = plan(&found, &Options::default());
        assert!(!steps.iter().any(|s| matches!(s, Step::Migrate { .. })));
        assert!(!steps.contains(&Step::KeyGenerate));
        // Its SQLite file made now: migrated, seeded when asked.
        let found = Found {
            sqlite_missing: Some(PathBuf::from("/p/database/database.sqlite")),
            ..laravel()
        };
        let (steps, _) = plan(
            &found,
            &Options {
                seed: true,
                ..Options::default()
            },
        );
        assert!(steps.contains(&Step::Migrate { seed: true }));
        // kiln.toml's services include a database.
        let found = Found {
            services: true,
            services_database: true,
            ..laravel()
        };
        assert!(
            plan(&found, &Options::default())
                .0
                .contains(&Step::Migrate { seed: false })
        );
        // ... only a cache: not migrated.
        let found = Found {
            services: true,
            ..laravel()
        };
        assert!(
            !plan(&found, &Options::default())
                .0
                .iter()
                .any(|s| matches!(s, Step::Migrate { .. }))
        );
    }

    #[test]
    fn an_empty_key_is_generated_and_a_set_one_kept() {
        let found = Found {
            env: true,
            app_key: false,
            ..laravel()
        };
        assert!(
            plan(&found, &Options::default())
                .0
                .contains(&Step::KeyGenerate)
        );
    }

    #[test]
    fn npm_doesnt_override_another_managers_lock_file() {
        let found = Found {
            lock: Lock::Other("pnpm"),
            ..laravel()
        };
        let (steps, notes) = plan(&found, &Options::default());
        assert!(!steps.iter().any(|s| matches!(s, Step::Npm { .. })));
        assert!(notes.iter().any(|n| n.contains("pnpm install")));
        let found = Found {
            lock: Lock::None,
            build_script: false,
            ..laravel()
        };
        assert!(plan(&found, &Options::default()).0.contains(&Step::Npm {
            ci: false,
            build: false
        }));
        assert!(
            !plan(
                &laravel(),
                &Options {
                    no_build: true,
                    ..Options::default()
                }
            )
            .0
            .iter()
            .any(|s| matches!(s, Step::Npm { .. }))
        );
    }

    #[test]
    fn a_plain_php_project_gets_no_artisan_steps() {
        let found = Found {
            composer: true,
            ..Found::default()
        };
        assert_eq!(
            plan(&found, &Options::default()).0,
            vec![Step::Link { secure: false }, Step::ComposerInstall]
        );
    }

    #[test]
    fn dotenv_values_are_read_as_laravel_reads_them() {
        let env = "APP_KEY=\nDB_CONNECTION=sqlite\n# DB_DATABASE=x\nexport APP_NAME=\"My App\"\nAPP_KEY='base64:abc'\n";
        assert_eq!(env_value(env, "APP_KEY").as_deref(), Some("base64:abc"));
        assert_eq!(env_value(env, "APP_NAME").as_deref(), Some("My App"));
        assert_eq!(env_value(env, "DB_DATABASE"), None);
        let dir = Path::new("/p");
        assert_eq!(
            sqlite_file(dir, env),
            Some(dir.join("database").join("database.sqlite"))
        );
        assert_eq!(sqlite_file(dir, "DB_CONNECTION=mysql\n"), None);
        assert_eq!(
            sqlite_file(dir, "DB_CONNECTION=sqlite\nDB_DATABASE=db.sqlite\n"),
            None
        );
    }
}
