# 0044. A service's web UI as a site: Mailpit at `mailpit.test`

- **Status:** accepted
- **Date:** 2026-11-03
- **Task:** P3-07 · Builds on [ADR 0032](0032-proxy-sites.md), [0035](0035-services.md) §4, [0036](0036-service-framework.md), [0037](0037-service-ports-and-coexistence.md) · Amends [ADR 0035](0035-services.md) §4 (`mail.test` becomes `mailpit.test`)

## Context

Mailpit's inbox is a web UI on its own port (8025, or the next free one). Remembering
`http://127.0.0.1:8026` is what `.test` names are for. ADR 0035 planned the inbox as a
proxy site, `mail.test`. The user asked for the service's own name, `mailpit.test`: it says
what it is, and doesn't claim a name that a project called `mail` would want.

Kiln already serves a loopback port as a site ([ADR 0032](0032-proxy-sites.md)): `kiln proxy
<name> <target>`, through Caddy, with HTTPS and live updates. Mailpit's UI uses relative URLs
and a WebSocket, both of which pass through such a proxy unchanged.

## Decision

1. **Providers can name a web UI** (`web_ui()`): a site name and the port role it listens
   on. Mailpit's is `mailpit` on `http`. Others can follow later, such as RustFS's console.
   The contract tests check the role is one of the provider's own.
2. **`kiln service add`** then serves the new instance's UI as `<site>.test`:
   - It uses Kiln's proxy site, so the hosts file, certificates and `kiln sites` work as
     for any proxy.
   - Over HTTPS when Kiln's CA is set up. Otherwise over HTTP, with a note on how to
     secure it (`kiln ca init`, then `kiln secure mailpit`).
   - **The name is never taken over.** If a site or proxy called `mailpit` already exists,
     including one serving another Mailpit instance, Kiln prints the UI's address and how
     to serve it under another name. The first instance gets `mailpit.test`; a second one
     doesn't.
   - A failure here is a note: the service itself is already running.
3. **The site follows its instance.** A proxy counts as the instance's when it forwards to
   that instance's UI port.
   - `kiln service port` moves the proxy to the new port, keeping HTTPS or HTTP.
   - `kiln service remove` removes it.
   - A proxy the user pointed elsewhere is left alone.
4. **`MAIL_*` wiring moves to P3-08.** ADR 0035 §9 already makes Mailpit's `MAIL_MAILER`,
   `MAIL_HOST` and `MAIL_PORT` part of the journaled `.env` wiring, with the database and
   cache keys. P3-07 keeps only the site.

## Consequences

- The inbox is at `mailpit.test` from the moment Mailpit is added, and stays there through
  port changes.
- As with any proxy, the name resolves once it is in the hosts file (`kiln hosts sync`).
  While Mailpit is stopped, the site answers `502 Bad Gateway`.
- `kiln uninstall` forgets the proxy with the rest of Kiln's state.

## Alternatives considered

- **`mail.test`, as planned.** Changed at the user's request: `mailpit.test` names the
  tool, and leaves `mail` free for a project.
- **One site per instance (`mailpit-1.test`).** Not now: most projects have one Mailpit,
  and a second one can be proxied by hand.
- **A Caddy route of its own, outside the proxy table.** Rejected: proxies already have the
  names, certificates, hosts entries and listing this needs.
