//! Reading a Laravel Herd `herd.yml` to migrate it to `kiln.toml` ([ADR 0027] §5). The keys
//! follow Herd's public documentation of the format; nothing of Herd's is reused.
//!
//! [ADR 0027]: ../../../docs/adr/0027-project-config-kiln-toml.md

use yaml_rust2::{Yaml, YamlLoader};

use crate::manifest::Manifest;

/// The file's name in a project folder.
pub const FILE: &str = "herd.yml";

/// A `herd.yml` as a [`Manifest`], plus notes on what wasn't carried over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    pub manifest: Manifest,
    pub notes: Vec<String>,
}

/// Reads a `herd.yml`: `name`, `php`, `secured` and `aliases` carry over; `services` and
/// `workspaces` are named in the notes.
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
                let names: Vec<String> = value
                    .as_hash()
                    .map(|h| {
                        h.keys()
                            .filter_map(|k| k.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                imported.notes.push(format!(
                    "services ({}) aren't imported: Kiln adds services in a later version",
                    if names.is_empty() {
                        "none listed".into()
                    } else {
                        names.join(", ")
                    }
                ));
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
        assert_eq!(imported.notes.len(), 2, "{:?}", imported.notes);
        assert!(imported.notes[0].contains("mysql"));
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
