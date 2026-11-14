# 0065. Request timing per site, from Caddy's access log

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4-18 · Builds on [ADR 0052](0052-phase-4-screens-and-scope.md), [0059](0059-log-viewer.md)

## Context

ADR 0052 planned a site's Requests view: "median, slowest 5%, counts, failures, slowest
requests; kept on the PC". Caddy can write an access log: one JSON line per request, with
the host, method, URI, status, response size and how long it took. Kiln's config doesn't
turn it on yet, so nothing records requests.

An access log can hold what requests carry. Caddy leaves out credentials by default
(`Cookie`, `Set-Cookie`, `Authorization`, `Proxy-Authorization` are redacted), but URIs keep
their query strings. It stays in Kiln's folder, readable by the user only, and is never sent
anywhere.

## Decision

1. **Caddy writes an access log**: every server names the logger `kiln_access`, and the log
   `access` writes those lines as JSON to `access.log` next to `caddy.log`, rolled at 10 MB
   with 3 kept, as Caddy's own log. The default log excludes them, so `caddy.log` keeps
   only the web server's own messages.
2. **`kiln requests [site]`** (a read; `requests.get` with an optional `host`) reads the
   current `access.log` and answers, per site (the request's host without its port):
   - `count`, and `failures`: responses with status 500 or above;
   - `median_ms` and `p95_ms`: the median and the slowest 5% boundary of the durations;
   - `slowest`: the ten slowest requests (time, method, URI, status, milliseconds);
   - `since`: the time of the oldest request read.
   Lines it can't read are skipped. The API checks a host is a plain name before it reaches
   the command line.
3. **The Logs page lists the access log** as "Web server (requests)", followed like the others.
4. **A site's page gets a Requests tab**: count, failures, median and slowest 5% as figures,
   and the slowest requests as a list, with Read again.
5. **It stays on the PC**: no request data leaves Kiln's folder, and `kiln uninstall` removes it
   with the rest.

## Consequences

- A slow page shows as numbers, not a feeling, without a profiler.
- Rolled files aren't read: the figures cover the current file, up to 10 MB of requests, and
  say from when.
- A per-route breakdown and a chart over time can come later from the same lines.
