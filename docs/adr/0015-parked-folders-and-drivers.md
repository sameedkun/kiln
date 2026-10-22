# 0015. Parked folders and framework drivers (document-root detection)

- **Status:** accepted
- **Date:** 2026-10-22
- **Task:** P1-10 · Builds on [ADR 0011](0011-caddy-integration.md), [0014](0014-php-cgi-supervisor.md)

## Context

So far a site is a name plus the exact folder to serve (`kiln link --path`). Two things are
missing:

1. **Parking:** most developers keep projects in one folder (e.g. `D:\Sites`) and expect every
   subfolder to be a site (`D:\Sites\blog` → `blog.test`) without linking each one.
2. **Framework awareness:** a Laravel project must be served from its `public/` folder, a
   Drupal or Craft project from `web/`, and so on. Today the user has to link the right
   subfolder by hand.

The plan said to port Laravel Valet's drivers (MIT). On a closer look, most of a Valet driver
is about Valet's own request handling. Kiln's Caddy route already covers that: front controller
through `index.php`, static files served directly, PHP over FastCGI
([ADR 0014](0014-php-cgi-supervisor.md) §6). What Kiln actually needs from a driver is **which
folder is the document root**. That's documented by each framework itself.

## Decision

### Drivers: detection only, written for Kiln

1. A **driver** looks at a project folder and returns a name and a document root. Rules are
   checked in order, and the first match wins:

   | Driver | Detected by | Document root |
   |---|---|---|
   | `laravel` | `artisan` and `public/index.php` | `public/` |
   | `symfony` | `bin/console` and `public/index.php` | `public/` |
   | `drupal` | `web/core/lib/Drupal.php` | `web/` |
   | `craft` | `craft` and `web/index.php` | `web/` |
   | `wordpress-bedrock` | `web/wp-config.php` | `web/` |
   | `wordpress` | `wp-config.php` or `wp-load.php` | the folder |
   | `php-public` | `public/index.php` | `public/` |
   | `php-web` | `web/index.php` | `web/` |
   | `php` | `index.php` | the folder |
   | `static` | none of the above | the folder |

2. Every PHP driver uses the same Caddy route (`index.php` front controller), which serves
   all of these frameworks' pretty URLs. A **`static`** site gets the plain file server, with
   no PHP route, even when a PHP pool runs.
3. Detection runs every time Kiln renders the web server config, so converting a folder to
   Laravel only needs `kiln web reload`. `kiln sites` shows each site's driver and document
   root.
4. **No Valet code is used.** The table comes from the frameworks' own documented layouts.
   The planned Valet entry in `THIRD_PARTY_NOTICES.md` becomes "considered, not used". Drivers
   with special routing (e.g. Magento's `pub/`, Statamic's static caching) can be added later
   as more rows.

### Links and parked folders

5. **Links** stay as they are: a name and a project folder (P1-06's `sites` table). The
   document root is now detected *inside* that folder.
   - Linking a Laravel project's root serves `public/`.
   - Linking `public/` itself still works: it detects as `php`.
6. **Parked folders** (new `parked` table, migration v4): `kiln park [folder]` (default: the
   current folder), `kiln unpark [folder]`.
   - Every immediate subfolder whose name is a valid host label becomes `<name>.test`.
   - Hidden folders (`.git`, …) and names that aren't valid labels are skipped and listed by
     `kiln sites`.
7. **Precedence:** a link always wins over a parked folder of the same name. Between two
   parked folders, the one parked first wins; `kiln sites` reports the shadowed one.
8. **HTTPS and parked sites:** `kiln secure <name>` on a parked site pins it as a link (same
   folder, `secure` on). `kiln unsecure` keeps the link. `kiln unlink` on a pinned site returns
   it to being a plain parked site.
9. **DNS:** the Windows hosts file has no wildcards, so each parked site needs an entry.
   `kiln hosts sync` lists links and parked sites, and reloads a running web server so new
   folders are served immediately. Creating a project folder therefore needs one
   `kiln hosts sync` (one UAC prompt). The command hints when a site is missing.
10. Parking changes no system state outside Kiln's database: it isn't a journaled operation.

## Consequences

- `kiln park D:\Sites` plus one `kiln hosts sync` serves every project in that folder, with
  Laravel, Symfony, Drupal, Craft and WordPress roots found automatically.
- A site's document root can change when its files change (e.g. after `laravel new`), at the
  next reload.
- New folders aren't served until `kiln hosts sync` (the DNS needs it anyway).

## Alternatives considered

- **Port Valet's drivers:** larger and tied to Valet's request flow. Most of it duplicates what
  the Caddy route already does.
- **Caddy wildcard routing (`*.test` → `{labels.1}`):** no reload for new folders, but
  per-project document roots would need file checks inside Caddy, and the hosts file needs an
  entry per site anyway.
- **Store detected drivers in the database:** gets stale when projects change; detecting is
  cheap (a few file checks per site).
