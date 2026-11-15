//! `kiln doctor <site>` ([ADR 0081]): one site's project, `.env`, packages, database and
//! answer, as pure rules over [`SiteFacts`] the CLI gathers.
//!
//! Findings name `.env` keys, never their values (only `APP_URL`, which isn't a secret).
//!
//! [ADR 0081]: ../../../docs/adr/0081-site-doctor.md

use std::path::{Path, PathBuf};

use crate::{Area, Finding, Report, Status, finding, with_fix};

/// What the site is, from its driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    Laravel,
    WordPress,
    #[default]
    Other,
    /// Files only: no PHP checks.
    Static,
    /// A name Kiln forwards to an address: only its answer is checked.
    Proxy,
}

/// One of Kiln's service instances.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstanceFacts {
    pub name: String,
    /// `mysql`, `mariadb`, `postgres`, `redis`, `mailpit`, …
    pub kind: String,
    pub ports: Vec<u16>,
    /// Kiln is meant to keep it running.
    pub enabled: bool,
    /// Its databases, when it runs and could list them.
    pub databases: Option<Vec<String>>,
}

/// What one `GET /` to the site gave.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Answer {
    /// Not a site of Kiln's (a folder was named).
    #[default]
    NotServed,
    ServerStopped,
    Got {
        code: u16,
        location: Option<String>,
    },
    Failed(String),
}

/// Everything the site rules look at.
#[allow(clippy::struct_excessive_bools)] // each is one thing seen in the folder
#[derive(Debug, Clone, Default)]
pub struct SiteFacts {
    /// The site's name, or the folder when Kiln doesn't serve it.
    pub name: String,
    pub folder: PathBuf,
    pub kind: Kind,
    /// Where Kiln serves it.
    pub url: Option<String>,
    /// The PHP version Kiln picks for the folder.
    pub php: Option<String>,
    pub composer_json: Option<String>,
    /// Extensions that PHP loads (normalised, `php -m`), if they could be listed.
    pub loaded_ext: Option<Vec<String>>,
    /// Extensions that build ships and Kiln can turn on.
    pub shipped_ext: Vec<String>,
    pub env: Option<String>,
    pub env_example: Option<String>,
    pub vendor: bool,
    /// `composer.lock` is newer than the last install.
    pub vendor_stale: bool,
    pub package_json: bool,
    pub node_modules: bool,
    /// The JS lock file is newer than the last install.
    pub node_stale: bool,
    pub storage_link_missing: bool,
    /// The package manager a lock file names when it isn't npm (`pnpm`, `yarn`, `bun`).
    pub js_manager: Option<String>,
    /// The SQLite file the `.env` names, and whether it exists.
    pub sqlite: Option<(PathBuf, bool)>,
    pub instances: Vec<InstanceFacts>,
    /// Ports on 127.0.0.1 that answered (of those [`ports_to_probe`] asked for).
    pub listening: Vec<u16>,
    pub answer: Answer,
    /// The last error line of `storage/logs/laravel.log`.
    pub log_line: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// .env as Laravel reads it
// ---------------------------------------------------------------------------------------------

/// One setting: its key, 1-based line, and value (unquoted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub line: usize,
    pub value: String,
}

/// Every setting of a dotenv text, in order, repeats included. A double-quoted value may run
/// over several lines.
#[must_use]
pub fn entries(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((i, raw)) = lines.next() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            continue;
        }
        let value = value.trim();
        let value = if let Some(rest) = value.strip_prefix('"') {
            let mut body = rest.to_owned();
            // An opening quote without its closing one on this line: the value goes on.
            while !closes(&body) {
                let Some((_, next)) = lines.next() else { break };
                body.push('\n');
                body.push_str(next);
            }
            let end = closing_quote(&body).unwrap_or(body.len());
            body[..end].to_owned()
        } else if let Some(rest) = value.strip_prefix('\'') {
            rest.split_once('\'').map_or(rest, |(v, _)| v).to_owned()
        } else {
            value
                .split_once(" #")
                .map_or(value, |(v, _)| v)
                .trim()
                .to_owned()
        };
        out.push(Entry {
            key: key.to_owned(),
            line: i + 1,
            value,
        });
    }
    out
}

/// Where the unescaped closing `"` is.
fn closing_quote(body: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some(i),
            _ => escaped = false,
        }
    }
    None
}

fn closes(body: &str) -> bool {
    closing_quote(body).is_some()
}

/// The value Laravel reads for `key`: phpdotenv lets a later line replace an earlier one
/// from the same file, so the last.
#[must_use]
pub fn value(text: &str, key: &str) -> Option<String> {
    entries(text)
        .into_iter()
        .rev()
        .find(|e| e.key == key)
        .map(|e| e.value)
}

fn is_loopback(host: &str) -> bool {
    matches!(
        host.trim_matches(['[', ']']),
        "127.0.0.1" | "localhost" | "::1"
    )
}

/// A database the `.env` points at: (connection, host, port).
fn database_target(env: &str) -> Option<(String, String, u16)> {
    let connection = value(env, "DB_CONNECTION")?;
    let default = match connection.as_str() {
        "mysql" | "mariadb" => 3306,
        "pgsql" => 5432,
        _ => return None,
    };
    let host = value(env, "DB_HOST")
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let port = value(env, "DB_PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(default);
    Some((connection, host, port))
}

/// Whether the `.env` sends anything to Redis.
fn uses_redis(env: &str) -> bool {
    [
        "CACHE_STORE",
        "CACHE_DRIVER",
        "SESSION_DRIVER",
        "QUEUE_CONNECTION",
        "BROADCAST_CONNECTION",
    ]
    .iter()
    .any(|k| value(env, k).as_deref() == Some("redis"))
}

fn port_of(env: &str, key: &str, default: u16) -> u16 {
    value(env, key)
        .and_then(|p| p.parse().ok())
        .unwrap_or(default)
}

fn host_of(env: &str, key: &str) -> String {
    value(env, key)
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_owned())
}

/// The ports on 127.0.0.1 the `.env` relies on, for the CLI to probe.
#[must_use]
pub fn ports_to_probe(env: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    if let Some((_, host, port)) = database_target(env)
        && is_loopback(&host)
    {
        ports.push(port);
    }
    if uses_redis(env) && is_loopback(&host_of(env, "REDIS_HOST")) {
        ports.push(port_of(env, "REDIS_PORT", 6379));
    }
    if value(env, "MAIL_MAILER").as_deref() == Some("smtp")
        && is_loopback(&host_of(env, "MAIL_HOST"))
    {
        ports.push(port_of(env, "MAIL_PORT", 1025));
    }
    ports
}

/// The SQLite file a `.env` names: `DB_DATABASE` when it's absolute, Laravel's
/// `database/database.sqlite` when it's empty, and `None` for a relative name (relative to
/// wherever PHP runs).
#[must_use]
pub fn sqlite_path(folder: &Path, env: &str) -> Option<PathBuf> {
    if value(env, "DB_CONNECTION")? != "sqlite" {
        return None;
    }
    match value(env, "DB_DATABASE").filter(|v| !v.is_empty()) {
        Some(path) if Path::new(&path).is_absolute() => Some(PathBuf::from(path)),
        Some(_) => None,
        None => Some(folder.join("database").join("database.sqlite")),
    }
}

// ---------------------------------------------------------------------------------------------
// Composer's PHP constraint
// ---------------------------------------------------------------------------------------------

type Version = [u64; 3];

/// `8.4.27`, `8.2`, `v8`: up to three numbers, a pre-release or stability suffix dropped.
fn parse_version(text: &str) -> Option<(Version, usize)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let text = text.split(['-', '@', '+']).next()?;
    let mut parts = [0u64; 3];
    let mut count = 0;
    for part in text.split('.') {
        if count == 3 {
            // Composer allows a fourth number; it never matters for PHP.
            part.parse::<u64>().ok()?;
            continue;
        }
        parts[count] = part.parse().ok()?;
        count += 1;
    }
    (count > 0).then_some((parts, count))
}

/// The next version after the first `places` numbers: `bump([8,2,0], 1)` is `[9,0,0]`.
fn bump(v: Version, places: usize) -> Version {
    let mut out = [0; 3];
    out[..places].copy_from_slice(&v[..places]);
    out[places - 1] += 1;
    out
}

/// One constraint (`^8.2`, `>=8.1`, `8.3.*`) as an inclusive lower and exclusive upper
/// bound, or `!=`.
fn allows_one(token: &str, v: Version) -> Option<bool> {
    let token = token.split('@').next()?.trim();
    if token == "*" || token.is_empty() {
        return Some(true);
    }
    for (op, cmp) in [
        (">=", 0u8),
        ("<=", 1),
        ("!=", 2),
        ("==", 3),
        (">", 4),
        ("<", 5),
        ("=", 3),
    ] {
        if let Some(rest) = token.strip_prefix(op) {
            let (w, _) = parse_version(rest)?;
            return Some(match cmp {
                0 => v >= w,
                1 => v <= w,
                2 => v != w,
                3 => v == w,
                4 => v > w,
                _ => v < w,
            });
        }
    }
    if let Some(rest) = token.strip_prefix('^') {
        let (w, n) = parse_version(rest)?;
        // ^8.2 is >=8.2 <9; ^0.3 is >=0.3 <0.4.
        let places = w.iter().take(n).position(|&x| x != 0).map_or(n, |i| i + 1);
        return Some(v >= w && v < bump(w, places));
    }
    if let Some(rest) = token.strip_prefix('~') {
        let (w, n) = parse_version(rest)?;
        // ~8.2 is >=8.2 <9; ~8.2.1 is >=8.2.1 <8.3.
        let places = if n == 1 { 1 } else { n - 1 };
        return Some(v >= w && v < bump(w, places));
    }
    if let Some(rest) = token
        .strip_suffix(".*")
        .or_else(|| token.strip_suffix(".x"))
    {
        let (w, n) = parse_version(rest)?;
        return Some(v >= w && v < bump(w, n));
    }
    let (w, _) = parse_version(token)?;
    Some(v == w)
}

/// Whether `version` meets a Composer constraint (`^8.2|^8.3`, `>=8.1 <8.4`), or `None` if
/// the constraint isn't one Kiln reads.
#[must_use]
pub fn php_allows(constraint: &str, version: &str) -> Option<bool> {
    let (v, _) = parse_version(version)?;
    let mut any = false;
    for alternative in constraint.replace("||", "|").split('|') {
        let mut tokens: Vec<String> = Vec::new();
        for word in alternative.split([' ', ',']).filter(|w| !w.is_empty()) {
            // `>= 8.1`: an operator on its own belongs to the next word.
            match tokens.last_mut() {
                Some(last) if last.chars().all(|c| "<>=!^~".contains(c)) => last.push_str(word),
                _ => tokens.push(word.to_owned()),
            }
        }
        if tokens.is_empty() {
            return None;
        }
        let all = if tokens.len() == 3 && tokens[1] == "-" {
            // A hyphen range: `8.1 - 8.3` is >=8.1 and below 8.4.
            let (low, _) = parse_version(&tokens[0])?;
            let (high, n) = parse_version(&tokens[2])?;
            v >= low && if n < 3 { v < bump(high, n) } else { v <= high }
        } else {
            let mut all = true;
            for token in &tokens {
                all &= allows_one(token, v)?;
            }
            all
        };
        any |= all;
    }
    Some(any)
}

fn minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

// ---------------------------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------------------------

fn runs(mut f: Finding, args: &[&str]) -> Finding {
    f.fix = Some(format!("kiln {}", args.join(" ")));
    f.run = Some(args.iter().map(|&a| a.to_owned()).collect());
    f
}

fn init(f: Finding, folder: &Path) -> Finding {
    let folder = folder.to_string_lossy();
    runs(f, &["init", &folder])
}

fn project(f: &SiteFacts, out: &mut Vec<Finding>) {
    let what = match f.kind {
        Kind::Laravel => "a Laravel app",
        Kind::WordPress => "a WordPress site",
        Kind::Other => "a PHP project",
        Kind::Static => "a static site",
        Kind::Proxy => "a proxy",
    };
    out.push(finding(
        Area::Project,
        Status::Ok,
        if f.name == f.folder.display().to_string() {
            format!("{} is {what}", f.name)
        } else {
            format!("{} is {what} in {}", f.name, f.folder.display())
        },
    ));
    if f.kind == Kind::Static {
        return; // served as files: no PHP
    }
    let Some(php) = &f.php else {
        out.push(runs(
            finding(Area::Project, Status::Fail, "Kiln has no PHP for it yet"),
            &["php", "install", "8.4"],
        ));
        return;
    };
    let composer: Option<serde_json::Value> = f
        .composer_json
        .as_deref()
        .and_then(|t| serde_json::from_str(t).ok());
    let require = composer.as_ref().map(|c| &c["require"]);
    match require.and_then(|r| r["php"].as_str()) {
        Some(range) => match php_allows(range, php) {
            Some(true) => out.push(finding(
                Area::Project,
                Status::Ok,
                format!("PHP {php} meets composer.json's \"{range}\""),
            )),
            Some(false) => out.push(with_fix(
                finding(
                    Area::Project,
                    Status::Fail,
                    format!("PHP {php} doesn't meet composer.json's \"{range}\""),
                ),
                format!(
                    "give it a PHP that does: `kiln isolate <version>` in {}, or php in kiln.toml",
                    f.folder.display()
                ),
            )),
            None => out.push(finding(
                Area::Project,
                Status::Info,
                format!("composer.json asks for PHP \"{range}\", which Kiln can't read; not checked (it has PHP {php})"),
            )),
        },
        None => out.push(finding(
            Area::Project,
            Status::Ok,
            format!("PHP {php} (composer.json doesn't ask for a version)"),
        )),
    }
    extensions(f, php, require, out);
}

/// The `ext-*` composer.json requires, against what the site's PHP loads.
fn extensions(
    f: &SiteFacts,
    php: &str,
    require: Option<&serde_json::Value>,
    out: &mut Vec<Finding>,
) {
    let wanted: Vec<String> = require
        .and_then(|r| r.as_object())
        .map(|r| {
            r.keys()
                .filter_map(|k| k.strip_prefix("ext-"))
                .map(|name| match name.to_ascii_lowercase().as_str() {
                    "zend-opcache" | "zend_opcache" => "opcache".to_owned(),
                    other => other.replace('-', "_"),
                })
                .collect()
        })
        .unwrap_or_default();
    if wanted.is_empty() {
        return;
    }
    let Some(loaded) = &f.loaded_ext else {
        out.push(finding(
            Area::Project,
            Status::Info,
            format!("PHP {php}'s extensions couldn't be listed; the ones composer.json needs aren't checked"),
        ));
        return;
    };
    let missing: Vec<&String> = wanted.iter().filter(|w| !loaded.contains(w)).collect();
    if missing.is_empty() {
        out.push(finding(
            Area::Project,
            Status::Ok,
            format!(
                "the extensions composer.json needs are on ({})",
                wanted.join(", ")
            ),
        ));
    }
    for name in missing {
        let message =
            format!("composer.json needs the {name} extension, which PHP {php} hasn't on");
        if f.shipped_ext.contains(name) {
            out.push(runs(
                finding(Area::Project, Status::Warn, message),
                &["php", "ext", "on", name, "--php", &minor(php)],
            ));
        } else {
            out.push(with_fix(
                finding(Area::Project, Status::Warn, message),
                format!(
                    "PHP {php} doesn't ship it; `kiln php ext add {name}` adds the ones Kiln pins"
                ),
            ));
        }
    }
}

fn keys(n: usize) -> &'static str {
    if n == 1 { "key" } else { "keys" }
}

/// Keys `.env.example` has that `.env` lacks (a warning), and the other way round (a note:
/// often the developer's own).
fn against_example(mine: &[Entry], example: &[Entry], out: &mut Vec<Finding>) {
    let missing_from = |from: &[Entry], of: &[Entry]| {
        let mut names: Vec<String> = Vec::new();
        for e in of {
            if !from.iter().any(|m| m.key == e.key) && !names.contains(&e.key) {
                names.push(e.key.clone());
            }
        }
        names
    };
    let lacking = missing_from(mine, example);
    out.push(if lacking.is_empty() {
        finding(
            Area::EnvFile,
            Status::Ok,
            ".env has every key .env.example has",
        )
    } else {
        with_fix(
            finding(
                Area::EnvFile,
                Status::Warn,
                format!(
                    "{} {} in .env.example that .env lacks: {}",
                    lacking.len(),
                    keys(lacking.len()),
                    lacking.join(", ")
                ),
            ),
            "add them to .env (their values are the project's choice)",
        )
    });
    let extra = missing_from(example, mine);
    if !extra.is_empty() {
        out.push(finding(
            Area::EnvFile,
            Status::Info,
            format!(
                "{} {} in .env that .env.example doesn't list: {}",
                extra.len(),
                keys(extra.len()),
                extra.join(", ")
            ),
        ));
    }
}

fn env_file(f: &SiteFacts, out: &mut Vec<Finding>) {
    let Some(env) = &f.env else {
        let missing = finding(Area::EnvFile, Status::Fail, ".env is missing");
        out.push(if f.env_example.is_some() {
            init(missing, &f.folder)
        } else {
            with_fix(missing, "create .env with the app's settings")
        });
        return;
    };
    let mine = entries(env);
    if let Some(example) = &f.env_example {
        against_example(&mine, &entries(example), out);
    }
    let mut seen: Vec<&str> = Vec::new();
    for e in &mine {
        if seen.contains(&e.key.as_str()) {
            continue;
        }
        seen.push(&e.key);
        let lines: Vec<String> = mine
            .iter()
            .filter(|m| m.key == e.key)
            .map(|m| m.line.to_string())
            .collect();
        if lines.len() > 1 {
            out.push(with_fix(
                finding(
                    Area::EnvFile,
                    Status::Warn,
                    format!(
                        "{} is set on lines {}; Laravel reads the last",
                        e.key,
                        lines.join(", ")
                    ),
                ),
                "keep one",
            ));
        }
    }
    if value(env, "APP_KEY").is_some_and(|k| !k.is_empty()) {
        out.push(finding(Area::EnvFile, Status::Ok, "APP_KEY is set"));
    } else {
        out.push(init(
            finding(
                Area::EnvFile,
                Status::Fail,
                "APP_KEY is empty: Laravel can't encrypt sessions or cookies",
            ),
            &f.folder,
        ));
    }
    if let Some(url) = &f.url {
        let wanted = url.trim_end_matches('/');
        match value(env, "APP_URL") {
            Some(set) if set.trim_end_matches('/') == wanted => out.push(finding(
                Area::EnvFile,
                Status::Ok,
                format!("APP_URL is the site's URL ({wanted})"),
            )),
            set => out.push(with_fix(
                finding(
                    Area::EnvFile,
                    Status::Warn,
                    format!(
                        "APP_URL is {}, but the site is at {wanted}: links and assets may point elsewhere",
                        set.filter(|s| !s.is_empty()).unwrap_or_else(|| "not set".to_owned())
                    ),
                ),
                format!("set APP_URL={wanted} in .env"),
            )),
        }
    }
}

fn packages(f: &SiteFacts, out: &mut Vec<Finding>) {
    let composer = f.composer_json.is_some();
    if composer && !f.vendor {
        out.push(init(
            finding(
                Area::Packages,
                Status::Fail,
                "vendor/ is missing: Composer hasn't installed its packages",
            ),
            &f.folder,
        ));
    } else if composer && f.vendor_stale {
        out.push(with_fix(
            finding(
                Area::Packages,
                Status::Warn,
                "composer.lock changed after the last install",
            ),
            "composer install",
        ));
    } else if composer {
        out.push(finding(
            Area::Packages,
            Status::Ok,
            "Composer's packages are installed",
        ));
    }
    if f.package_json && !f.node_modules {
        let missing = finding(
            Area::Packages,
            Status::Warn,
            "node_modules/ is missing: the front end's packages aren't installed",
        );
        // kiln init installs with npm only; it leaves another manager's lock file alone.
        out.push(match &f.js_manager {
            Some(manager) => with_fix(missing, format!("{manager} install")),
            None => init(missing, &f.folder),
        });
    } else if f.package_json && f.node_stale {
        out.push(with_fix(
            finding(
                Area::Packages,
                Status::Warn,
                "the front end's lock file changed after the last install",
            ),
            "npm install (or the project's own package manager)",
        ));
    } else if f.package_json {
        out.push(finding(
            Area::Packages,
            Status::Ok,
            "the front end's packages are installed",
        ));
    }
    if f.kind == Kind::Laravel && f.storage_link_missing {
        out.push(init(
            finding(
                Area::Packages,
                Status::Info,
                "public/storage isn't linked: uploaded files won't show",
            ),
            &f.folder,
        ));
    }
}

/// The instance of `kinds` on `port`.
fn instance_on<'a>(f: &'a SiteFacts, port: u16, kinds: &[&str]) -> Option<&'a InstanceFacts> {
    f.instances
        .iter()
        .find(|i| i.ports.contains(&port) && kinds.contains(&i.kind.as_str()))
}

/// A service the `.env` relies on, by port: Kiln's and running, stopped, or someone else's.
/// Returns the instance when it runs.
fn service_on<'a>(
    f: &'a SiteFacts,
    out: &mut Vec<Finding>,
    what: &str,
    port: u16,
    kinds: &[&str],
    missing: Status,
) -> Option<&'a InstanceFacts> {
    let listening = f.listening.contains(&port);
    match instance_on(f, port, kinds) {
        Some(i) if !i.enabled => {
            out.push(runs(
                finding(
                    Area::Database,
                    missing,
                    format!("{what} is Kiln's {}, which is stopped", i.name),
                ),
                &["service", "start", &i.name],
            ));
            None
        }
        Some(i) if !listening => {
            out.push(runs(
                finding(
                    Area::Database,
                    missing,
                    format!(
                        "{what} is Kiln's {}, which doesn't answer on port {port}",
                        i.name
                    ),
                ),
                &["service", "restart", &i.name],
            ));
            None
        }
        Some(i) => Some(i),
        None if listening => {
            out.push(finding(
                Area::Database,
                Status::Info,
                format!("{what} is on port {port}, which Kiln doesn't run; not checked"),
            ));
            None
        }
        None => {
            // One of Kiln's of that kind, a running one first.
            let mut candidates: Vec<&InstanceFacts> = f
                .instances
                .iter()
                .filter(|i| kinds.contains(&i.kind.as_str()))
                .collect();
            candidates.sort_by_key(|i| !i.enabled);
            let other = candidates.first().copied();
            let message = finding(
                Area::Database,
                missing,
                format!("{what} is on 127.0.0.1:{port}, where nothing answers"),
            );
            out.push(match other {
                Some(i)
                    if kinds
                        .iter()
                        .any(|k| ["mysql", "mariadb", "postgres"].contains(k)) =>
                {
                    let folder = f.folder.to_string_lossy();
                    runs(message, &["env", "use", &i.name, "--project", &folder])
                }
                Some(i) => with_fix(
                    message,
                    format!("point it at Kiln's {} (port {:?})", i.name, i.ports),
                ),
                None => with_fix(message, "start that server, or add one of Kiln's"),
            });
            None
        }
    }
}

fn database(f: &SiteFacts, out: &mut Vec<Finding>) {
    let Some(env) = &f.env else { return };
    let connection = value(env, "DB_CONNECTION").filter(|c| !c.is_empty());
    match connection.as_deref() {
        None => out.push(finding(
            Area::Database,
            Status::Info,
            "DB_CONNECTION isn't set: config/database.php decides; not checked",
        )),
        Some("sqlite") => match &f.sqlite {
            Some((path, true)) => out.push(finding(
                Area::Database,
                Status::Ok,
                format!("SQLite: {}", path.display()),
            )),
            Some((path, false)) => out.push(init(
                finding(
                    Area::Database,
                    Status::Fail,
                    format!("the SQLite file {} doesn't exist", path.display()),
                ),
                &f.folder,
            )),
            None => out.push(finding(
                Area::Database,
                Status::Info,
                "SQLite at a relative path, which depends on where PHP runs; not checked",
            )),
        },
        Some(_) => match database_target(env) {
            None => out.push(finding(
                Area::Database,
                Status::Info,
                format!(
                    "DB_CONNECTION is {}, which Kiln doesn't check",
                    connection.unwrap_or_default()
                ),
            )),
            Some((_, host, _)) if !is_loopback(&host) => out.push(finding(
                Area::Database,
                Status::Info,
                format!("the database is on {host}, not this computer; not checked"),
            )),
            Some((connection, _, port)) => server_database(f, env, &connection, port, out),
        },
    }
    services(f, env, out);
}

/// A MySQL, MariaDB or PostgreSQL database on this computer.
fn server_database(f: &SiteFacts, env: &str, connection: &str, port: u16, out: &mut Vec<Finding>) {
    let kinds: &[&str] = if connection == "pgsql" {
        &["postgres"]
    } else {
        &["mysql", "mariadb"]
    };
    let Some(instance) = service_on(f, out, "The database", port, kinds, Status::Fail) else {
        return;
    };
    let name = value(env, "DB_DATABASE").filter(|d| !d.is_empty());
    match (&instance.databases, name) {
        (_, None) => out.push(finding(
            Area::Database,
            Status::Warn,
            format!("DB_DATABASE isn't set; {} is running", instance.name),
        )),
        (None, Some(name)) => out.push(finding(
            Area::Database,
            Status::Info,
            format!(
                "{}'s databases couldn't be listed; {name} not checked",
                instance.name
            ),
        )),
        (Some(list), Some(name)) if list.contains(&name) => out.push(finding(
            Area::Database,
            Status::Ok,
            format!("{name} on Kiln's {}", instance.name),
        )),
        (Some(_), Some(name)) => out.push(runs(
            finding(
                Area::Database,
                Status::Fail,
                format!("Kiln's {} has no database called {name}", instance.name),
            ),
            &["db", "create", &instance.name, &name],
        )),
    }
}

/// Redis and mail, when the `.env` uses them on this computer.
fn services(f: &SiteFacts, env: &str, out: &mut Vec<Finding>) {
    if uses_redis(env) && is_loopback(&host_of(env, "REDIS_HOST")) {
        let port = port_of(env, "REDIS_PORT", 6379);
        if let Some(i) = service_on(f, out, "Redis", port, &["redis", "valkey"], Status::Warn) {
            out.push(finding(
                Area::Database,
                Status::Ok,
                format!("Redis: Kiln's {}", i.name),
            ));
        }
    }
    if value(env, "MAIL_MAILER").as_deref() == Some("smtp")
        && is_loopback(&host_of(env, "MAIL_HOST"))
    {
        let port = port_of(env, "MAIL_PORT", 1025);
        if let Some(i) = service_on(f, out, "Mail", port, &["mailpit"], Status::Warn) {
            out.push(finding(
                Area::Database,
                Status::Ok,
                format!("Mail goes to Kiln's {}", i.name),
            ));
        }
    }
}

fn answer(f: &SiteFacts, out: &mut Vec<Finding>) {
    let url = f.url.as_deref().unwrap_or(&f.name);
    out.push(match &f.answer {
        Answer::NotServed => with_fix(
            finding(Area::Answer, Status::Info, "Kiln doesn't serve this folder"),
            format!("`kiln link` in {}", f.folder.display()),
        ),
        Answer::ServerStopped => runs(
            finding(
                Area::Answer,
                Status::Info,
                "the web server is stopped; not asked",
            ),
            &["web", "start"],
        ),
        Answer::Got { code, .. } if (200..300).contains(code) => {
            finding(Area::Answer, Status::Ok, format!("{url} answers {code}"))
        }
        Answer::Got { code, location } if (300..400).contains(code) => finding(
            Area::Answer,
            Status::Ok,
            format!(
                "{url} answers {code}, to {}",
                location.as_deref().unwrap_or("(no Location)")
            ),
        ),
        Answer::Got { code, .. } if *code >= 500 => {
            let mut message = format!("{url} answers {code}: the app failed");
            if let Some(line) = &f.log_line {
                message.push_str("\nlaravel.log: ");
                message.push_str(line);
            }
            finding(Area::Answer, Status::Fail, message)
        }
        Answer::Got { code, .. } => finding(
            Area::Answer,
            Status::Warn,
            format!("{url} answers {code} at /"),
        ),
        Answer::Failed(why) => finding(
            Area::Answer,
            Status::Fail,
            format!("{url} didn't answer: {why}"),
        ),
    });
}

/// The findings for one site.
#[must_use]
pub fn diagnose(f: &SiteFacts) -> Report {
    let mut out = Vec::new();
    if f.kind != Kind::Proxy {
        project(f, &mut out);
        if f.kind == Kind::Laravel {
            env_file(f, &mut out);
        }
        packages(f, &mut out);
        if f.kind == Kind::Laravel {
            database(f, &mut out);
        }
    }
    answer(f, &mut out);
    Report { findings: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laravel() -> SiteFacts {
        SiteFacts {
            name: "shop.test".into(),
            folder: PathBuf::from(r"C:\Sites\shop"),
            kind: Kind::Laravel,
            url: Some("https://shop.test".into()),
            php: Some("8.4.27".into()),
            composer_json: Some(r#"{"require":{"php":"^8.2","ext-mbstring":"*"}}"#.into()),
            loaded_ext: Some(vec!["mbstring".into(), "pdo_mysql".into()]),
            env: Some(
                "APP_KEY=base64:abc\nAPP_URL=https://shop.test\nDB_CONNECTION=sqlite\n".into(),
            ),
            env_example: Some("APP_KEY=\nAPP_URL=\nDB_CONNECTION=sqlite\n".into()),
            vendor: true,
            sqlite: Some((
                PathBuf::from(r"C:\Sites\shop\database\database.sqlite"),
                true,
            )),
            answer: Answer::Got {
                code: 200,
                location: None,
            },
            ..SiteFacts::default()
        }
    }

    fn problems(r: &Report) -> Vec<&Finding> {
        r.findings
            .iter()
            .filter(|f| f.status >= Status::Warn)
            .collect()
    }

    #[test]
    fn a_healthy_site_has_no_problems() {
        let report = diagnose(&laravel());
        assert_eq!(problems(&report), Vec::<&Finding>::new());
        assert!(report.render().contains("shop.test answers 200"));
    }

    #[test]
    fn composer_constraints() {
        for (range, version, ok) in [
            ("^8.2", "8.4.27", Some(true)),
            ("^8.2", "8.1.30", Some(false)),
            ("^8.2", "9.0.0", Some(false)),
            ("~8.2.0", "8.2.9", Some(true)),
            ("~8.2.0", "8.3.0", Some(false)),
            ("~8.2", "8.9.0", Some(true)),
            (">=8.1 <8.4", "8.3.1", Some(true)),
            (">=8.1,<8.4", "8.4.0", Some(false)),
            (">= 8.1", "8.1.0", Some(true)),
            ("^7.4|^8.0", "8.3.0", Some(true)),
            ("^7.4 || ^8.0", "7.3.0", Some(false)),
            ("8.2.*", "8.2.4", Some(true)),
            ("8.2.*", "8.3.0", Some(false)),
            ("8.1 - 8.3", "8.3.20", Some(true)),
            ("8.1 - 8.3", "8.4.0", Some(false)),
            ("*", "8.4.1", Some(true)),
            ("^8.2@dev", "8.4.1", Some(true)),
            ("dev-main", "8.4.1", None),
            ("^8.x-dev", "8.4.1", None),
        ] {
            assert_eq!(php_allows(range, version), ok, "{range} against {version}");
        }
    }

    #[test]
    fn a_php_outside_the_range_fails_and_an_unreadable_one_is_not_checked() {
        let mut facts = laravel();
        facts.php = Some("8.1.30".into());
        let report = diagnose(&facts);
        assert!(problems(&report)[0].message.contains("doesn't meet"));
        facts.composer_json = Some(r#"{"require":{"php":"dev-main"}}"#.into());
        let text = diagnose(&facts).render();
        assert!(text.contains("not checked"), "{text}");
    }

    #[test]
    fn a_missing_extension_is_turned_on_when_the_build_ships_it() {
        let mut facts = laravel();
        facts.composer_json =
            Some(r#"{"require":{"ext-gmp":"*","ext-imagick":"*","ext-zend-opcache":"*"}}"#.into());
        facts.loaded_ext = Some(vec!["opcache".into()]);
        facts.shipped_ext = vec!["gmp".into()];
        let report = diagnose(&facts);
        let found = problems(&report);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found[0].run.as_deref(),
            Some(&["php", "ext", "on", "gmp", "--php", "8.4"].map(String::from)[..])
        );
        assert!(found[1].run.is_none());
        facts.loaded_ext = None;
        assert!(diagnose(&facts).render().contains("couldn't be listed"));
    }

    #[test]
    fn env_keys_missing_and_repeated_are_named_never_their_values() {
        let mut facts = laravel();
        facts.env = Some(
            "APP_KEY=base64:secret\nAPP_URL=https://shop.test\nDB_CONNECTION=sqlite\nDB_PASSWORD=hunter2\nDB_PASSWORD=other\n"
                .into(),
        );
        facts.env_example =
            Some("APP_KEY=\nAPP_URL=\nMAIL_MAILER=log\nSTRIPE_KEY=\n# comment\n".into());
        let text = diagnose(&facts).render();
        assert!(
            text.contains("2 keys in .env.example that .env lacks: MAIL_MAILER, STRIPE_KEY"),
            "{text}"
        );
        assert!(text.contains("DB_PASSWORD is set on lines 4, 5; Laravel reads the last"));
        assert!(!text.contains("hunter2") && !text.contains("secret"));
    }

    #[test]
    fn a_clone_points_at_kiln_init() {
        let mut facts = laravel();
        facts.env = None;
        facts.vendor = false;
        facts.package_json = true;
        let report = diagnose(&facts);
        let runs: Vec<_> = report
            .findings
            .iter()
            .filter_map(|f| f.run.clone())
            .collect();
        assert!(runs.len() >= 3);
        assert!(runs.iter().all(|r| r == &["init", r"C:\Sites\shop"]));
        let text = report.render();
        assert!(text.contains(".env is missing") && text.contains("vendor/ is missing"));
    }

    #[test]
    fn an_empty_key_and_a_wrong_url_are_found() {
        let mut facts = laravel();
        facts.env = Some("APP_KEY=\nAPP_URL=http://localhost\nDB_CONNECTION=sqlite\n".into());
        let text = diagnose(&facts).render();
        assert!(text.contains("APP_KEY is empty"));
        assert!(text.contains("APP_URL is http://localhost, but the site is at https://shop.test"));
    }

    fn mysql_site(enabled: bool, databases: Option<Vec<String>>, listening: bool) -> SiteFacts {
        let mut facts = laravel();
        facts.env = Some(
            "APP_KEY=x\nAPP_URL=https://shop.test\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=3307\nDB_DATABASE=shop\n"
                .into(),
        );
        facts.sqlite = None;
        facts.instances = vec![InstanceFacts {
            name: "mysql-8.4".into(),
            kind: "mysql".into(),
            ports: vec![3307],
            enabled,
            databases,
        }];
        if listening {
            facts.listening = vec![3307];
        }
        facts
    }

    #[test]
    fn the_database_on_kilns_instance() {
        let ok = diagnose(&mysql_site(true, Some(vec!["shop".into()]), true));
        assert!(ok.render().contains("shop on Kiln's mysql-8.4"));
        let missing = diagnose(&mysql_site(true, Some(vec![]), true));
        assert_eq!(
            problems(&missing)[0].run.as_deref(),
            Some(&["db", "create", "mysql-8.4", "shop"].map(String::from)[..])
        );
        let stopped = diagnose(&mysql_site(false, None, false));
        assert_eq!(
            problems(&stopped)[0].run.as_deref(),
            Some(&["service", "start", "mysql-8.4"].map(String::from)[..])
        );
        let unlisted = diagnose(&mysql_site(true, None, true)).render();
        assert!(unlisted.contains("couldn't be listed; shop not checked"));
    }

    #[test]
    fn a_database_kiln_does_not_run_is_not_checked() {
        let mut facts = mysql_site(true, None, false);
        facts.instances.clear();
        facts.listening = vec![3307];
        assert!(
            diagnose(&facts)
                .render()
                .contains("Kiln doesn't run; not checked")
        );
        facts.listening.clear();
        let report = diagnose(&facts);
        assert!(problems(&report)[0].message.contains("nothing answers"));
        facts.env = Some("APP_KEY=x\nDB_CONNECTION=pgsql\nDB_HOST=db.example.com\n".into());
        assert!(
            diagnose(&facts)
                .render()
                .contains("not this computer; not checked")
        );
    }

    #[test]
    fn redis_and_mail_are_checked_when_used() {
        let mut facts = laravel();
        facts.env = Some(
            "APP_KEY=x\nAPP_URL=https://shop.test\nDB_CONNECTION=sqlite\nCACHE_STORE=redis\nMAIL_MAILER=smtp\nMAIL_PORT=1026\n"
                .into(),
        );
        assert_eq!(
            ports_to_probe(facts.env.as_deref().unwrap()),
            vec![6379, 1026]
        );
        facts.instances = vec![InstanceFacts {
            name: "mailpit".into(),
            kind: "mailpit".into(),
            ports: vec![1026, 8026],
            enabled: false,
            databases: None,
        }];
        let report = diagnose(&facts);
        let found = problems(&report);
        assert!(found[0].message.contains("Redis is on 127.0.0.1:6379"));
        assert_eq!(
            found[1].run.as_deref(),
            Some(&["service", "start", "mailpit"].map(String::from)[..])
        );
    }

    #[test]
    fn the_answer() {
        let mut facts = laravel();
        facts.answer = Answer::Got {
            code: 500,
            location: None,
        };
        facts.log_line = Some("local.ERROR: Class \"Foo\" not found".into());
        let text = diagnose(&facts).render();
        assert!(text.contains("answers 500") && text.contains("laravel.log: local.ERROR"));
        facts.answer = Answer::ServerStopped;
        assert!(diagnose(&facts).render().contains("stopped; not asked"));
        facts.kind = Kind::Proxy;
        facts.answer = Answer::Failed("connection refused".into());
        let report = diagnose(&facts);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.worst(), Status::Fail);
    }

    #[test]
    fn dotenv_values_as_laravel_reads_them() {
        let text = "A=1\nexport B=\"two\nlines\"\nC='x # y'\nD=plain # note\nA=3\n";
        assert_eq!(value(text, "A").as_deref(), Some("3"));
        assert_eq!(value(text, "B").as_deref(), Some("two\nlines"));
        assert_eq!(value(text, "C").as_deref(), Some("x # y"));
        assert_eq!(value(text, "D").as_deref(), Some("plain"));
        let lines: Vec<usize> = entries(text).iter().map(|e| e.line).collect();
        assert_eq!(lines, vec![1, 2, 4, 5, 6]);
    }

    #[test]
    fn keys_only_in_env_are_a_note() {
        let mut facts = laravel();
        facts.env = Some(
            "APP_KEY=x\nAPP_URL=https://shop.test\nDB_CONNECTION=sqlite\nMY_TOKEN=abc\n".into(),
        );
        let report = diagnose(&facts);
        let note = report
            .findings
            .iter()
            .find(|f| f.message.contains("doesn't list"))
            .unwrap();
        assert_eq!(note.status, Status::Info);
        assert_eq!(
            note.message,
            "1 key in .env that .env.example doesn't list: MY_TOKEN"
        );
        assert!(!report.render().contains("abc"));
    }

    #[test]
    fn a_static_site_needs_no_php() {
        let facts = SiteFacts {
            name: "docs.test".into(),
            folder: PathBuf::from(r"C:\Sites\docs"),
            kind: Kind::Static,
            url: Some("http://docs.test".into()),
            answer: Answer::Got {
                code: 200,
                location: None,
            },
            ..SiteFacts::default()
        };
        assert_eq!(diagnose(&facts).worst(), Status::Ok);
    }

    #[test]
    fn another_package_manager_is_named_not_kiln_init() {
        let mut facts = laravel();
        facts.package_json = true;
        facts.js_manager = Some("pnpm".into());
        let report = diagnose(&facts);
        let missing = problems(&report)[0];
        assert_eq!(missing.fix.as_deref(), Some("pnpm install"));
        assert!(missing.run.is_none());
    }

    #[test]
    fn env_use_points_at_a_running_instance_first() {
        let mut facts = mysql_site(true, None, false);
        facts.instances[0].ports = vec![3399];
        facts.instances[0].enabled = false;
        facts.instances.push(InstanceFacts {
            name: "mariadb-11".into(),
            kind: "mariadb".into(),
            ports: vec![3308],
            enabled: true,
            databases: None,
        });
        let report = diagnose(&facts);
        assert_eq!(
            problems(&report)[0].run.as_ref().unwrap()[..3],
            ["env", "use", "mariadb-11"].map(String::from)
        );
    }

    #[test]
    fn sqlite_paths() {
        let dir = Path::new(r"C:\p");
        assert_eq!(
            sqlite_path(dir, "DB_CONNECTION=sqlite\n"),
            Some(dir.join("database").join("database.sqlite"))
        );
        assert_eq!(
            sqlite_path(dir, "DB_CONNECTION=sqlite\nDB_DATABASE=db.sqlite\n"),
            None
        );
        assert_eq!(sqlite_path(dir, "DB_CONNECTION=mysql\n"), None);
    }
}
