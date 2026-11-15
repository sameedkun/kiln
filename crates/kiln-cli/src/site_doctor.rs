//! `kiln doctor <site>` ([ADR 0081]): gathers one site's facts (read-only) for
//! `kiln_doctor::site`'s rules. It runs no Composer, npm or artisan; the only code of the
//! project's that runs is its answer to one request, as any visit's.
//!
//! [ADR 0081]: ../../../docs/adr/0081-site-doctor.md

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use kiln_doctor::Status;
use kiln_doctor::site::{self, Answer, InstanceFacts, Kind, SiteFacts};
use kiln_state::StateStore;
use kiln_sys::background::ProcStatus;

/// The most of the site's answer read.
const MOST: u64 = 64 * 1024;
const WAIT: Duration = Duration::from_secs(10);

/// Prints the report (with --json, it is the result's data); returns whether nothing failed.
///
/// # Errors
/// The site isn't one of Kiln's, or the folder doesn't exist.
pub fn run(target: &str) -> Result<bool, String> {
    let facts = gather(target)?;
    let report = site::diagnose(&facts);
    if crate::out::json() {
        crate::out::data(serde_json::from_str(&report.to_json()).unwrap_or_default());
    } else {
        print!("{}", report.render());
    }
    Ok(report.worst() < Status::Fail)
}

/// A folder when it has a path separator or is `.`, else a site's name.
fn is_folder(target: &str) -> bool {
    target == "." || target == ".." || target.contains(['/', '\\'])
}

fn kind_of(driver: kiln_sites::Driver) -> Kind {
    match driver {
        kiln_sites::Driver::Laravel => Kind::Laravel,
        kiln_sites::Driver::WordPress | kiln_sites::Driver::WordPressBedrock => Kind::WordPress,
        kiln_sites::Driver::Static => Kind::Static,
        _ => Kind::Other,
    }
}

/// What Kiln serves under that name or for that folder: (name, folder, kind, url).
fn find(
    store: &StateStore,
    target: &str,
) -> Result<(String, PathBuf, Kind, Option<String>), String> {
    let resolved = crate::web::resolved(store)?;
    let urls = crate::web::site_urls(store);
    if is_folder(target) {
        let folder = std::fs::canonicalize(target)
            .map(crate::web::strip_verbatim)
            .map_err(|e| format!("cannot use {target}: {e}"))?;
        let site = resolved
            .sites
            .iter()
            .find(|s| s.alias_of.is_none() && s.project == folder);
        return Ok(if let Some(s) = site {
            (
                s.host.to_string(),
                folder,
                kind_of(s.driver),
                urls.get(s.host.as_str()).cloned(),
            )
        } else {
            let driver = kiln_sites::detect(&folder);
            (folder.display().to_string(), folder, kind_of(driver), None)
        });
    }
    let tld = kiln_runtime::tld(store).map_err(|e| e.to_string())?;
    let host = crate::web::host_name_under(target, &tld)?;
    if let Some(s) = resolved.sites.iter().find(|s| s.host == host) {
        // An alias is checked as its main site, under the name asked for.
        let main = s.main_host();
        let site = resolved.sites.iter().find(|m| &m.host == main).unwrap_or(s);
        return Ok((
            host.to_string(),
            site.project.clone(),
            kind_of(site.driver),
            urls.get(host.as_str()).cloned(),
        ));
    }
    if let Some(p) = crate::web::proxy_specs(store)?
        .into_iter()
        .find(|p| p.host == host)
    {
        return Ok((
            host.to_string(),
            p.root,
            Kind::Proxy,
            urls.get(host.as_str()).cloned(),
        ));
    }
    Err(format!("Kiln doesn't serve {host}"))
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// `lock` changed after `marker` was written by the last install.
fn newer(lock: &Path, marker: &Path) -> bool {
    matches!((modified(lock), modified(marker)), (Some(l), Some(m)) if l > m)
}

/// The JS lock file and what its package manager writes into `node_modules` on install.
fn js_stale(folder: &Path) -> bool {
    let modules = folder.join("node_modules");
    [
        ("package-lock.json", ".package-lock.json"),
        ("pnpm-lock.yaml", ".modules.yaml"),
        ("yarn.lock", ".yarn-integrity"),
    ]
    .iter()
    .any(|(lock, marker)| newer(&folder.join(lock), &modules.join(marker)))
}

/// The package manager a lock file names, when it isn't npm (whose lock wins, as in
/// `kiln init`).
fn js_manager(folder: &Path) -> Option<String> {
    if folder.join("package-lock.json").is_file() {
        return None;
    }
    [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lockb", "bun"),
        ("bun.lock", "bun"),
    ]
    .iter()
    .find(|(lock, _)| folder.join(lock).is_file())
    .map(|(_, manager)| (*manager).to_owned())
}

/// The last `.ERROR:` line of the app's log, cut short.
fn last_error(folder: &Path) -> Option<String> {
    let path = folder.join("storage").join("logs").join("laravel.log");
    let mut file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut tail = Vec::new();
    std::io::Seek::seek(
        &mut file,
        std::io::SeekFrom::Start(size.saturating_sub(MOST)),
    )
    .ok()?;
    file.take(MOST).read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    let line = text.lines().rev().find(|l| l.contains(".ERROR:"))?;
    let line = line.split_once("] ").map_or(line, |(_, rest)| rest).trim();
    let cut = line.floor_char_boundary(300);
    Some(if cut < line.len() {
        format!("{}…", &line[..cut])
    } else {
        line.to_owned()
    })
}

fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// Kiln's instances; databases are listed only for the one on `db_port`, if it runs.
fn instances(store: &StateStore, db_port: Option<u16>, up: &[u16]) -> Vec<InstanceFacts> {
    store
        .services()
        .unwrap_or_default()
        .into_iter()
        .map(|record| {
            let ports: Vec<u16> = record.ports.values().copied().collect();
            let asked = db_port.is_some_and(|p| ports.contains(&p) && up.contains(&p));
            let databases = (asked && record.enabled)
                .then(|| {
                    let provider = crate::service::provider_of(&record.kind).ok()?;
                    let dirs = crate::service::dirs_of(&record).ok()?;
                    let found = provider.list_databases(&dirs, &record.ports)?.ok()?;
                    Some(found.into_iter().map(|(db, _)| db).collect())
                })
                .flatten();
            InstanceFacts {
                name: record.name,
                kind: record.kind,
                ports,
                enabled: record.enabled,
                databases,
            }
        })
        .collect()
}

/// The PHP for the folder, its loaded and shipped extensions.
fn php(store: &StateStore, folder: &Path) -> (Option<String>, Option<Vec<String>>, Vec<String>) {
    let Some(record) = kiln_runtime::php_for_dir(store, folder)
        .ok()
        .and_then(|p| p.php.php)
    else {
        return (None, None, Vec::new());
    };
    let loaded = crate::ext::modules(&record).ok().map(|(loaded, _)| loaded);
    let shipped = crate::ext::shipped(&record).unwrap_or_default();
    (Some(record.version), loaded, shipped)
}

/// For another name of a site: the main site's URL, which `APP_URL` should name (ADR 0083).
fn main_url(store: &StateStore, name: &str) -> Option<String> {
    let resolved = crate::web::resolved(store).ok()?;
    let main = resolved
        .sites
        .iter()
        .find(|s| s.host.as_str() == name)?
        .alias_of
        .clone()?;
    crate::web::site_urls(store).get(main.as_str()).cloned()
}

fn gather(target: &str) -> Result<SiteFacts, String> {
    let store = crate::doctor::open_read_only()?;
    let (name, folder, kind, url) = find(&store, target)?;
    let answer = match &url {
        None => Answer::NotServed,
        Some(url) => ask(&store, &name, url),
    };
    if kind == Kind::Proxy {
        return Ok(SiteFacts {
            name,
            folder,
            kind,
            url,
            answer,
            ..SiteFacts::default()
        });
    }
    let env = read(&folder.join(".env"));
    let probe = env.as_deref().map(site::ports_to_probe).unwrap_or_default();
    let up: Vec<u16> = probe.iter().copied().filter(|&p| listening(p)).collect();
    let db_port = probe.first().copied();
    let (php, loaded_ext, shipped_ext) = php(&store, &folder);
    let log_line = matches!(answer, Answer::Got { code, .. } if code >= 500)
        .then(|| last_error(&folder))
        .flatten();
    Ok(SiteFacts {
        php,
        loaded_ext,
        shipped_ext,
        composer_json: read(&folder.join("composer.json")),
        env_example: read(&folder.join(".env.example")),
        vendor: folder.join("vendor").join("autoload.php").is_file(),
        vendor_stale: newer(
            &folder.join("composer.lock"),
            &folder
                .join("vendor")
                .join("composer")
                .join("installed.json"),
        ),
        package_json: folder.join("package.json").is_file(),
        node_modules: folder.join("node_modules").is_dir(),
        node_stale: js_stale(&folder),
        js_manager: js_manager(&folder),
        storage_link_missing: folder.join("storage").join("app").join("public").is_dir()
            && std::fs::symlink_metadata(folder.join("public").join("storage")).is_err(),
        sqlite: env
            .as_deref()
            .and_then(|e| site::sqlite_path(&folder, e))
            .map(|p| {
                let exists = p.is_file();
                (p, exists)
            }),
        instances: instances(&store, db_port, &up),
        main_url: main_url(&store, &name),
        listening: up,
        env,
        log_line,
        answer,
        name,
        folder,
        kind,
        url,
    })
}

/// One `GET /` to Kiln's web server for `host`, over HTTPS (Kiln's root only) when the URL
/// is. No redirect is followed.
fn ask(store: &StateStore, host: &str, url: &str) -> Answer {
    let running =
        crate::web::server().is_ok_and(|s| matches!(s.status(), Ok(ProcStatus::Running { .. })));
    if !running {
        return Answer::ServerStopped;
    }
    let secure = url.starts_with("https://");
    let kind = if secure {
        &crate::coexist::HTTPS
    } else {
        &crate::coexist::HTTP
    };
    let port = crate::coexist::planned_port(store, kind).unwrap_or(if secure { 443 } else { 80 });
    let result = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), WAIT)
        .map_err(|e| format!("port {port}: {e}"))
        .and_then(|sock| {
            sock.set_read_timeout(Some(WAIT))
                .and_then(|()| sock.set_write_timeout(Some(WAIT)))
                .map_err(|e| e.to_string())?;
            if secure {
                let tls = tls(host, sock)?;
                get(tls, host)
            } else {
                get(sock, host)
            }
        });
    match result {
        Ok((code, location)) => Answer::Got { code, location },
        Err(e) => Answer::Failed(e),
    }
}

/// A TLS stream to `host` that trusts only Kiln's root, its handshake done.
fn tls(
    host: &str,
    mut sock: TcpStream,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, String> {
    let root = crate::apply::ca_store()?
        .root()
        .map_err(|e| format!("Kiln's root certificate: {e}"))?;
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls_pki_types::CertificateDer::from(root.der))
        .map_err(|e| e.to_string())?;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name =
        rustls_pki_types::ServerName::try_from(host.to_owned()).map_err(|e| e.to_string())?;
    let mut conn =
        rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    while conn.is_handshaking() {
        conn.complete_io(&mut sock)
            .map_err(|e| format!("HTTPS: {e}"))?;
    }
    Ok(rustls::StreamOwned::new(conn, sock))
}

/// The status code and `Location` of `GET /` with `Host: host`.
fn get(mut stream: impl Read + Write, host: &str) -> Result<(u16, Option<String>), String> {
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: {host}\r\nUser-Agent: kiln-doctor\r\nAccept: text/html\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    // A TLS peer may close without close_notify; what was read is what matters.
    let _ = stream.take(MOST).read_to_end(&mut response);
    parse(&String::from_utf8_lossy(&response))
}

fn parse(response: &str) -> Result<(u16, Option<String>), String> {
    let head = response.split("\r\n\r\n").next().unwrap_or_default();
    let mut lines = head.lines();
    let code = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or("no HTTP answer within 10 s")?;
    let location = lines.find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("location")
            .then(|| v.trim().to_owned())
    });
    Ok((code, location))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_names() {
        assert!(is_folder("."));
        assert!(is_folder(r"C:\Sites\shop"));
        assert!(is_folder("./shop"));
        assert!(!is_folder("shop"));
        assert!(!is_folder("shop.test"));
    }

    #[test]
    fn an_answer_is_read_from_its_head() {
        assert_eq!(
            parse("HTTP/1.1 302 Found\r\nlocation: /login\r\n\r\nbody"),
            Ok((302, Some("/login".into())))
        );
        assert_eq!(parse("HTTP/1.1 200 OK\r\n\r\n"), Ok((200, None)));
        assert!(parse("").is_err());
    }

    #[test]
    fn the_last_error_line_of_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("storage").join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("laravel.log"),
            "[2026-11-15 10:00:00] local.ERROR: first\n#0 trace\n[2026-11-15 10:01:00] local.ERROR: Class \"Foo\" not found {\"exception\":...}\n#0 C:\\x.php(1)\n",
        )
        .unwrap();
        assert_eq!(
            last_error(dir.path()).as_deref(),
            Some("local.ERROR: Class \"Foo\" not found {\"exception\":...}")
        );
    }

    #[test]
    fn a_lock_newer_than_the_install_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let modules = dir.path().join("node_modules");
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::write(modules.join(".package-lock.json"), "{}").unwrap();
        assert!(!js_stale(dir.path()));
        let lock = dir.path().join("package-lock.json");
        std::fs::write(&lock, "{}").unwrap();
        let later = SystemTime::now() + Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(js_stale(dir.path()));
    }

    /// Every fix the rules can run is a command the API lets a client run (ADR 0081 §6).
    #[test]
    fn every_fix_the_rules_run_is_allowed() {
        use kiln_daemon::allow::{Mode, check};
        for args in [
            vec!["init", r"C:\Sites\shop"],
            vec!["service", "start", "mysql-8.4"],
            vec!["service", "restart", "mysql-8.4"],
            vec!["db", "create", "mysql-8.4", "shop"],
            vec!["php", "ext", "on", "gmp", "--php", "8.4"],
            vec!["php", "install", "8.4"],
            vec!["web", "start"],
            vec!["env", "use", "mysql-8.4", "--project", r"C:\Sites\shop"],
        ] {
            let args: Vec<String> = args.into_iter().map(String::from).collect();
            assert_eq!(check(&args, Mode::Apply), Ok(()), "{args:?}");
            // The app previews exactly init and env use (SiteCheck.tsx's fixWay); the rest
            // have no preview and are asked about in words.
            let previews = args[0] == "init" || args[..2] == ["env", "use"];
            assert_eq!(
                check(&args, Mode::Preview).is_ok(),
                previews,
                "{args:?} previews"
            );
        }
    }
}
