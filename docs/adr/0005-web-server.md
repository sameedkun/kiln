# 0005. Web server: Caddy, driven through its JSON admin API

- **Status:** accepted
- **Date:** 2026-10-20
- **Evidence:** [spike 01 results](../spikes/results/01-web-server.md)

## Context

Kiln needs a reverse proxy and FastCGI front end that:

- serves PHP through `php-cgi` pools on Windows (no PHP-FPM there) and PHP-FPM on Unix;
- terminates TLS with certificates from Kiln's own name-constrained local CA;
- reloads configuration without dropping in-flight requests when sites change;
- runs unprivileged on loopback 80/443.

Spike 01 compared nginx 1.30.6 and Caddy 2.11.7 on Windows. Both reached zero errors on
reloads, worker crashes and worker recycling once tuned. Caddy was 2–25× faster on static
files, had better PHP tail latency, and can issue leaf certificates from our own root. nginx is
smaller and more familiar.

## Decision

1. Kiln uses **Caddy** as its web server on all platforms, behind the `WebServer` trait so
   nginx remains a possible alternative backend.
2. Kiln generates **Caddy JSON config from typed Rust structs** and applies it through the
   admin API (`POST /load`), not through Caddyfiles. Users can add per-site Caddyfile snippets,
   which Kiln adapts and merges.
3. Required settings, enforced by snapshot tests on the generated config:
   - every listener bound to loopback (`127.0.0.1` / `::1`), via `default_bind` or per-listener;
   - `skip_install_trust`: Caddy never touches OS trust stores; Kiln's `TrustStore` adapter
     does;
   - the PKI app uses Kiln's name-constrained root. Leaf issuance and renewal are left to
     Caddy;
   - FastCGI upstreams without passive quarantine: no `fail_duration`, and retries bounded by
     `lb_try_duration` (≈2 s) and `lb_try_interval`, not by `lb_retries`;
   - an explicit catch-all route returning 404 for unknown hosts;
   - the admin endpoint is not reachable by other local users (see follow-ups).
4. Kiln ships the binary under a Kiln-specific image name (e.g. `kiln-caddy.exe`). Other tools
   kill server processes by image name; Herd does this for `nginx.exe` at startup.
5. The process supervisor owns php-cgi recycling and staggers it across workers.

## Consequences

- No Kiln-side leaf certificate issuance or renewal code, and no reload signalling.
- A larger download (~51 MB binary vs 6 MB), accepted.
- Users who expect nginx config snippets get Caddyfile snippets instead. The docs need a
  translation guide for common nginx rules.
- A Caddy upgrade is a dependency upgrade. Pin versions and verify checksums (and cosign
  signatures once `cosign` verification is automated).

## Follow-ups

- Verify Caddy admin over a Unix socket on Windows. Otherwise use a random loopback port plus
  `enforce_origin` and an OS-level guard. Tracked for Phase 1.
- Translation guide: common nginx snippets → Caddyfile.
- Integration tests: WordPress, Unicode and long-path document roots, HTTP/3.

## Alternatives considered

- **nginx** (80 points vs 93): familiar and small, but slow on Windows for large files
  (`select()`, a single effective worker), needs Kiln-side certificate issuance, and needs
  text-template generation plus signal-based reloads.
