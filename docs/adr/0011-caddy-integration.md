# 0011. Caddy integration: generated config, HTTP by default, HTTPS per site

- **Status:** accepted
- **Date:** 2026-10-21
- **Task:** P1-06 · Builds on [ADR 0005](0005-web-server.md), [0010](0010-local-certificate-authority.md)

## Context

[ADR 0005](0005-web-server.md) chose Caddy, driven by JSON generated from typed Rust structs.
P1-06 turns that into running code: Kiln has to get a trusted Caddy binary, describe the
user's sites to it, start and stop it, and hand it Kiln's CA when a site needs HTTPS.

Requirements added for P1-06:

- **HTTP is the default.** A linked site is served at `http://<name>.test`. HTTPS is opt-in per
  site (`kiln secure <name>`), because many projects never need it, and a site without HTTPS
  should not depend on the CA being created and trusted.
- Kiln must run alongside Laravel Herd, which already listens on ports 80 and 443 on the dev
  machine ([I-11](../issues.md#i-11)).

Spike 01 already established the Caddy settings Kiln needs ([I-15](../issues.md#i-15) to
[I-18](../issues.md#i-18)).

## Decision

### Sites

1. A **site** is a name (`blog` → `blog.test`), a document root, and a `secure` flag that
   defaults to **false**. Sites live in a new `sites` table in the state database
   (migration v2). P1-06 adds `kiln link`, `kiln unlink`, `kiln secure`, `kiln unsecure` and
   `kiln sites`. P1-10 builds on this with `park` and framework drivers.
2. Until PHP lands (P1-08/P1-09), a site serves static files from its root.
3. DNS stays separate: `kiln hosts sync` puts every linked site into Kiln's hosts block (one
   UAC prompt). Linking a site doesn't trigger a UAC prompt on its own.

### Generated config (new crate `kiln-web`)

4. `kiln-web` renders Caddy JSON from typed structs and has no state-database knowledge.
   Tests assert the invariants below on the generated JSON:
   - **Loopback only:** every listener is `127.0.0.1:<port>` or `[::1]:<port>` ([I-16](../issues.md#i-16)).
   - **Catch-all 404** on every server, for unknown hosts ([I-17](../issues.md#i-17)).
   - **`automatic_https.disable`** on every server. Kiln decides which sites are HTTPS and
     writes the redirects itself, so Caddy never invents listeners, redirects or ACME
     issuers.
   - **Admin endpoint disabled** (`admin.disabled`) and `persist: false`, so no TCP admin
     port is open ([I-20](../issues.md#i-20)). A config change restarts Caddy. P1-07 brings
     live reload back over a locked-down channel.
   - Caddy's storage and logs live in Kiln's local data directory, not Caddy's defaults.
5. **HTTP server** (`kiln_http`, default port 80): one route per site. A plain site is served
   there. A secure site gets a `308` redirect to its HTTPS URL, including the port when it
   isn't 443.
6. **HTTPS server** (`kiln_https`, default port 443) exists **only if at least one site is
   secure**. With no secure sites, Kiln doesn't bind 443 and doesn't need the CA. The server
   carries only the secure sites' routes plus the 404. A plain site requested over HTTPS fails
   the TLS handshake, because no certificate exists for it.
7. **Certificates:**
   - `tls.certificates.automate` lists exactly the secure sites.
   - Their automation policy uses the `internal` issuer bound to a PKI CA named `kiln`.
   - That CA is Kiln's: `root` is `root.crt` only (no key), and `intermediate` is
     `intermediate.crt` + `intermediate.key` from `CaStore::web_server_files`.
   - `install_trust: false`: Caddy never touches trust stores
     ([ADR 0010](0010-local-certificate-authority.md) §7).

   Leaf issuance and renewal stay with Caddy. Before starting with secure sites, Kiln
   re-issues the intermediate if it is missing or has fewer than 10 days left. Caddy can't
   renew it without the root key.
8. FastCGI upstream rules (no `fail_duration`, time-bounded retries; [I-14](../issues.md#i-14),
   [I-15](../issues.md#i-15)) are encoded when PHP routes arrive in P1-08/P1-09.

### Binary

9. Kiln runs Caddy as **`kiln-caddy.exe`** (`kiln-caddy` on Unix) from `<local data>/bin`. On
   Windows the local data directory is `%LOCALAPPDATA%\Kiln`, because binaries and caches
   don't belong in the roaming profile. The renamed image means a `taskkill /IM caddy.exe`
   from another tool can't hit Kiln's server ([I-11](../issues.md#i-11)).
10. `kiln web install --archive <file>` accepts only an **official release archive whose
    SHA-512 is pinned in Kiln's source**. The pins are copied from Caddy's signed
    `checksums.txt`, starting with 2.11.7. Kiln extracts the `caddy` executable, writes it with
    `rename_replace`, and records the version. Automatic download arrives with the shared
    downloader in P1-08, which also fetches PHP. Until then the CLI prints the official URL.

### Process

11. `kiln web start` writes `caddy.json`, runs `kiln-caddy validate`, then starts
    `kiln-caddy run` in the background (no console window, output to the log file). It waits
    until every listener accepts connections and records the PID and executable path in
    `caddy.pid`.
    - Before starting, it checks each port. If one is taken, for example by Herd on 80/443, it
      says so and suggests `--http-port` / `--https-port`.
    - Chosen ports are saved as settings (`web.http_port`, `web.https_port`).
12. `kiln web stop` opens the recorded process, **checks that its executable is the recorded
    `kiln-caddy`** (so PID reuse can't kill an unrelated process), terminates it and waits.
    Process inspection and termination go in `kiln-sys` (ADR 0008).
13. `link`, `unlink`, `secure` and `unsecure` restart the server if it is running, so changes
    apply immediately.
14. Starting the web server isn't a journaled operation. It changes no system state, only
    files under Kiln's own directories. System changes (hosts, trust) stay journaled operations.

## Consequences

- A fresh user can serve `http://blog.test` without creating or trusting any certificate.
- HTTPS for a site is one command, once the CA is created and trusted.
- Each site change briefly restarts Caddy (tens of milliseconds) until P1-07 restores live
  reload.
- Running next to Herd means non-default ports, such as `http://blog.test:8080`, until P1-14
  adds coexistence handling.
- Linux needs an answer for ports below 1024 (`ip_unprivileged_port_start` or a capability).
  That's tracked with P1-04.

## Alternatives considered

- **HTTPS for every site by default** (Herd/Valet style): rejected per the requirement above.
  It forces CA creation and trust before the first page loads.
- **Let Caddy's automatic HTTPS decide:** it adds HTTP→HTTPS redirects and certificates for
  every host it sees. Kiln couldn't keep HTTP sites on HTTP without fighting it.
- **Keep the admin API on TCP for live reload now:** reopens [I-20](../issues.md#i-20) before
  P1-07 has a safe channel. Restarting is good enough for development.
- **Download Caddy inside P1-06:** pulls an HTTP client and TLS stack in ahead of P1-08,
  which needs the same downloader for PHP.
