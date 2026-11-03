//! Reading a Laravel Herd `herd.yml` to migrate it to `kiln.toml` ([ADR 0027] §5). The keys
//! follow Herd's public documentation of the format; nothing of Herd's is reused.
//!
//! [ADR 0027]: ../../../docs/adr/0027-project-config-kiln-toml.md

use yaml_rust2::{Yaml, YamlLoader};

use crate::manifest::{Manifest, ServiceWant};

/// The file's name in a project folder.
pub const FILE: &str = "herd.yml";

/// A `herd.yml` as a [`Manifest`], plus notes on what wasn't carried over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    pub manifest: Manifest,
    pub notes: Vec<String>,
}

/// Reads a `herd.yml`: `name`, `php`, `secured`, `aliases` and `services` carry over
/// (services as Kiln's kinds); anything else, such as `workspaces`, is named in the notes.
pub fn parse(text: &str) -> Result<Imported, String> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    let mut imported = Imported::default();
    let Some(root) = docs.into_iter().next() else {
        return Ok(imported); // an empty file
    };
    let Yaml::Hash(entries) = root else {
        return Err("expected keys such as `php:` at the top".into());
    };
    for (key, value) in entries {
        let Some(key) = key.as_str() else {
            continue;
        };
        match key {
            "name" => imported.manifest.name = Some(scalar(&value, key)?),
            "php" => {
                if matches!(value, Yaml::Real(_) | Yaml::Integer(_)) {
                    imported.notes.push(format!(
                        "php: is written without quotes, so it was read as the number {}; check that this is the version you meant",
                        scalar(&value, key)?
                    ));
                }
                imported.manifest.php = Some(scalar(&value, key)?);
            }
            "secured" => {
                imported.manifest.secure =
                    Some(value.as_bool().ok_or("`secured:` must be true or false")?);
            }
            "aliases" => {
                let Yaml::Array(list) = value else {
                    return Err("`aliases:` must be a list of names".into());
                };
                imported.manifest.aliases = list
                    .iter()
                    .map(|v| scalar(v, key))
                    .collect::<Result<_, _>>()?;
            }
            "services" => {
                let Yaml::Hash(services) = value else {
                    return Err("`services:` must list services by name".into());
                };
                for (name, settings) in services {
                    let Some(name) = name.as_str() else {
                        continue;
                    };
                    if let Some(want) = service(name, &settings, &mut imported.notes) {
                        imported.manifest.services.push(want);
                    }
                }
            }
            "workspaces" => imported
                .notes
                .push("workspaces aren't imported: Kiln has no workspaces yet".into()),
            other => imported
                .notes
                .push(format!("`{other}:` isn't known to Kiln; ignored")),
        }
    }
    Ok(imported)
}

/// Herd's service names as Kiln's kinds ([ADR 0046] §3). MinIO publishes source only now,
/// so its place goes to RustFS, the S3-compatible store Kiln runs.
///
/// [ADR 0046]: ../../../docs/adr/0046-project-services.md
fn kind_of(name: &str) -> Option<&'static str> {
    Some(match name {
        "mysql" => "mysql",
        "mariadb" => "mariadb",
        "postgresql" | "postgres" | "pgsql" => "postgres",
        "redis" => "redis",
        "meilisearch" => "meilisearch",
        "minio" | "rustfs" => "rustfs",
        "mongodb" => "mongodb",
        "mailpit" => "mailpit",
        _ => return None,
    })
}

/// One `services:` entry as a [`ServiceWant`], noting what changed or was left out.
fn service(name: &str, settings: &Yaml, notes: &mut Vec<String>) -> Option<ServiceWant> {
    let Some(kind) = kind_of(name) else {
        notes.push(if name == "reverb" {
            "service reverb isn't imported: Kiln runs Reverb per project in a later version".into()
        } else {
            format!("service {name} isn't imported: Kiln has no {name}")
        });
        return None;
    };
    if name == "minio" {
        notes.push(
            "service minio became rustfs: MinIO is source-only now, and RustFS is S3-compatible"
                .into(),
        );
    }
    // Herd pins a full release (8.0.36); Kiln keeps the line it names (8.0).
    let version = settings["version"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| match &settings["version"] {
            Yaml::Real(r) => Some(r.clone()),
            Yaml::Integer(i) => Some(i.to_string()),
            _ => None,
        })
        .map(|v| {
            v.trim()
                .splitn(3, '.')
                .take(2)
                .collect::<Vec<_>>()
                .join(".")
        })
        .filter(|v| {
            !v.is_empty()
                && v.split('.')
                    .all(|n| n.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty())
        });
    let port = match &settings["port"] {
        Yaml::Integer(p) => u16::try_from(*p).ok().filter(|p| *p >= 1024),
        Yaml::String(s) => {
            let fixed = s.trim().parse::<u16>().ok().filter(|p| *p >= 1024);
            if fixed.is_none() {
                notes.push(format!(
                    "service {name}: port {s} isn't kept; Kiln picks a free port and `kiln env use` writes it to .env"
                ));
            }
            fixed
        }
        _ => None,
    };
    Some(ServiceWant {
        kind: kind.to_owned(),
        version,
        port,
    })
}

/// A string, number or boolean as text (YAML often leaves strings unquoted).
fn scalar(value: &Yaml, key: &str) -> Result<String, String> {
    let text = match value {
        Yaml::String(s) => s.trim().to_owned(),
        Yaml::Real(r) => r.clone(),
        Yaml::Integer(i) => i.to_string(),
        _ => return Err(format!("`{key}:` must be a single value")),
    };
    if text.is_empty() {
        return Err(format!("`{key}:` is empty"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_over_name_php_https_and_aliases_and_notes_the_rest() {
        let imported = parse(
            "name: my-project\nphp: '8.3'\nsecured: true\naliases:\n    - my-project-alias\n\
             services:\n    mysql:\n        version: 8.0.36\n        port: '${DB_PORT}'\n\
             workspaces:\n  database: clone\n",
        )
        .unwrap();
        let m = &imported.manifest;
        assert_eq!(m.name.as_deref(), Some("my-project"));
        assert_eq!(m.php.as_deref(), Some("8.3"));
        assert_eq!(m.secure, Some(true));
        assert_eq!(m.aliases, ["my-project-alias"]);
        assert_eq!(
            m.services,
            [ServiceWant {
                kind: "mysql".into(),
                version: Some("8.0".into()),
                port: None
            }]
        );
        assert_eq!(imported.notes.len(), 2, "{:?}", imported.notes);
        assert!(
            imported.notes[0].contains("isn't kept"),
            "the DB_PORT placeholder"
        );
        assert!(imported.notes[1].contains("workspaces"));
    }

    #[test]
    fn herds_services_become_kilns_kinds() {
        let imported = parse(
            "services:
  postgresql:
    version: 17.2
  minio:
    version: '2024'
  redis:
    port: 6390
  reverb:
    version: 1
  typesense:
    version: 27
",
        )
        .unwrap();
        let kinds: Vec<_> = imported
            .manifest
            .services
            .iter()
            .map(|s| (s.kind.as_str(), s.version.as_deref(), s.port))
            .collect();
        assert_eq!(
            kinds,
            [
                ("postgres", Some("17.2"), None),
                ("rustfs", Some("2024"), None),
                ("redis", None, Some(6390)),
            ]
        );
        let notes = imported.notes.join(
            "
",
        );
        assert!(notes.contains("minio became rustfs"), "{notes}");
        assert!(notes.contains("reverb"), "{notes}");
        assert!(notes.contains("Kiln has no typesense"), "{notes}");
    }

    #[test]
    fn an_unquoted_php_version_is_kept_but_flagged() {
        let imported = parse("php: 8.4\n").unwrap();
        assert_eq!(imported.manifest.php.as_deref(), Some("8.4"));
        assert!(imported.notes[0].contains("without quotes"));
    }

    #[test]
    fn bad_files_are_refused_with_the_reason() {
        assert!(parse("- a\n- b\n").is_err(), "not a mapping");
        assert!(parse("secured: maybe\n").is_err());
        assert!(parse("aliases: one\n").is_err());
        assert!(parse("php: [\n").is_err(), "not YAML");
        assert_eq!(parse("").unwrap(), Imported::default());
    }
}
