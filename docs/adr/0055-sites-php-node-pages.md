# 0055. The Sites, PHP and Node pages

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-04 · Builds on [ADR 0052](0052-phase-4-screens-and-scope.md) (the screens), [0053](0053-api-v2-and-json-output.md) (the API), [0054](0054-desktop-app-shell.md) (the shell)

## Context

P4-15 drew these pages with sample data; P4-04 connects them to Kiln. What each needs, and
where it stands:

- **Sites:** the list and its groups, and adding (link, park, proxy) and removing them.
  Everything comes from `sites.list` and the commands already on the API's allow-list.
  Two fields the screens show aren't in `sites.list` yet:
  - each site's Node version;
  - its framework's version (ADR 0053 §6).
- **A site's page:**
  - the site's PHP (`isolate`, `unisolate`), HTTPS (`secure`, `unsecure`), Node, its
    folder and document root;
  - its requests (P4-18), logs (P4-08) and `.env` (P4-05) come with their own tasks;
  - which services the site uses comes with P4-05, from the `.env` wiring.
- **PHP:** versions and the default from `php.list`, extensions from `php.extensions`, what
  can be installed from `php.available`. The changes are `php install|remove|default|update`
  and `php ext on|off`. Install and update download, so they report progress.
- **Node:** the same, from `node.list` and `node.available`, with
  `node install|remove|default`.
- **Folders:** linking and parking need one, so the app asks Windows for it.

## Decision

1. **`kiln --json sites` gains, for each site:**
   - `project`, its folder;
   - `framework_version`, read from the project's `composer.lock` for the framework's
     package, or from WordPress' `wp-includes/version.php`;
   - `node`, the version that runs there and why: the file that asks for it (`kiln.toml`,
     `.nvmrc` or `.node-version`) and what it asks, whether that fell back to the default,
     or nothing installed.
2. **Two app commands, both checked on the Rust side:**
   - `open_folder` opens a site's folder by its host name, as `open_site` opens its URL. The
     page never hands the app a path to open.
   - `pick_folder` shows Windows' folder dialog (Tauri's dialog plugin, MIT OR Apache-2.0)
     and returns the folder chosen.
3. **Names and paths a user types are checked in the page before they reach a job:**
   - a site name is lower-case letters, digits and hyphens, so nothing starting with `-` is
     read as an option;
   - a proxy's target is a port, or a loopback address with a port;
   - a folder is the absolute path the dialog returned.

   The CLI checks everything again, as for any command.
4. **Downloads show progress:** installing or updating PHP or Node shows a progress bar fed
   by the job's `progress` events (ADR 0053 §1). Every change runs as a job and shows how
   it went.
5. **Not on these pages yet,** each with its task:
   - a site's requests (P4-18), logs (P4-08), `.env` and services (P4-05);
   - "New Laravel project" (`kiln new`) arrives with the onboarding (P4-06), which already
     handles long, chatty jobs;
   - PHP's worker count stays in the CLI (`kiln php workers`).

## Consequences

- **What's left:** the three pages are connected with what Kiln has today. Each missing tab
  says which task brings it.
- **One more Tauri plugin:** its MPL-free licence keeps the app's deny policy as it is.
