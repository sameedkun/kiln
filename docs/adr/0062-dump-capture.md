# 0062. Dump capture with Symfony's dump server

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-11 · Builds on [ADR 0045](0045-env-wiring.md), [0059](0059-log-viewer.md), [0060](0060-dev-processes.md)

## Context

`dump()` in a web request prints into the page, where it breaks JSON responses, redirects and
layouts. Herd Pro collects dumps in a window of their own. The plan asks for `dump()` and
`dd()` output "collected and shown, by an open-source collector chosen in a spike".

The candidates:

- **Symfony VarDumper's dump server** (`var-dump-server`, MIT). Every Laravel project has it:
  `laravel/framework` requires `symfony/var-dumper`, and Laravel's `dump()` and `dd()` are
  VarDumper's. With `VAR_DUMPER_FORMAT=server` and `VAR_DUMPER_SERVER=<host:port>` in the
  environment, VarDumper sends each dump to the server instead of the output, with where it
  came from (file, line, request), and falls back to the output when no server answers.
  Laravel leaves VarDumper's own handler in place for that format. The server prints each
  dump as text (`--format=cli`) or HTML.
- **A collector of Kiln's own** speaking VarDumper's protocol. Rejected for now: the dumps
  arrive as serialized PHP objects, so it would need PHP to read them anyway.
- **Others** (Buggregator, Ray's server): larger programs to pin and download, for the same
  data.

## Decision

1. **The dump server runs in the project, as a dev process** ([ADR 0060](0060-dev-processes.md)):
   a new kind, `dumps`, runs `php vendor/symfony/var-dumper/Resources/bin/var-dump-server
   --host=127.0.0.1:<port> --format=cli` in the project with its PHP. Port 9912 (VarDumper's
   own default), or the next free one. Without the server in `vendor`, it refuses and says
   to run `composer install` (or `composer require --dev symfony/var-dumper`).
2. **Its log is the collection.** Each dump, with its source and request, goes into the
   instance's log, so the Logs page shows and follows it; a site's page gets a **Dumps** tab
   with the same view.
3. **The project's `.env` sends dumps there**, through the usual wiring and its preview
   ([ADR 0045](0045-env-wiring.md)): `VAR_DUMPER_FORMAT=server` and
   `VAR_DUMPER_SERVER=127.0.0.1:<port>`. Starting Dumps in the app asks for that change right
   after. Undo puts the `.env` back; with the server stopped, dumps go back into the page.
4. **`dd()` still ends the request**: it dumps to the server, then stops, which is what a
   developer asked for.

## Consequences

- Dumps no longer break pages and API responses, and they are kept (the log rolls with the
  others).
- It works for any project using VarDumper, not only Laravel, but the app offers it on
  Laravel sites, where it is known to be there.
- The Scenario is the spike's evidence: with Dumps on and wired, a route calling `dump()`
  returns its page without the dump, and the dump appears in the dump server's log.
- Showing dumps as HTML (collapsible) can come later, with `--format=html` and a viewer.
