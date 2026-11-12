# 0052. Phase 4's screens, and what the app shows beyond the CLI

- **Status:** accepted
- **Date:** 2026-11-12
- **Task:** P4-15 · Amends the Phase 4 plan of [ADR 0050](0050-desktop-app.md) · Builds on [ADR 0051](0051-desktop-visual-design.md)

## Context

After P4-02, the user compared Kiln's gallery with Lerd's dashboard (lerd.sh). Lerd is
open source (MIT), with containers under Podman, and on Windows it runs inside WSL2. Its
dashboard shows much more than ours did:
- each site's framework, workers and request timing;
- services grouped by category;
- the system's health and resource use;
- an update check;
- an MCP server for AI assistants.

The user's words: "our ui looks good but it doesn't have too much in it". They left it to us
whether to add more now or after Phase 4.

Most of the gap is screens Phase 4 had planned but not drawn. Some of it is new features.
Kiln's API (P4-01) should be shaped by what the screens show, so the screens come first.

Clean-room, as with Herd: Kiln takes ideas of what to show from Lerd's public pages, not
its design, text, code or assets.

## Decision

1. **The screens come before the API (P4-15).** Every page is drawn in the design gallery,
   from the base components and with sample data:
   - Home;
   - Sites, grouped by parked folder, link, or proxy and tool;
   - a site's page with tabs: Overview, with PHP, Node, HTTPS, its workers and its
     services; Requests; Logs; `.env`;
   - Services, by category, with the catalog to add one;
   - PHP, with versions, updates and extensions;
   - Node;
   - Logs, with sources and following live;
   - History;
   - Settings, with the PC's health from `kiln doctor`, Kiln's footprint, updates and AI
     assistants.

   The gallery's sample data (`src/gallery/sample.ts`) is the first draft of the shapes
   P4-01's read methods return.
2. **Features added to Phase 4:**
   - **P4-16, Kiln's footprint:** memory and CPU per Kiln process, disk use by folder, and
     clearing the download cache.
   - **P4-17, updates:**
     - PHP patch updates (already `kiln php update`) shown where they apply;
     - checking GitHub for a new Kiln, off by default. The check sends nothing about the user.
   - **P4-18, request timing per site,** read from Caddy's access log for that site:
     - the median, the slowest 5%, counts and failures, and the slowest requests;
     - kept on the PC.
   - **P4-19, an MCP server for AI assistants** (`kiln mcp`, moved from Phase 5):
     - it reads sites, services, logs and history;
     - changes go through the CLI, so they keep previews and journals and show in History;
     - it is off until the user turns it on.
3. **Moved to Phase 5 or later:** git worktrees as sites, database snapshots before a
   migration, a Tinker console, sharing sites on the LAN or through public links, plugins,
   and imported themes.
4. **The look stays Kiln's** ([ADR 0051](0051-desktop-visual-design.md)):
   - Home starts with one sentence about what matters;
   - figures and charts live on detail pages (a site's Requests tab), not on Home;
   - frameworks are words, not logos;
   - statuses are words with their dots.

## Consequences

- P4-01 can be written against screens that exist, and its contract tests can use the
  gallery's sample shapes.
- Phase 4 grows by four tasks (P4-16 to P4-19). Request timing and the MCP server each get
  an ADR when they start: the first reads Caddy's logs, the second opens Kiln to other
  programs on the PC.
- The gallery's checks cover every screen, in light and dark, with axe.

## Alternatives considered

- **Finish Phase 4 as planned, then add more.** The API would be designed without the
  screens that use it, and redone after.
- **Copy Lerd's dashboard layout.** It is dense and colourful, which the user had called
  generic in Kiln's own first prototype. Its ideas were taken, not its look.
