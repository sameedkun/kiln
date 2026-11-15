# 0083. Extra names for a site (`kiln domain`), and `APP_URL` following HTTPS

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-04 · Builds on [ADR 0015](0015-parked-folders-and-drivers.md),
  [0027](0027-project-config-kiln-toml.md), [0045](0045-env-wiring.md),
  [0074](0074-site-groups.md)

## Context

A site can have other names today only through its project's `kiln.toml` (`aliases`, ADR 0027).
That file belongs to the project and is often committed, so it suits names a team shares
(`admin.shop.test`), not ones a single developer wants on their machine. The CLI and the app
can't add a name without editing it. Lerd and Herd both manage extra names from their own
UI.

Turning HTTPS on or off changes a site's URL. A Laravel app's `APP_URL` then names the
old scheme, so links, redirects and asset URLs point at the wrong one until someone edits
`.env`.

## Decision

1. **`kiln domain add <site> <name>...`**, **`kiln domain remove <site> <name>...`** and
   **`kiln domain list [<site>]`** (`--json`).
   - A name is written short (`admin`) or with the TLD (`admin.shop.test`), and is checked
     like any site name.
   - A name another site, proxy or alias already has is refused.
   - Proxy sites aren't covered yet.
2. **Where names are kept:** in Kiln's state, as the setting `sites.domains`
   (`{"shop.test": ["admin.shop.test"]}`), never in the project. As with groups (ADR 0074),
   nothing of the project's or the system's changes, so it isn't journaled; `remove` is the
   way back.
3. **They serve as aliases do:**
   - Each name is another site for the same folder, following its main site's PHP, HTTPS
     and settings.
   - It gets a certificate, its own row in `kiln sites` (source `alias`), and a place in the
     hosts file on the next `kiln hosts sync` (or the app's offer).
   - If its main site goes, the name stops being served and stays in the setting until
     removed, or until a site of that name returns.
   - In the sites JSON, an extra name has `"domain": true`. A main site lists its names
     with where each comes from: `aliases: [{host, from: "kiln" | "kiln.toml"}]`.
4. **`APP_URL` follows `kiln secure` and `kiln unsecure`.**
   - When the site's `.env` has an `APP_URL` on that site's own name, only its scheme (and
     the HTTPS or HTTP port Kiln serves on) changes.
   - This is the journaled `.env` change of ADR 0045. Kiln names its undo number, and
     `kiln undo` reverses it.
   - An `APP_URL` on another name, such as a tunnel or a production URL, is left alone,
     with a note.
5. **The app:** the site page lists the site's names. Kiln's own names have a Remove
   button; ones from `kiln.toml` say so. An Add field runs `kiln domain add`, and then
   offers the hosts file as after a link.

## Consequences

- A developer adds `api.shop.test` in two clicks, without touching the repo.
- `kiln.toml` aliases and Kiln's names behave the same at run time, differing only in where
  they're kept.
- Turning HTTPS on now writes to the project's `.env`. Only one value changes, it's shown,
  and it's undoable.
