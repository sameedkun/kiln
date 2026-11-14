//! `kiln mcp` ([ADR 0066]): a Model Context Protocol server over standard input and output, so
//! an AI assistant can read Kiln's sites, services, logs and history, and, when the user
//! allows it, start, stop and restart services and undo changes.
//!
//! [ADR 0066]: ../../../docs/adr/0066-mcp-server.md

use std::io::{BufRead, Write};
use std::process::Command;

use serde_json::{Value, json};

/// `on` when the change tools are offered; off (unset) by default.
pub const ALLOW_CHANGES_KEY: &str = "mcp.allow_changes";

/// The protocol version answered when the client asks for one Kiln doesn't know.
const PROTOCOL: &str = "2025-06-18";

/// Whether the user allowed changes.
pub fn changes_allowed() -> bool {
    crate::web::open_store()
        .ok()
        .and_then(|s| s.setting(ALLOW_CHANGES_KEY).ok().flatten())
        .is_some_and(|v| v == "on")
}

fn tool(name: &str, description: &str, properties: &Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// The tools offered: the reads always, the changes only when allowed.
#[must_use]
pub fn tools(changes: bool) -> Vec<Value> {
    let none = json!({});
    let mut all = vec![
        tool(
            "kiln_sites",
            "Kiln's sites: address, PHP and Node versions, HTTPS, project folder and framework.",
            &none,
            &[],
        ),
        tool(
            "kiln_services",
            "Kiln's service instances (databases, Redis, Mailpit…): ports, whether they run, and which sites use them.",
            &none,
            &[],
        ),
        tool(
            "kiln_logs",
            "Without `source`: every log Kiln knows of (its daemon, the web server, PHP errors, each service, each Laravel site). With `source` (an id from that list): its last lines.",
            &json!({
                "source": { "type": "string", "description": "A log's id, e.g. web, php, service:mysql-8.4, site:shop.test" },
                "lines": { "type": "integer", "description": "How many lines (default 100)" },
            }),
            &[],
        ),
        tool(
            "kiln_requests",
            "How fast each site answers, from the web server's access log: requests, failures, median, slowest 5%, slowest requests.",
            &json!({ "site": { "type": "string", "description": "One site, e.g. shop.test" } }),
            &[],
        ),
        tool(
            "kiln_history",
            "What Kiln changed on this PC, newest first, with each operation's id.",
            &none,
            &[],
        ),
        tool(
            "kiln_doctor",
            "Kiln's health checks: ports, the web server, PHP, sites, the hosts file, services; each problem with its fix.",
            &none,
            &[],
        ),
    ];
    if changes {
        let name = json!({ "name": { "type": "string", "description": "A service instance, as kiln_services lists it" } });
        all.push(tool(
            "kiln_service_start",
            "Start a service instance.",
            &name,
            &["name"],
        ));
        all.push(tool(
            "kiln_service_stop",
            "Stop a service instance.",
            &name,
            &["name"],
        ));
        all.push(tool(
            "kiln_service_restart",
            "Restart a service instance.",
            &name,
            &["name"],
        ));
        all.push(tool(
            "kiln_undo",
            "Undo one operation from kiln_history, as the user could in Kiln's History.",
            &json!({ "id": { "type": "integer", "description": "The operation's id" } }),
            &["id"],
        ));
    }
    all
}

/// A plain name for the command line: letters, digits, `.`, `-`, `_`, `:`, not starting with
/// `-`, so it can't become an option.
fn plain(value: &str) -> Result<&str, String> {
    if !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/'))
    {
        Ok(value)
    } else {
        Err(format!("{value:?} isn't a plain name"))
    }
}

/// The `kiln` arguments a tool call runs, or why not.
pub fn command_for(name: &str, args: &Value, changes: bool) -> Result<Vec<String>, String> {
    let text = |key: &str| args[key].as_str();
    let mut command: Vec<String> = match name {
        "kiln_sites" => vec!["sites".into()],
        "kiln_services" => vec!["service".into(), "list".into()],
        "kiln_logs" => {
            let mut c = vec!["logs".to_owned()];
            if let Some(source) = text("source") {
                c.push(plain(source)?.to_owned());
                let lines = args["lines"].as_u64().unwrap_or(100).clamp(1, 2000);
                c.extend(["--lines".to_owned(), lines.to_string()]);
            }
            c
        }
        "kiln_requests" => {
            let mut c = vec!["requests".to_owned()];
            if let Some(site) = text("site") {
                c.push(plain(site)?.to_owned());
            }
            c
        }
        "kiln_history" => vec!["history".into()],
        "kiln_doctor" => vec!["doctor".into()],
        "kiln_service_start" | "kiln_service_stop" | "kiln_service_restart" if changes => {
            let action = name.trim_start_matches("kiln_service_");
            let instance = text("name").ok_or("`name` is missing")?;
            vec!["service".into(), action.into(), plain(instance)?.into()]
        }
        "kiln_undo" if changes => {
            let id = args["id"].as_u64().ok_or("`id` (a number) is missing")?;
            vec!["--confirmed".into(), "undo".into(), id.to_string()]
        }
        "kiln_service_start" | "kiln_service_stop" | "kiln_service_restart" | "kiln_undo" => {
            return Err(
                "changes through Kiln's MCP server are off; the user turns them on with `kiln mcp allow-changes on`".into(),
            );
        }
        other => return Err(format!("no tool called {other}")),
    };
    command.insert(0, "--json".into());
    Ok(command)
}

/// Runs `kiln <args>` and gives its result's data (or its error) as text.
fn run_kiln(args: &[String]) -> (String, bool) {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return (e.to_string(), true),
    };
    let output = match Command::new(exe).args(args).output() {
        Ok(output) => output,
        Err(e) => return (format!("cannot run kiln: {e}"), true),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let last = stdout.lines().rev().find_map(|l| {
        serde_json::from_str::<Value>(l)
            .ok()
            .filter(|v| v["type"] == "result" || v["type"] == "error")
    });
    match last {
        Some(v) if v["type"] == "result" => (
            serde_json::to_string_pretty(&v["data"]).unwrap_or_default(),
            false,
        ),
        Some(v) => (
            v["message"].as_str().unwrap_or("kiln failed").to_owned(),
            true,
        ),
        None => (
            format!(
                "kiln gave no result:\n{}{}",
                stdout,
                String::from_utf8_lossy(&output.stderr)
            ),
            true,
        ),
    }
}

/// The answer to one JSON-RPC message, or `None` for a notification.
pub fn handle(
    message: &Value,
    changes: bool,
    run: &mut dyn FnMut(&[String]) -> (String, bool),
) -> Option<Value> {
    let id = message.get("id")?.clone();
    let method = message["method"].as_str().unwrap_or_default();
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "kiln", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Kiln runs this PC's local PHP sites (name.test) and their services. Read sites, services, logs, request timing, history and the doctor's checks. When the user allowed changes, services can be started, stopped and restarted, and operations from the history undone.",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools(changes) })),
        "tools/call" => {
            let name = message["params"]["name"].as_str().unwrap_or_default();
            let args = &message["params"]["arguments"];
            let (text, is_error) = match command_for(name, args, changes) {
                Ok(command) => run(&command),
                Err(e) => (e, true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        other => Err(json!({ "code": -32601, "message": format!("no method {other}") })),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    })
}

/// `kiln mcp`: serves until standard input closes.
pub fn serve() -> Result<(), String> {
    let changes = changes_allowed();
    eprintln!(
        "kiln mcp: serving on standard input and output; changes are {}",
        if changes {
            "allowed"
        } else {
            "off (`kiln mcp allow-changes on`)"
        }
    );
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(&message, changes, &mut run_kiln),
            Err(e) => Some(json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("not JSON: {e}") },
            })),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}").map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// `kiln mcp allow-changes on|off`.
pub fn set_allow_changes(on: bool) -> Result<(), String> {
    let mut store = crate::web::open_store()?;
    store
        .set_setting(ALLOW_CHANGES_KEY, if on { "on" } else { "off" })
        .map_err(|e| e.to_string())?;
    if on {
        say!(
            "AI assistants using `kiln mcp` may start, stop and restart services and undo changes from History. Restart the assistant's MCP connection to see the new tools."
        );
    } else {
        say!("AI assistants using `kiln mcp` can only read.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(message: &Value, changes: bool) -> (Option<Value>, Vec<Vec<String>>) {
        let mut ran = Vec::new();
        let reply = handle(message, changes, &mut |args: &[String]| {
            ran.push(args.to_vec());
            ("{}".to_owned(), false)
        });
        (reply, ran)
    }

    #[test]
    fn it_introduces_itself_and_lists_reads_only_by_default() {
        let (reply, _) = call(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}),
            false,
        );
        let reply = reply.unwrap();
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(reply["result"]["serverInfo"]["name"], "kiln");
        let (reply, _) = call(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            false,
        );
        let names: Vec<_> = reply.unwrap()["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect();
        assert!(names.contains(&"kiln_sites".to_owned()));
        assert!(
            !names
                .iter()
                .any(|n| n.contains("start") || n.contains("undo"))
        );
        let (reply, _) = call(
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            false,
        );
        assert!(reply.is_none(), "notifications get no answer");
    }

    #[test]
    fn tools_run_kiln_with_json_and_checked_arguments() {
        let (reply, ran) = call(
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"kiln_logs","arguments":{"source":"service:mysql-8.4","lines":5}}}),
            false,
        );
        assert_eq!(reply.unwrap()["result"]["isError"], false);
        assert_eq!(
            ran,
            [["--json", "logs", "service:mysql-8.4", "--lines", "5"]]
        );
        // An argument can't become an option.
        let (reply, ran) = call(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"kiln_requests","arguments":{"site":"--all"}}}),
            false,
        );
        assert_eq!(reply.unwrap()["result"]["isError"], true);
        assert_eq!(ran.len(), 0);
    }

    #[test]
    fn changes_need_the_users_permission() {
        let start = json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"kiln_service_start","arguments":{"name":"mysql-8.4"}}});
        let (reply, ran) = call(&start, false);
        assert!(
            reply.unwrap()["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("allow-changes on")
        );
        assert_eq!(ran.len(), 0);
        let (_, ran) = call(&start, true);
        assert_eq!(ran, [["--json", "service", "start", "mysql-8.4"]]);
        let (_, ran) = call(
            &json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"kiln_undo","arguments":{"id":12}}}),
            true,
        );
        assert_eq!(ran, [["--json", "--confirmed", "undo", "12"]]);
    }

    #[test]
    fn unknown_methods_are_errors() {
        let (reply, _) = call(
            &json!({"jsonrpc":"2.0","id":7,"method":"resources/list"}),
            false,
        );
        assert_eq!(reply.unwrap()["error"]["code"], -32601);
    }
}
