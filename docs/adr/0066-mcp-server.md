# 0066. An MCP server for AI assistants: `kiln mcp`

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4-19 · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0053](0053-api-v2-and-json-output.md)

## Context

AI assistants in editors and terminals (Claude Code, Cursor, VS Code and others) can use local
tools through the Model Context Protocol (MCP): the assistant starts a program and talks
JSON-RPC to it over standard input and output. Herd offers one for its sites. ADR 0052 put
Kiln's in Phase 4: "reads sites, services, logs and history; changes through the CLI, in
History; off by default".

`kiln --json` already gives every read as data (ADR 0053), and every change Kiln makes goes
through its journal, so History shows it and can undo it.

## Decision

1. **`kiln mcp`** is an MCP server over standard input and output (newline-delimited JSON-RPC
   2.0): `initialize`, `ping`, `tools/list` and `tools/call`. It writes nothing else to
   standard output; notes go to standard error.
2. **Its tools read** by running `kiln --json …` and returning the result's data as text:
   - `kiln_sites`: the sites with their PHP, Node, HTTPS and folders;
   - `kiln_services`: the service instances, ports, state and which sites use them;
   - `kiln_logs`: the logs Kiln knows of, or one log's last lines (`source`, `lines`);
   - `kiln_requests`: request timing per site (`site`);
   - `kiln_history`: what Kiln changed;
   - `kiln_doctor`: the doctor's findings.
3. **Changes are off until the user allows them** with `kiln mcp allow-changes on`
   (`mcp.allow_changes`). Only then does the server list the change tools:
   - `kiln_service_start`, `kiln_service_stop`, `kiln_service_restart` (`name`);
   - `kiln_undo` (`id`): undoes one operation, as the user could in History.
   Each runs the same `kiln` command the user would, under the same lock. Starting and
   stopping a service is a state Kiln keeps, not a journaled operation, so it shows on the
   Services page rather than in History; an undo is recorded in History like any other. A
   tool's arguments are checked (an instance's name, a number) before they reach the command
   line, so an argument can't become an option.
4. **Off by default:** Kiln registers itself with no assistant. The user adds `kiln mcp` to
   their assistant's settings; the app's Settings shows the line to add and the
   allow-changes switch.

## Consequences

- An assistant can see why a site fails (its log, the doctor, which services it uses) without
  the user copying output.
- Nothing an assistant does is hidden: its changes are the user's own commands, visible where
  those show (the Services page, History), and off unless allowed.
- More change tools (linking, PHP versions) can be added the same way later.
