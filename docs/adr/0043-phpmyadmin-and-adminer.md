# 0043. phpMyAdmin and Adminer, served as Kiln sites and pointed at Kiln's instances

- **Status:** accepted
- **Date:** 2026-11-03
- **Task:** P3-15 · Builds on [ADR 0015](0015-parked-folders-and-drivers.md), [0018](0018-history-undo-uninstall.md), [0027](0027-project-config-kiln-toml.md), [0035](0035-services.md), [0039](0039-windows-first-and-services-catalog.md), [0042](0042-meilisearch-rustfs-mongodb.md) §1

## Context

Herd Pro has no database web UI. DBngin and most local stacks ship phpMyAdmin or Adminer, and
[ADR 0039](0039-windows-first-and-services-catalog.md) adds both to Kiln. They're PHP apps,
so Kiln can serve them like any site, with no new process.

| | phpMyAdmin 5.2.3 | Adminer 6.2.0 |
|---|---|---|
| Download | `phpMyAdmin-5.2.3-english.zip`, 11 MB, from files.phpmyadmin.net | `adminer-6.2.0.php`, 581 KB, one file, from GitHub |
| Published check | a `.sha256` file beside the archive | GitHub's asset digest |
| License | GPL-2.0 | Apache-2.0 or GPL-2.0 |
| Databases | MySQL, MariaDB | MySQL, MariaDB, PostgreSQL, SQLite, and more through plugins |
| PHP | the version feed says below 8.4, but the 5.2.3 ChangeLog fixes PHP 8.4 and 8.5 deprecations | 7.4 or later |
| Passwordless login | `auth_type = config` with `AllowNoPassword` | refused unless a plugin's `login()` allows it |
| Fixed server list | `$cfg['Servers']` | a plugin's `loginFormField()` and `credentials()`, as Adminer's own `login-servers` plugin does |

Kiln's instances come and go (`kiln service add`, `remove`, `port`), so a config written once
would go stale.

## Decision

1. **`kiln tool add <phpmyadmin|adminer>`** and **`kiln tool remove <name>`:**
   - `add` downloads the pinned release into its own folder:
     `<local data>/tools/<name>/<release folder>`, unpacked or, for Adminer, copied.
     This is the same `install_into_folder` used for companions
     ([ADR 0042](0042-meilisearch-rustfs-mongodb.md) §1).
   - It then writes Kiln's files beside the release and links that folder as a site:
     `pma.test` or `adminer.test`, over plain HTTP.
   - `add` needs a Kiln PHP and says which to install if there is none.
   - `remove` unlinks the site and deletes the tool's folder.
2. **`servers.json`:**
   - `<local data>/tools/servers.json` lists every instance: name, kind and main port.
   - The CLI rewrites it after every service change, once a tool is installed.
   - Both tools read it at each request, so they always show the current instances.
   - A failure to write it is printed and never stops the service command.
3. **phpMyAdmin:** Kiln writes `config.inc.php`:
   - one server per MySQL or MariaDB instance, named after it;
   - `auth_type = config` as root with no password: the servers listen on 127.0.0.1 only
     ([ADR 0038](0038-mysql-and-mariadb.md));
   - a random 32-character `blowfish_secret`;
   - its temporary folder in the tool's folder;
   - no version check and no error reports sent to phpMyAdmin.
4. **Adminer:** Kiln writes `index.php`, which includes the release, and
   `adminer-plugins.php`, which Adminer loads from its folder:
   - one plugin, a few dozen lines;
   - the login form offers Kiln's MySQL, MariaDB and PostgreSQL instances by name;
   - it fills in the driver and user (`root`, or `postgres` for PostgreSQL);
   - `login()` lets a listed instance in without a password, and nothing else.
   - Adminer's own login CSRF token stays in force.
5. **No PHP pin.** Both run on the default PHP. CI runs them on PHP 8.5, the newest, and
   fails on any PHP warning or deprecation notice in their pages.
6. **Uninstall:** a site inside Kiln's tools folder is Kiln's own, not a project. So it
   doesn't stop Kiln's folder being deleted ([ADR 0018](0018-history-undo-uninstall.md) §5).

## Consequences

- One command gives a database UI that already knows every Kiln instance, and follows
  them as they change.
- The tools open without a login. They're reachable only through Kiln's web server on this
  computer, as the databases themselves are. They keep their own CSRF protection.
- MongoDB and Redis have no web UI here. Adminer's plugins for them need PHP extensions
  Kiln doesn't enable; `kiln db` covers them.
- Kiln now writes PHP code into the tools' folders. It's generated from Rust with the paths
  quoted for PHP, and unit-tested.

## Alternatives considered

- **Serving the tools from Kiln's own binary or a separate server.** Rejected: they are
  PHP apps, and Kiln already serves PHP sites.
- **Adminer's `Adminer\Password` for passwordless access.** Rejected: it asks for a
  password Adminer checks itself, which adds a step for nothing on a local machine.
- **Writing the server list into the configs at each change.** Rejected: the configs
  would need rewriting from every service command. One data file the configs read is
  simpler.
- **Pinning phpMyAdmin to PHP 8.4.** Not needed: 5.2.3 runs clean on 8.5 (checked in CI),
  and a pin would force a second PHP install on many users.
- **HTTPS by default.** Left to the user (`kiln secure pma`). The sites need no secure
  context, and HTTPS needs Kiln's CA trusted first.
