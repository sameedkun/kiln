//! One-line, human descriptions of journaled operations, for `kiln history`, `kiln undo` and
//! `kiln uninstall` ([ADR 0018] §1–3). Built from the stored parameters only, so they work
//! for any journal entry, including ones whose target has changed since.
//!
//! [ADR 0018]: ../../../docs/adr/0018-history-undo-uninstall.md

use serde_json::Value;

use crate::autostart::AUTOSTART_KIND;
use crate::hosts_op::HOSTS_BLOCK_KIND;
use crate::shell_profile::SHELL_PROFILE_KIND;
use crate::trust_nss::NSS_TRUST_KIND;
use crate::trust_system::SYSTEM_TRUST_KIND;
use crate::user_path::USER_PATH_KIND;

/// Same value as `trust::TRUST_CA_KIND`, which only exists on Windows.
const TRUST_KIND: &str = "trust.ca.user";

/// What an operation of `kind` with `params` changed, in a few words.
#[must_use]
pub fn describe(kind: &str, params: &Value) -> String {
    match kind {
        HOSTS_BLOCK_KIND => {
            let names: Vec<&str> = params["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e.get(1).and_then(Value::as_str))
                .collect();
            if names.is_empty() {
                "hosts file: Kiln's block removed".to_owned()
            } else {
                format!("hosts file: {}", list(&names, 6))
            }
        }
        TRUST_KIND => format!(
            "trusted for your account: {}",
            params["subject"].as_str().unwrap_or("Kiln's root CA")
        ),
        USER_PATH_KIND => format!(
            "{} first on your user PATH",
            params["entry"].as_str().unwrap_or("?")
        ),
        NSS_TRUST_KIND => format!(
            "Kiln's root CA trusted in {}",
            params["label"].as_str().unwrap_or("a browser")
        ),
        SHELL_PROFILE_KIND => format!(
            "Kiln's shims on PATH in {}",
            params["file"].as_str().unwrap_or("a shell startup file")
        ),
        SYSTEM_TRUST_KIND => "Kiln's root CA trusted machine-wide (system store)".to_owned(),
        AUTOSTART_KIND => format!(
            "Kiln's daemon starts at login ({})",
            params["location"].as_str().unwrap_or("login items")
        ),
        other => other.to_owned(),
    }
}

/// `a, b, c and 4 more`.
fn list(items: &[&str], max: usize) -> String {
    if items.len() <= max {
        items.join(", ")
    } else {
        format!("{} and {} more", items[..max].join(", "), items.len() - max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_kind_reads_naturally() {
        assert_eq!(
            describe(
                HOSTS_BLOCK_KIND,
                &json!({"tld": "test", "entries": [["127.0.0.1", "blog.test"], ["127.0.0.1", "shop.test"]]})
            ),
            "hosts file: blog.test, shop.test"
        );
        assert_eq!(
            describe(HOSTS_BLOCK_KIND, &json!({"tld": "test", "entries": []})),
            "hosts file: Kiln's block removed"
        );
        assert_eq!(
            describe(
                TRUST_KIND,
                &json!({"subject": "CN=Kiln Local Development CA (x)"})
            ),
            "trusted for your account: CN=Kiln Local Development CA (x)"
        );
        assert_eq!(
            describe(USER_PATH_KIND, &json!({"entry": r"C:\Kiln\bin\shims"})),
            r"C:\Kiln\bin\shims first on your user PATH"
        );
        assert_eq!(
            describe(NSS_TRUST_KIND, &json!({"label": "Firefox profile x"})),
            "Kiln's root CA trusted in Firefox profile x"
        );
        assert!(describe(SYSTEM_TRUST_KIND, &json!({})).contains("machine-wide"));
        assert_eq!(
            describe(SHELL_PROFILE_KIND, &json!({"file": "/home/me/.bashrc"})),
            "Kiln's shims on PATH in /home/me/.bashrc"
        );
        assert_eq!(describe("future.kind", &json!({})), "future.kind");
    }

    #[test]
    fn long_lists_are_shortened() {
        let entries: Vec<_> = (0..9)
            .map(|i| json!(["127.0.0.1", format!("s{i}.test")]))
            .collect();
        let text = describe(HOSTS_BLOCK_KIND, &json!({"entries": entries}));
        assert!(text.ends_with("s5.test and 3 more"), "{text}");
    }
}
