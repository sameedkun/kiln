# 0056. The Services page, and a site's services and `.env`

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-05 · Builds on [ADR 0045](0045-env-wiring.md) (`.env` wiring), [0049](0049-data-safety.md) (dump and restore), [0053](0053-api-v2-and-json-output.md), [0055](0055-sites-php-node-pages.md)

## Context

P4-05 connects the Services page drawn in P4-15:

- the instances, by kind, started and stopped;
- the catalog, to add one, with download progress;
- each instance's ports, log, wiring into a project, dump and restore, and the data tools;
- on a site's page, the services it uses and its `.env`.

The CLI does all of it already (`service add|start|stop|restart|port|remove|logs`,
`env use`, `db dump|restore`, `tool add`), and the jobs allow-list has every change. Three
things are missing:

- **Which sites use an instance:** nothing records it. `env use` writes `.env` keys as a
  journaled edit, but the journal holds the file's path and the keys, not the instance.
  A project's `.env` can also change after it was wired.
- **A log, and a `.env`, as data:** `service logs` prints text, and nothing shows a
  project's `.env`.
- **Where a dump goes:** `db dump` writes to the current folder by default. For a job
  that is the daemon's folder.

## Decision

1. **"Used by" is read from the projects' `.env` files, not from history.** An instance is
   used by a site when the site's `.env` holds every key Kiln would write for that
   instance's port: the provider's own `env_keys` for its ports, such as `DB_PORT=3306`,
   `REDIS_PORT=6379`, `MAIL_PORT=1025`, `MEILISEARCH_HOST=http://127.0.0.1:7700`.
   - **It reflects the file as it is now,** however it was edited.
   - **Each listing reads one small file per site.**
   - **`kiln --json service list` gains, per instance:**
     - `used_by`, the sites' hosts;
     - `web`, the URL of its web page when Kiln serves one (Mailpit's inbox);
     - `databases`, whether it holds databases (for dump, restore and the data tools).
2. **Two reads gain data:**
   - **`kiln --json service logs <name> [--lines N]`:** each log file with its last lines.
   - **`kiln env show [--project DIR]`** (new): a project's `.env` as keys and values, in
     order. In text, secrets (keys with `KEY`, `SECRET`, `PASSWORD` or `TOKEN`) are hidden
     unless `--reveal` is given. The app hides them until the user asks.

   The API's read methods are `services.logs` (`name`, `lines`) and `env.show`
   (`project`). The daemon checks that `project` is the folder of one of Kiln's sites, so
   the app can only read the `.env` of a site Kiln serves.
3. **Dumps and restores go through Windows' file dialogs:**
   - "Dump a database…" asks where to save, defaulting to
     `<instance>-<database>-<date>.sql`, and passes `--output`;
   - "Restore…" asks for the `.sql` file.

   The app's Rust side shows both dialogs, as it does for folders (ADR 0055 §2).
4. **The page:**
   - **The instances:** grouped by kind (databases, cache, search, mail, storage,
     realtime), with filter chips. Each shows its version, its main port, the sites that
     use it and its status, with these actions:
     - start and stop;
     - its web page;
     - restart, its log;
     - change a port;
     - wire a site (`env use`, through its preview);
     - for databases: dump, restore and browse;
     - remove, which keeps the data unless the user ticks "Delete its data".
   - **"Add a service":** the catalog with each kind's versions and licence. Adding one runs
     `service add` with a progress bar. Reverb asks which site's project it runs in.
   - **Browsing data:** phpMyAdmin (MySQL, MariaDB) and Adminer (those and PostgreSQL) open
     at their sites when added. Otherwise the app offers to add them (`tool add`). WhoDB is
     P4-12.
5. **A site's page gains:**
   - **Its services** ("used by", §1), with "Wire a service…".
   - **Its `.env`:** each key with its value. Secrets are hidden until shown. Keys whose
     value is what Kiln writes for one of its instances are tagged "set by Kiln".

## Consequences

- **"Used by" can be wrong,** when a project points at an instance's port with other
  hosts, or with keys Kiln doesn't write. It never shows a site using an instance whose
  port it doesn't name.
- **`.env` stays on the user's PC:** the API only reads the `.env` of Kiln's own sites. It
  goes to the app over the user's own socket, as everything else.
- **Every change on the page is a CLI command,** so it shows in History, with its undo
  where the command has one.
