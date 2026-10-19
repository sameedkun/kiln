# 0005. Web server: nginx or Caddy

- **Status:** proposed, pending [spike 01](../spikes/01-web-server.md)
- **Date:** 2026-10-19

## Context

Kiln needs a reverse proxy and FastCGI front end that:

- serves PHP through `php-cgi` pools on Windows (no PHP-FPM there) and PHP-FPM on Unix;
- terminates TLS with certificates from Kiln's own name-constrained local CA;
- reloads configuration without dropping in-flight requests when sites change;
- runs unprivileged on loopback 80/443 (Linux needs `cap_net_bind_service`).

**nginx** is what PHP developers know, and users can paste familiar snippets. Kiln would
generate config files, issue certificates itself and signal reloads.

**Caddy** (Apache-2.0) has a built-in PKI app, live configuration through a JSON admin API,
and HTTP/3. It could replace our reload and certificate plumbing, but its snippet syntax is
less familiar.

Both will sit behind the `WebServer` trait whichever is chosen, so the other remains possible
later.

## Decision

To be made from the spike's measurements against the criteria in spike 01.

## Consequences

To be filled in when accepted.
