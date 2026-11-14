//! `kiln new <name> --template wordpress --db <instance>` ([ADR 0070], amended in P4D-07):
//! `WordPress` from wordpress.org, as its own install guide has it (download, unpack, a
//! database, `wp-config.php`), set up so the site opens on `WordPress`'s installer.
//!
//! [ADR 0070]: ../../../docs/adr/0070-new-site-templates.md

use std::fmt::Write as _;
use std::path::Path;

/// `WordPress` 7.1.4, pinned by SHA-256 computed on a runner by the pin workflow (wordpress.org
/// publishes SHA-1; CI checks that too).
pub const RELEASE: kiln_services::Release = kiln_services::Release {
    version: "7.1.4",
    os: "any",
    arch: "any",
    archive: "wordpress-7.1.4.zip",
    url: "https://downloads.wordpress.org/release/wordpress-7.1.4.zip",
    sha256: "a147146c159362778ecbbbfb07bb20ddf4e85b130f971c824e656a8f938c5f87",
    executable: "index.php",
    tree: Some("wordpress"),
};

/// A database name for the project: its name, `-` as `_` (`my-blog` is `my_blog`).
#[must_use]
pub fn database_name(project: &str) -> String {
    project.replace('-', "_")
}

/// 64 random printable characters for one of `WordPress`'s keys and salts (no quote or
/// backslash, so it sits in a PHP string as it is).
fn salt() -> Result<String, String> {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()-_=+[]{}<>~;:,.?/|";
    let mut bytes = [0u8; 64];
    getrandom::fill(&mut bytes).map_err(|e| format!("no random source: {e}"))?;
    Ok(bytes
        .iter()
        .map(|b| CHARS[usize::from(*b) % CHARS.len()] as char)
        .collect())
}

/// `wp-config.php` from `WordPress`'s own `wp-config-sample.php`: the database, its login and
/// address, and fresh keys and salts.
pub fn config(
    sample: &str,
    database: &str,
    user: &str,
    password: &str,
    host: &str,
) -> Result<String, String> {
    let mut text = sample
        .replace("'database_name_here'", &format!("'{database}'"))
        .replace("'username_here'", &format!("'{user}'"))
        .replace("'password_here'", &format!("'{password}'"))
        .replace("'localhost'", &format!("'{host}'"));
    while let Some(at) = text.find("'put your unique phrase here'") {
        let mut quoted = String::new();
        let _ = write!(quoted, "'{}'", salt()?);
        text.replace_range(at..at + "'put your unique phrase here'".len(), &quoted);
    }
    Ok(text)
}

/// `WordPress` unpacked into `project`, its database made on `instance`, `wp-config.php`
/// written. `project` doesn't exist yet.
pub fn create(project: &Path, instance: &str) -> Result<(), String> {
    let store = crate::web::open_store()?;
    let record = crate::service::record_of(&store, instance)?;
    drop(store);
    if !matches!(record.kind.as_str(), "mysql" | "mariadb") {
        return Err(format!(
            "WordPress runs on MySQL or MariaDB; {instance} is {}",
            record.kind
        ));
    }
    let provider = crate::service::provider_of(&record.kind)?;
    let dirs = crate::service::dirs_of(&record)?;

    let archive = crate::service::download(&RELEASE)?;
    crate::out::step("Unpacking WordPress");
    let parent = project.parent().ok_or("no folder to make the project in")?;
    let staging = parent.join(format!(".kiln-wordpress-{}", std::process::id()));
    let unpacked = kiln_services::install_into_folder(&RELEASE, &archive, &staging)
        .map_err(|e| e.to_string())?;
    let tree = unpacked
        .parent()
        .ok_or("unpacked into nothing")?
        .to_path_buf();
    // Beside the project, so the same drive: a move, not a copy.
    kiln_sys::rename_replace(&tree, project)
        .map_err(|e| format!("cannot move WordPress into {}: {e}", project.display()))?;
    let _ = std::fs::remove_dir_all(&staging);
    say!(
        "Unpacked WordPress {} into {}.",
        RELEASE.version,
        project.display()
    );

    crate::out::step("Making its database");
    let name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let database = database_name(&name);
    match provider.create_database(&dirs, &record.ports, &database) {
        Some(Ok(true)) => say!("Created the database {database} on {instance}."),
        Some(Ok(false)) => say!("{instance} already has a database {database}; WordPress uses it."),
        Some(Err(e)) => return Err(format!("cannot make the database {database}: {e}")),
        None => return Err(format!("{instance} has no databases")),
    }
    let keys = provider.env_keys(&record.ports);
    let key = |k: &str| {
        keys.iter()
            .find(|(name, _)| *name == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let host = format!("{}:{}", key("DB_HOST"), key("DB_PORT"));
    let sample = std::fs::read_to_string(project.join("wp-config-sample.php"))
        .map_err(|e| format!("cannot read wp-config-sample.php: {e}"))?;
    let text = config(
        &sample,
        &database,
        &key("DB_USERNAME"),
        &key("DB_PASSWORD"),
        &host,
    )?;
    std::fs::write(project.join("wp-config.php"), text)
        .map_err(|e| format!("cannot write wp-config.php: {e}"))?;
    say!("Wrote wp-config.php: the database {database} on {instance} ({host}), with fresh keys.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "define( 'DB_NAME', 'database_name_here' );\n\
        define( 'DB_USER', 'username_here' );\n\
        define( 'DB_PASSWORD', 'password_here' );\n\
        define( 'DB_HOST', 'localhost' );\n\
        define( 'AUTH_KEY',         'put your unique phrase here' );\n\
        define( 'NONCE_SALT',       'put your unique phrase here' );\n";

    #[test]
    fn wp_config_gets_the_database_and_fresh_keys() {
        let text = config(SAMPLE, "my_blog", "root", "", "127.0.0.1:3306").unwrap();
        assert!(text.contains("'DB_NAME', 'my_blog'"));
        assert!(text.contains("'DB_USER', 'root'"));
        assert!(text.contains("'DB_PASSWORD', ''"));
        assert!(text.contains("'DB_HOST', '127.0.0.1:3306'"));
        assert!(!text.contains("put your unique phrase here"));
        assert!(!text.contains('\\'));
        let keys: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("_KEY") || l.contains("_SALT"))
            .collect();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0].split('\'').nth(3), keys[1].split('\'').nth(3));
    }

    #[test]
    fn a_project_name_is_a_database_name() {
        assert_eq!(database_name("my-blog"), "my_blog");
    }
}
