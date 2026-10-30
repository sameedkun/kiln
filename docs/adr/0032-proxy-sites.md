# 0032. Proxy sites

- **Status:** accepted
- **Date:** 2026-10-30
- **Task:** P2-09 · Builds on [ADR 0005](0005-web-server.md), [0011](0011-caddy-integration.md), [0015](0015-parked-folders-and-drivers.md)

## Context

Kiln serves folders: static files, or PHP through its workers. Front-end and Node apps run
their own dev server instead:

- **Vite** on `localhost:5173`, Next.js on `3000`, a Laravel Octane or Reverb server on
  another port.
- **What users want:** reach them as `app.test`, over HTTPS when the app needs a secure
  context (service workers, secure cookies, OAuth callbacks), with live reload still
  working.

Herd and Valet offer `proxy <name> <url>` for this. The Phase 2 exit criterion asks that a
proxy site serves a local dev server, such as Vite. Plan principle 3 says Kiln listens only
on loopback, and proxies must not become a way to reach other machines.

## Decision

1. **Commands:**
   - `kiln proxy <name> <target> [--secure]` serves `<name>.test` from the target. Run
     again, it changes the target.
   - `kiln unproxy <name>` stops serving it.
   - `kiln secure` / `kiln unsecure <name>` work on proxies as on other sites.
   - `kiln sites` lists proxies with their targets.
2. **Targets are loopback only:**
   - **Forms:** a port (`5173`), `host:port`, or `http://host:port` / `https://host:port`.
     A port alone means `http://localhost:<port>`.
   - **Hosts:** `localhost`, an address in `127.0.0.0/8`, or `[::1]`. Anything else is
     refused, so Kiln never forwards to another machine.
   - **Not allowed:** a path, query or user name in the target. Caddy forwards the whole
     request path, so a base path would be misleading.
3. **Caddy's `reverse_proxy`** serves the site:
   - **`localhost` is dialled as given.** Caddy then tries `::1` and `127.0.0.1`, which
     matters because Vite on Windows often listens on `::1` only.
   - **HTTPS targets** are reached over TLS without checking their certificate. Dev servers
     use self-signed ones, and the connection never leaves the machine.
   - **The `Host` header becomes the target's** `host:port`. Dev servers check it: Vite 6
     answers "Blocked request" for any other host. `X-Forwarded-Host` and
     `X-Forwarded-Proto` (set by Caddy) still carry the `.test` name and scheme.
   - **Live reload passes through.** Caddy upgrades WebSockets and flushes server-sent
     events, so Vite's HMR connects back through `<name>.test`. Over HTTPS it uses `wss`,
     ended by Caddy.
4. **A site kind of its own,** stored in a new `proxies` table:
   - **A linked site with the same name** is refused: `kiln unlink` it first.
   - **A parked subfolder with the same name** is shadowed by the proxy. Kiln says so, and
     the folder is served again after `kiln unproxy`.
   - Proxies follow the same names, hosts entries (`kiln hosts sync`), certificates and
     live reload as linked sites. `kiln uninstall` forgets them with the rest of Kiln's
     state.
5. **Not journaled,** like links: it's Kiln's own state, and nothing system-wide changes
   except through `kiln hosts sync`, which is journaled.

## Consequences

- **A dev server becomes `app.test` in one command,** over HTTPS if asked, with HMR working.
- **While the dev server isn't running,** Caddy answers `502 Bad Gateway`. That is the
  standard answer, and `kiln sites` names the target to start.
- **An app that builds absolute URLs from `Host`** sees the target's `host:port`. It should
  read `X-Forwarded-Host`, as it would behind any proxy. Vite and similar dev servers need
  no change, because they emit relative URLs.

## Alternatives considered

- **Keep the original `Host` header:** truer to production, but Vite 6+ and other dev
  servers reject it until the user edits their config (`server.allowedHosts`).
- **Allow any target:** Kiln would become an open forwarder for `.test` names. Loopback
  covers dev servers, containers that publish ports, and SSH tunnels.
- **Store proxies as linked sites with a target column:** a site would then be either a
  folder or a URL in every code path that reads links. A table of its own keeps the folder
  logic as it is.
