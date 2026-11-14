# 0076. Sharing a site

- **Status:** accepted; §4 (LAN sharing) approved by the user and built 2026-11-14
- **Date:** 2026-11-14
- **Task:** P4C-12 (from Phases 5 and 6) · Principle 3 (loopback only unless opted in per site)

## Context

The user asked to share a site beyond this PC (2026-11-14): on the LAN, and through a tunnel
(Cloudflare Tunnel, ngrok or Expose). Kiln listens on loopback only; principle 3 allows more
only when the user opts in per site, and no firewall rules by default.

## Decision

1. **Tunnels first: a Cloudflare quick tunnel**, which needs no account and no firewall
   change. `cloudflared` (Apache-2.0, 2026.11.0, pinned by GitHub's digest, the same SHA-256
   the release notes list) runs as a project process of the site's project, `tunnel`, kept
   up by Kiln like the queue worker (`Runtime::Own`, ADR 0073). It connects out to
   Cloudflare and forwards to Kiln's own web server on loopback with the site's name as the
   host (`--http-host-header`); a secure site is forwarded to over HTTPS with its name for
   TLS, its certificate Kiln's own (`--no-tls-verify`). **Nothing new listens on the
   network**; the Scenario checks that.
2. **`kiln share <site>`** starts it (or finds it running) and prints its public address,
   `https://….trycloudflare.com`, read from its log; `kiln share <site> --stop` removes it,
   and the address stops working. `kiln service list` gives a tunnel's `shared_url`.
   cloudflared gets an empty config file of Kiln's (`--config`), so a tunnel of the user's
   own (`~/.cloudflared/config.yml`) never applies, and `--no-autoupdate`.
3. **The app asks in words** before sharing: anyone with the address can open the site
   while it's shared. The site's Overview shows the address, with Copy and "Stop sharing".
4. **LAN sharing**, per site and opt-in (`kiln share <site> --lan [--stop]`): that one site
   on this PC's private network address (10/8, 172.16/12 or 192.168/16, found by asking the
   system which address it would route from; a public one is refused) and a port of its own
   (8101 up), plain HTTP, to any host name, until stopped. Each share is its own server in
   the web config, the only listeners beyond loopback (a test keeps every other listener on
   loopback). Windows' own firewall prompt is the consent for the network; Kiln adds no
   rule. `kiln --json sites` gives a shared site's `lan_url`.
5. **ngrok and Expose** need accounts and aren't open source: if the user has them, they
   work against Kiln's sites as they are (`ngrok http --host-header=shop.test 8080`); Kiln
   doesn't download them.

## Consequences

- A quick tunnel's address is random and changes each time; named tunnels on the user's
  own Cloudflare account can come later.
- While shared, the site is public: the dialog says so, and the address shows only while
  the tunnel runs.
