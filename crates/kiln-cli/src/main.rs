//! `kiln` CLI: previews (`plan`), state inspection (`state info`), hosts block changes
//! (`hosts set|sync|clear`), the local CA (`ca`), sites and the web server (`link`, `secure`,
//! `web`), PHP runtimes (`php`), and `undo`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kiln_core::Tld;
use kiln_platform::hosts;

// First: its `say!` and `say_err!` macros are used by every module after it.
#[macro_use]
mod out;
mod app;
mod apply;
mod coexist;
mod daemon;
mod doctor;
mod dump;
mod env;
mod ext;
mod footprint;
mod groups;
mod history;
mod logs;
mod mcp;
mod new;
mod node;
mod pecl;
mod php;
mod pool;
mod project;
mod proxy;
mod requests;
mod service;
mod services_up;
mod tool;
mod uninstall;
mod updates;
mod web;
mod xdebug;

#[derive(Debug, Parser)]
#[command(
    name = "kiln",
    version,
    about = "Local PHP development environment (pre-alpha)"
)]
struct Cli {
    /// Print JSON Lines for programs instead of text (ADR 0053): events, then one `result`
    /// or `error`. Never asks on the terminal.
    #[arg(long, global = true)]
    json: bool,
    /// With --json: answer yes to the command's confirmation. Without it, a command that
    /// asks stops there with exit code 3, after showing its preview.
    #[arg(long, global = true)]
    confirmed: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show what a change would do, without applying it.
    Plan {
        #[command(subcommand)]
        target: PlanTarget,
    },
    /// Inspect Kiln's own state database.
    State {
        #[command(subcommand)]
        action: StateAction,
    },
    /// Manage Kiln's block in the system hosts file (Windows; needs administrator approval).
    Hosts {
        #[command(subcommand)]
        action: HostsAction,
    },
    /// Kiln's local certificate authority (only ever valid for the development TLD).
    Ca {
        #[command(subcommand)]
        action: CaAction,
    },
    /// Serve a folder as <name>.test, over plain HTTP unless --secure is given.
    Link {
        /// Site name (default: the folder's name), with or without `.test`.
        name: Option<String>,
        /// Folder to serve (default: the current folder).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Serve it over HTTPS (same as `kiln secure` afterwards).
        #[arg(long)]
        secure: bool,
    },
    /// Stop serving a site. Its files are untouched.
    Unlink { name: String },
    /// Serve `<name>.test` from a local dev server: a port (5173), `host:port`, or
    /// `http(s)://host:port` on this machine. Live reload keeps working through it.
    Proxy {
        name: String,
        target: String,
        /// Serve it over HTTPS.
        #[arg(long)]
        secure: bool,
    },
    /// Stop serving a proxy site.
    Unproxy { name: String },
    /// Serve every subfolder of a folder as <subfolder>.test (default: the current folder).
    Park { path: Option<PathBuf> },
    /// Stop serving a parked folder's subfolders. Nothing is deleted.
    Unpark { path: Option<PathBuf> },
    /// Turn HTTPS on for a site (HTTP then redirects to it). Needs `kiln ca init` once.
    Secure { name: String },
    /// Turn HTTPS off for a site again; it is served over plain HTTP.
    Unsecure { name: String },
    /// List sites (linked and parked) with their URLs, framework and document root.
    Sites,
    /// Pin a site to a PHP version, e.g. `kiln isolate 8.3` in its folder, or `kiln isolate 8.3 blog`.
    Isolate {
        /// A minor version (newest installed patch) or an exact one, e.g. 8.3 or 8.3.36.
        version: String,
        /// The site (default: the one whose folder contains the current folder).
        site: Option<String>,
    },
    /// Let a site use the default PHP version again.
    Unisolate { site: Option<String> },
    /// Check Kiln's health: state, ports, web server, PHP, sites, DNS, certificates, shims.
    /// Changes nothing; exits with 1 if something failed.
    /// With --json, the findings are the result's data.
    Doctor,
    /// What can be updated: PHP patch releases for Kiln's PHP, and a new Kiln on GitHub if
    /// checking for it is on (off by default; ADR 0064).
    Updates {
        #[command(subcommand)]
        action: Option<UpdatesAction>,
    },
    /// A Model Context Protocol server on standard input and output, for AI assistants: reads
    /// sites, services, logs and history; changes only when allowed (ADR 0066).
    Mcp {
        #[command(subcommand)]
        action: Option<McpAction>,
    },
    /// Put a site in a group (sites that belong together, shown together), or with no group
    /// given, take it out of its group (ADR 0074).
    Group {
        /// The site, e.g. shop or shop.test.
        site: String,
        /// The group's name, e.g. "Acme".
        group: Option<String>,
    },
    /// How fast each site answers, from the web server's access log: requests, failures,
    /// median, slowest 5%, the slowest requests (ADR 0065). Changes nothing.
    Requests {
        /// One site, e.g. shop or shop.test.
        site: Option<String>,
    },
    /// What Kiln uses on this PC: memory and CPU per kind of Kiln process, disk use per folder
    /// (ADR 0063). Changes nothing.
    Footprint,
    /// Kiln's download cache: `kiln cache clear` removes it; Kiln downloads again what it
    /// needs.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Kiln's logs: without a source, every log Kiln knows of (its own, the web server's,
    /// PHP's, each service's, each Laravel site's); with one, its last lines (ADR 0059).
    Logs {
        /// A log's id, as `kiln logs` lists it (such as `web`, `php`, `service:mysql`).
        source: Option<String>,
        /// How many lines.
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },
    /// Kiln's settings: the development TLD, the web server's ports (and whether Kiln picks
    /// them), PHP's worker count (ADR 0058). Changes nothing.
    Settings,
    /// What's set up and what isn't yet: the web server, PHP, HTTPS, the hosts file, the shims,
    /// starting at sign-in. What first-run onboarding asks about (ADR 0057). Changes nothing.
    Setup,
    /// Show what `php`, `composer` and `node` run in a folder (default: the current one), and
    /// why.
    Which { path: Option<PathBuf> },
    /// Composer (verified download from getcomposer.org, run through the `composer` shim).
    Composer {
        #[command(subcommand)]
        action: ComposerAction,
    },
    /// The `php`, `composer`, `node`, `npm`, `npx` and `corepack` commands on your PATH that
    /// follow each project's PHP and Node versions.
    Shims {
        #[command(subcommand)]
        action: ShimsAction,
    },
    /// PHP runtimes from windows.php.net (verified downloads, into Kiln's own folder).
    Php {
        #[command(subcommand)]
        action: PhpAction,
    },
    /// Node runtimes from nodejs.org (signature-checked downloads, into Kiln's own folder).
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    /// Kiln's web server (Caddy, running as kiln-caddy).
    Web {
        #[command(subcommand)]
        action: WebAction,
    },
    /// Create a Laravel project with Kiln's PHP and Composer, and serve it as <name>.test.
    New {
        /// Project and site name, e.g. shop (served as shop.test).
        name: String,
        /// What to start from: laravel, react, vue, livewire (Laravel's starter kits), symfony,
        /// bedrock (`WordPress`) or empty.
        #[arg(long, default_value = "laravel")]
        template: String,
        /// Folder to create it in (default: the current folder).
        #[arg(long)]
        path: Option<PathBuf>,
        /// PHP version to use, e.g. 8.3; also written to the project's kiln.toml.
        #[arg(long)]
        php: Option<String>,
        /// Serve it over HTTPS.
        #[arg(long)]
        secure: bool,
        /// Use a Kiln database: mysql, mariadb or pgsql (its first instance), or an instance
        /// such as mysql-8.4. Its .env is pointed there and the migrations run.
        #[arg(long)]
        db: Option<String>,
    },
    /// Bring a project's settings over from another tool.
    Import {
        #[command(subcommand)]
        from: ImportFrom,
    },
    /// Services such as Mailpit: instances that run on this computer only, each with its data.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Open a database service's client on an instance (`kiln db mysql-8.4 [database]`), or
    /// dump and restore its data as portable .sql files.
    #[command(args_conflicts_with_subcommands = true)]
    Db {
        #[command(subcommand)]
        action: Option<DbAction>,
        /// The instance, e.g. mysql-8.4 (see `kiln service list`).
        name: Option<String>,
        /// A database to open.
        database: Option<String>,
    },
    /// Point a project's .env at a Kiln service (journaled: `kiln undo` reverses it).
    Env {
        #[command(subcommand)]
        action: EnvAction,
    },
    /// Database web tools served as sites: phpMyAdmin (pma.test) and Adminer (adminer.test),
    /// pointed at Kiln's instances.
    Tool {
        #[command(subcommand)]
        action: ToolAction,
    },
    /// Start or stop all services at once.
    Services {
        #[command(subcommand)]
        action: ServicesAction,
    },
    /// Kiln's desktop app (Windows): start it in the tray when you sign in.
    App {
        #[command(subcommand)]
        action: AppAction,
    },
    /// Kiln's optional background daemon (kilnd): keeps the web server running.
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// List what Kiln changed on this machine (newest first) and what is still in effect.
    History {
        /// Include operations that were undone or rolled back.
        #[arg(long)]
        all: bool,
    },
    /// Remove Kiln: stop it, undo every system change (newest first), delete its folders.
    /// Always shows the full plan first; your projects and other tools are never touched.
    Uninstall {
        #[command(flatten)]
        options: uninstall::Options,
    },
    /// Undo a recorded operation by its number (see `kiln history`), or the newest one.
    Undo {
        /// The operation number (see `kiln history`); default: the newest one still in effect.
        id: Option<u64>,
        /// Undo even if the target was edited by hand since (overwrites those edits).
        #[arg(long)]
        force: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ImportFrom {
    /// Write the folder's kiln.toml from its Laravel Herd herd.yml (name, PHP, HTTPS, aliases),
    /// and link the folder if it isn't a site yet.
    Herd {
        /// The project folder (default: the current one).
        path: Option<PathBuf>,
        /// Replace an existing kiln.toml.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DbAction {
    /// Write a database (or all of them) to a .sql file with the service's own tool
    /// (`mysqldump`, `mariadb-dump`, `pg_dump`).
    Dump {
        /// The instance, e.g. mysql-8.4.
        name: String,
        /// The database (default: every one).
        database: Option<String>,
        /// The file to write (default: <instance>-<database>-<time>.sql here). Never overwritten.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// The instance's databases and their size (MySQL, MariaDB, PostgreSQL).
    List {
        /// The instance, e.g. mysql-8.4.
        name: String,
    },
    /// Make an empty database.
    Create {
        /// The instance, e.g. mysql-8.4.
        name: String,
        /// The database.
        database: String,
    },
    /// Delete a database and everything in it, after asking. It can't be undone: dump it
    /// first (`kiln db dump`) to keep a copy.
    Drop {
        /// The instance, e.g. mysql-8.4.
        name: String,
        /// The database.
        database: String,
    },
    /// Load a .sql file into an instance: into a database (made if missing), or the whole
    /// server for a dump of every database.
    Restore {
        /// The instance, e.g. mysql-8.4.
        name: String,
        /// The .sql file.
        file: PathBuf,
        /// The database to load it into.
        database: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum EnvAction {
    /// Set the .env keys for an instance, e.g. `kiln env use mysql-8.4`: shows the old and new
    /// values first. A database instance also gets the project's database, made if missing.
    Use {
        /// The instance (see `kiln service list`).
        name: String,
        /// The project folder (default: the current one).
        #[arg(long)]
        project: Option<PathBuf>,
        /// The database to use (default: the folder's name).
        #[arg(long)]
        database: Option<String>,
        /// Use this Redis instance for Laravel's cache (`CACHE_STORE=redis`).
        #[arg(long)]
        cache: bool,
        /// Use it for sessions (`SESSION_DRIVER=redis`).
        #[arg(long)]
        session: bool,
        /// Use it for queues (`QUEUE_CONNECTION=redis`).
        #[arg(long)]
        queue: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// List a project's .env, secrets hidden unless --reveal (with --json: every key and value,
    /// and which ones are Kiln's).
    Show {
        /// The project folder (default: the current one).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Show the values of secrets (keys with KEY, SECRET, PASSWORD or TOKEN) too.
        #[arg(long)]
        reveal: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ToolAction {
    /// Download (checked against Kiln's pinned SHA-256) and serve a tool: phpmyadmin or adminer.
    Add { name: String },
    /// Stop serving a tool and delete its files.
    Remove { name: String },
}

#[derive(Debug, Subcommand)]
enum ServiceAction {
    /// What Kiln can run: every kind with its versions for this machine, licence and ports.
    Catalog,
    /// Download (checked against Kiln's pinned SHA-256), set up and start a service.
    Add {
        /// What to run, e.g. mailpit.
        kind: String,
        /// A pinned version, e.g. 1.31 (default: the newest Kiln has).
        #[arg(long)]
        version: Option<String>,
        /// Its name (default: <kind>-<major>, e.g. mailpit-1).
        #[arg(long)]
        name: Option<String>,
        /// A port by role, e.g. smtp=2525 (default: the usual one, or the next free one).
        #[arg(long = "port", value_name = "ROLE=PORT")]
        ports: Vec<String>,
        /// For reverb: the Laravel project it runs in (default: the current folder).
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// List the services, their ports and whether they run.
    List,
    /// Start services (all of them without names).
    Start { names: Vec<String> },
    /// Stop services (all of them without names). Their data stays.
    Stop { names: Vec<String> },
    /// Stop and start a service again.
    Restart { name: String },
    /// Move a service to other ports, e.g. `kiln service port mailpit-1 smtp=2525`. A
    /// running service restarts on them; its data stays.
    Port {
        name: String,
        /// New ports by role.
        #[arg(value_name = "ROLE=PORT", required = true)]
        ports: Vec<String>,
    },
    /// Show a service's recent output.
    Logs {
        name: String,
        /// How many lines.
        #[arg(long, default_value_t = 40)]
        lines: usize,
    },
    /// Stop a service and remove it. Its data is kept unless --delete-data is given.
    Remove {
        name: String,
        /// Also delete its data.
        #[arg(long)]
        delete_data: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ServicesAction {
    /// In a project whose kiln.toml has [services]: find or add each one, start it and point
    /// the .env at it (showing the change first). Elsewhere: start every service.
    Up {
        /// The project folder (default: the current one).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Do not ask before changing the .env.
        #[arg(long, short)]
        yes: bool,
    },
    /// Stop every service. Their data stays.
    Down,
    /// Start again what should be running, after its supervisor stopped on its own (a crash
    /// or a reboot). Kiln's daemon runs this; it starts nothing that was stopped.
    #[command(hide = true)]
    Resume,
}

#[derive(Debug, Clone, Copy, Subcommand)]
enum AppAction {
    /// Start Kiln's app in the tray when you sign in; it starts the daemon too. Takes over
    /// Kiln's login item from `kiln daemon enable` (journaled, undoable).
    Enable {
        #[arg(long, short)]
        yes: bool,
    },
    /// Start nothing of Kiln's at sign-in (the app's item and the daemon's).
    Disable {
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum McpAction {
    /// Let assistants start, stop and restart services and undo changes (`on`), or only read
    /// (`off`, the default).
    AllowChanges {
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
}

#[derive(Debug, Subcommand)]
enum UpdatesAction {
    /// Let Kiln ask GitHub for its newest release (`on`), or not (`off`, the default).
    CheckKiln {
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
}

#[derive(Debug, Subcommand)]
enum CacheAction {
    /// Remove the verified copies of downloads Kiln has already installed.
    Clear,
}

#[derive(Debug, Subcommand)]
enum DaemonAction {
    /// Start the daemon for this user (it runs until stopped or you log out).
    Start,
    /// Stop the daemon. The web server keeps running.
    Stop,
    /// Show whether the daemon runs and what it watches.
    Status,
    /// Start the daemon whenever you log in (and now). Undo with `disable` or `kiln undo`.
    Enable {
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Stop starting the daemon at login. A running daemon keeps running.
    Disable {
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Make one call to the daemon's API and print the JSON result.
    Call {
        /// The method, e.g. `web.status`.
        method: String,
        /// Its parameters as JSON.
        params: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ComposerAction {
    /// Download and verify the current stable Composer.
    Install,
}

#[derive(Debug, Subcommand)]
enum ShimsAction {
    /// Install the `php`, `composer`, `node`, `npm`, `npx` and `corepack` shims and put them
    /// first on your user PATH (asks first; undoable).
    Install {
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum PhpAction {
    /// List the PHP versions that can be installed (latest patch of each).
    Available,
    /// Download, verify and install a PHP version, e.g. 8.4 (or its exact latest patch).
    Install {
        version: String,
        /// Linux: use the PHP your distribution installed (php8.3 and php-fpm8.3) instead of
        /// downloading one.
        #[arg(long)]
        system: bool,
        /// Windows: don't add phpredis, which Kiln adds to a new PHP by default (ADR 0072).
        #[arg(long)]
        without_redis: bool,
    },
    /// List the PHP versions Kiln has installed.
    List,
    /// Delete an installed PHP version.
    Remove { version: String },
    /// Install the newest patch of each installed PHP version (or of one, e.g. 8.4) beside
    /// the current one, and switch to it live. The previous patch is kept for `rollback`.
    Update { version: Option<String> },
    /// Go back to the patch the last `kiln php update` replaced, and remove the newer one.
    Rollback { version: Option<String> },
    /// Choose the PHP version that serves sites (applied live if the server runs).
    Default { version: String },
    /// Set how many PHP workers serve sites per version (1–32, default 4; applied live).
    Workers { count: usize },
    /// The extensions of a PHP build: list them, or turn them on and off (undoable).
    Ext {
        #[command(subcommand)]
        action: ExtAction,
    },
    /// Xdebug on demand: a debug worker per PHP version that only requests asking for
    /// Xdebug reach (the `XDEBUG_SESSION` cookie or `?XDEBUG_SESSION_START=1`). Windows only.
    Xdebug {
        #[command(subcommand)]
        action: XdebugAction,
    },
}

#[derive(Debug, Subcommand)]
enum XdebugAction {
    /// Fetch the pinned Xdebug build (checked) and run the debug worker.
    On {
        /// The PHP build: 8.4 (its newest patch) or 8.4.26. Default: the default PHP.
        #[arg(long)]
        php: Option<String>,
    },
    /// Stop the debug worker.
    Off {
        /// The PHP build: 8.4 (its newest patch) or 8.4.26. Default: the default PHP.
        #[arg(long)]
        php: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ExtAction {
    /// Add a PECL extension Kiln pins (redis), checked, and turn it on (ADR 0067).
    Add {
        name: String,
        /// The PHP build: 8.4 (its newest patch) or 8.4.26. Default: the default PHP.
        #[arg(long)]
        php: Option<String>,
    },
    /// Show which extensions are on, off or built in.
    List {
        /// The PHP build: 8.4 (its newest patch) or 8.4.26. Default: the default PHP.
        #[arg(long)]
        php: Option<String>,
    },
    /// Turn extensions on in the build's php.ini, e.g. `kiln php ext on xsl gmp`.
    On {
        #[arg(required = true)]
        names: Vec<String>,
        /// The PHP build: 8.4 (its newest patch) or 8.4.26. Default: the default PHP.
        #[arg(long)]
        php: Option<String>,
    },
    /// Turn extensions off in the build's php.ini.
    Off {
        #[arg(required = true)]
        names: Vec<String>,
        /// The PHP build: 8.4 (its newest patch) or 8.4.26. Default: the default PHP.
        #[arg(long)]
        php: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum NodeAction {
    /// List the newest release of each Node line, with its LTS name.
    Available,
    /// Download, check and install a Node version: 22, 22.11.0, lts, lts/jod or latest.
    Install { version: String },
    /// List the Node versions Kiln has installed.
    List,
    /// Delete an installed Node version (with its global packages).
    Remove { version: String },
    /// Choose the Node version used where no project asks for one: 22, lts, ….
    Default { version: String },
}

#[derive(Debug, Subcommand)]
enum WebAction {
    /// Download (or take from --archive) the pinned Caddy release, verify its SHA-512, install it.
    Install {
        /// Use this already downloaded release archive instead of downloading it.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Start the web server in the background.
    Start {
        /// HTTP port, kept even if busy (remembered). Without it Kiln picks a free one:
        /// 80, else 8080, 8000, 8081, 8888.
        #[arg(long)]
        http_port: Option<u16>,
        /// HTTPS port, kept even if busy (remembered). Without it Kiln picks a free one:
        /// 443, else 8443, 4443, 9443. Only used when a site is secure.
        #[arg(long)]
        https_port: Option<u16>,
        /// Forget ports chosen with --http-port/--https-port and pick free ones again.
        #[arg(long, conflicts_with_all = ["http_port", "https_port"])]
        auto_ports: bool,
    },
    /// Stop the web server.
    Stop,
    /// Apply the current sites and settings to the running server without restarting it.
    Reload,
    /// Stop and start the web server (needed after changing ports).
    Restart,
    /// Show whether the web server is installed and running.
    Status,
    /// Print the Caddy config Kiln would run, without starting anything.
    Config,
}

#[derive(Debug, Subcommand)]
enum CaAction {
    /// Create the root CA (if there is none) and issue a fresh intermediate for the web server.
    Init {
        /// The development TLD, fixed by the root: test (the default), internal, example or
        /// invalid. Only when the root is created; changing it later needs a new root.
        #[arg(long)]
        tld: Option<String>,
    },
    /// Show the CA's certificates, fingerprints and expiry.
    Info,
    /// Trust the root CA for your account: the Windows certificate store (Windows asks you to
    /// confirm), or on Linux each browser's NSS database (Chrome, Firefox).
    Trust {
        /// Do not ask for confirmation in the terminal (Windows still shows its own dialog).
        #[arg(long, short)]
        yes: bool,
        /// Linux: also trust it machine-wide in the system store, for command-line tools
        /// (curl, PHP, Node, git). Asks for an administrator password.
        #[arg(long)]
        system: bool,
    },
    /// Issue a new intermediate certificate for the web server if fewer than 10 days are left
    /// (Kiln's daemon does this by itself). A running server restarts to use it.
    Renew {
        /// Renew now, however long the current one is still valid.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
enum HostsAction {
    /// Make Kiln's block contain exactly these site names (replacing its previous content).
    Set {
        /// Site names, with or without the TLD (e.g. `blog` or `blog.test`).
        #[arg(required = true)]
        names: Vec<String>,
        /// The development TLD; defaults to Kiln's (`test` unless chosen at `kiln ca init`).
        #[arg(long)]
        tld: Option<String>,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Make Kiln's block list exactly the linked and parked sites (and reload the server).
    Sync {
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Remove Kiln's block from the hosts file.
    Clear {
        /// The development TLD; defaults to Kiln's (`test` unless chosen at `kiln ca init`).
        #[arg(long)]
        tld: Option<String>,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum StateAction {
    /// Where the state database is, its schema version and journal summary.
    Info {
        /// State directory to inspect instead of the per-user default.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

fn state_info(dir: Option<PathBuf>) -> Result<(), String> {
    let dir = dir
        .or_else(kiln_state::default_state_dir)
        .ok_or("cannot determine the state directory (APPDATA/HOME not set)")?;
    let db = dir.join(kiln_state::DB_FILE);
    say!("State directory: {}", dir.display());
    if !db.is_file() {
        say!("No state database yet ({} does not exist).", db.display());
        if dir.join(kiln_state::LEGACY_JOURNAL_FILE).is_file() {
            say!("A legacy journal.jsonl is present; it will be imported on first use.");
        }
        return Ok(());
    }
    let store = kiln_state::StateStore::open(&db).map_err(|e| e.to_string())?;
    say!("Database:        {}", db.display());
    say!(
        "Schema version:  {} (this build: {})",
        store.schema_version().map_err(|e| e.to_string())?,
        kiln_state::SCHEMA_VERSION
    );
    let counts = store.counts_by_state().map_err(|e| e.to_string())?;
    let total: u64 = counts.iter().map(|(_, n)| n).sum();
    say!("Operations:      {total}");
    for (state, n) in counts {
        say!("  {state:?}: {n}");
    }
    Ok(())
}

#[derive(Debug, Subcommand)]
enum PlanTarget {
    /// Show what `kiln uninstall` would do (same as `kiln uninstall --dry-run`).
    Uninstall,
    /// Preview Kiln's managed block in the hosts file for the given site names.
    Hosts {
        /// Site names, with or without the TLD (e.g. `blog` or `blog.test`).
        names: Vec<String>,
        /// The development TLD; defaults to Kiln's (`test` unless chosen at `kiln ca init`).
        #[arg(long)]
        tld: Option<String>,
        /// Hosts file to read instead of the system one.
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

pub(crate) fn system_hosts_path() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join("System32\\drivers\\etc\\hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

/// `--tld` as given (it must be one a user can choose), else Kiln's current TLD.
fn tld_arg(tld: Option<&str>) -> Result<Tld, String> {
    match tld {
        Some(tld) => Tld::new(tld).map_err(|e| e.to_string()),
        None => web::current_tld(),
    }
}

fn plan_hosts(names: &[String], tld: Option<&str>, file: Option<PathBuf>) -> Result<(), String> {
    let (_tld, entries) = apply::site_entries(names, tld_arg(tld)?)?;
    let path = file.unwrap_or_else(system_hosts_path);
    let current =
        std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let before = hosts::read_block(&current).map_err(|e| e.to_string())?;
    let updated = hosts::upsert_block(&current, &entries).map_err(|e| e.to_string())?;
    let after = hosts::read_block(&updated).map_err(|e| e.to_string())?;

    say!("Plan: update Kiln's block in {}", path.display());
    for (ip, name) in &before {
        if !after.contains(&(ip.clone(), name.clone())) {
            say!("  - {ip} {name}");
        }
    }
    for (ip, name) in &after {
        if !before.contains(&(ip.clone(), name.clone())) {
            say!("  + {ip} {name}");
        }
    }
    if before == after {
        say!("  (no changes)");
    }
    let other = hosts::remove_block(&current)
        .map_err(|e| e.to_string())?
        .len();
    say!("Lines outside Kiln's block stay unchanged ({other} bytes of other content).");
    say!("Nothing was written. Use `kiln hosts set` to apply.");
    Ok(())
}

/// Whether `command` may change Kiln's state or the machine, and so takes the change lock.
/// `uninstall` doesn't: it stops the daemon and deletes the folder the lock file is in.
fn changes_state(command: &Command) -> bool {
    !matches!(
        command,
        Command::Plan { .. }
            | Command::State { .. }
            | Command::Sites
            | Command::Which { .. }
            | Command::History { .. }
            | Command::Doctor
            | Command::Setup
            | Command::Settings
            | Command::Logs { .. }
            | Command::Footprint
            // It serves for hours; each tool runs its own `kiln`, which locks as it needs.
            | Command::Mcp { action: None }
            | Command::Requests { .. }
            | Command::Updates { action: None }
            | Command::Db {
                action: Some(DbAction::List { .. }),
                ..
            }
            | Command::Env {
                action: EnvAction::Show { .. }
            }
            | Command::Uninstall { .. }
            | Command::Daemon {
                action: DaemonAction::Start
                    | DaemonAction::Stop
                    | DaemonAction::Status
                    | DaemonAction::Call { .. }
            }
            | Command::Ca {
                action: CaAction::Info
            }
            | Command::Php {
                action: PhpAction::Available
                    | PhpAction::List
                    | PhpAction::Ext {
                        action: ExtAction::List { .. }
                    }
            }
            | Command::Node {
                action: NodeAction::Available | NodeAction::List
            }
            | Command::Web {
                action: WebAction::Status | WebAction::Config
            }
            | Command::Service {
                action: ServiceAction::List | ServiceAction::Catalog | ServiceAction::Logs { .. }
            }
            // A client session can last hours and changes nothing of Kiln's.
            | Command::Db { .. }
    )
}

/// Takes `<state>/kiln.lock`, waiting (and saying so) while another change holds it.
fn change_lock() -> Result<Option<std::fs::File>, String> {
    use std::fs::TryLockError;
    let Some(dir) = kiln_state::default_state_dir() else {
        return Ok(None);
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join("kiln.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            say_err!("Waiting for another kiln command (or Kiln's daemon) to finish…");
            file.lock()
                .map_err(|e| format!("cannot lock {}: {e}", path.display()))?;
        }
        Err(TryLockError::Error(e)) => {
            return Err(format!("cannot lock {}: {e}", path.display()));
        }
    }
    Ok(Some(file))
}

/// Exit code when the output's reader went away (128 + SIGPIPE, as for Unix tools).
const CLOSED_OUTPUT: i32 = 141;

/// Ends quietly when the program reading kiln's output has stopped (`kiln … | head`), as
/// Unix tools do, instead of panicking at the next `println!` (I-63). Other panics are
/// reported as before.
fn quiet_on_closed_output() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or_default();
        // Unix: EPIPE "Broken pipe" (32). Windows: "The pipe is being closed" (232).
        if message.starts_with("failed printing to std")
            && (message.contains("os error 32)") || message.contains("os error 232)"))
        {
            std::process::exit(CLOSED_OUTPUT);
        }
        default(info);
    }));
}

fn main() -> ExitCode {
    quiet_on_closed_output();
    let cli = Cli::parse();
    out::start(cli.json, cli.confirmed);
    if cli.confirmed && !cli.json {
        return out::finish(Err(
            "--confirmed goes with --json; on a terminal, use the command's --yes".into(),
        ));
    }
    if matches!(cli.command, Command::Doctor) {
        // A failed check is still a successful read: the final line says so, the exit code
        // is 1 as before (ADR 0053 §1).
        let healthy = doctor::run();
        let code = out::finish(Ok(()));
        return if healthy { code } else { ExitCode::FAILURE };
    }
    if cli.json && is_interactive(&cli.command) {
        return out::finish(Err(
            "this command talks to the terminal, so it doesn't run with --json".into(),
        ));
    }
    // One change at a time, whether from another `kiln` or from the daemon (ADR 0026 §1).
    let result = if changes_state(&cli.command) {
        change_lock().and_then(|_lock| run(cli.command))
    } else {
        run(cli.command)
    };
    out::finish(result)
}

/// Commands that hand the terminal to another program: a database client.
fn is_interactive(command: &Command) -> bool {
    matches!(
        command,
        Command::Db {
            action: None,
            name: Some(_),
            ..
        }
    )
}

fn run_ca(action: CaAction) -> Result<(), String> {
    match action {
        CaAction::Init { tld } => apply::ca_init(tld.as_deref()),
        CaAction::Info => apply::ca_info(),
        CaAction::Trust { yes, system } => apply::ca_trust(yes, system),
        CaAction::Renew { force } => web::ca_renew(force),
    }
}

fn run_php(action: PhpAction) -> Result<(), String> {
    match action {
        PhpAction::Available => php::available(),
        PhpAction::Install {
            version,
            system,
            without_redis,
        } => php::install(&version, system, !without_redis),
        PhpAction::List => php::list(),
        PhpAction::Remove { version } => php::remove(&version),
        PhpAction::Update { version } => php::update(version.as_deref()),
        PhpAction::Rollback { version } => php::rollback(version.as_deref()),
        PhpAction::Default { version } => php::set_default(&version),
        PhpAction::Workers { count } => php::set_workers(count),
        PhpAction::Xdebug { action } => match action {
            XdebugAction::On { php } => xdebug::on(php.as_deref()),
            XdebugAction::Off { php } => xdebug::off(php.as_deref()),
        },
        PhpAction::Ext { action } => match action {
            ExtAction::List { php } => ext::list(php.as_deref()),
            ExtAction::On { names, php } => ext::toggle(&names, php.as_deref(), true),
            ExtAction::Off { names, php } => ext::toggle(&names, php.as_deref(), false),
            ExtAction::Add { name, php } => pecl::add(&name, php.as_deref()),
        },
    }
}

fn run_db(
    action: Option<DbAction>,
    name: Option<String>,
    database: Option<&str>,
) -> Result<(), String> {
    match (action, name) {
        (
            Some(DbAction::Dump {
                name,
                database,
                output,
            }),
            _,
        ) => dump::dump(&name, database.as_deref(), output),
        (
            Some(DbAction::Restore {
                name,
                file,
                database,
            }),
            _,
        ) => dump::restore(&name, &file, database.as_deref()),
        (Some(DbAction::List { name }), _) => dump::list(&name),
        (Some(DbAction::Create { name, database }), _) => dump::create(&name, &database),
        (Some(DbAction::Drop { name, database }), _) => dump::drop(&name, &database),
        (None, Some(name)) => service::db(&name, database),
        (None, None) => {
            Err("give an instance, e.g. `kiln db mysql-8.4` (see `kiln service list`)".into())
        }
    }
}

fn run_services(action: ServicesAction) -> Result<(), String> {
    match action {
        ServicesAction::Up { project, yes } => services_up::up(project, yes),
        ServicesAction::Down => service::stop(&[]),
        ServicesAction::Resume => service::resume(),
    }
}

fn run_env(action: EnvAction) -> Result<(), String> {
    match action {
        EnvAction::Use {
            name,
            project,
            database,
            cache,
            session,
            queue,
            yes,
        } => env::use_instance(&name, project, database, cache, session, queue, yes).map(|_| ()),
        EnvAction::Show { project, reveal } => env::show(project, reveal),
    }
}

fn run_tool(action: ToolAction) -> Result<(), String> {
    match action {
        ToolAction::Add { name } => tool::add(&name),
        ToolAction::Remove { name } => tool::remove(&name),
    }
}

fn run_service(action: ServiceAction) -> Result<(), String> {
    match action {
        ServiceAction::Add {
            kind,
            version,
            name,
            ports,
            project,
        } => service::add(&kind, version.as_deref(), name.as_deref(), &ports, project).map(|_| ()),
        ServiceAction::List => service::list(),
        ServiceAction::Catalog => {
            service::catalog();
            Ok(())
        }
        ServiceAction::Start { names } => service::start(&names),
        ServiceAction::Stop { names } => service::stop(&names),
        ServiceAction::Restart { name } => service::restart(&name),
        ServiceAction::Port { name, ports } => service::port(&name, &ports),
        ServiceAction::Logs { name, lines } => service::logs(&name, lines),
        ServiceAction::Remove { name, delete_data } => service::remove(&name, delete_data),
    }
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Doctor => unreachable!("handled in main"),
        Command::Setup => doctor::setup(),
        Command::Settings => web::settings(),
        Command::Footprint => footprint::footprint(),
        Command::Mcp { action } => run_mcp(action),
        Command::Requests { site } => requests::requests(site.as_deref()),
        Command::Group { site, group } => groups::set(&site, group.as_deref()),
        Command::Updates { action: None } => updates::updates(),
        Command::Updates {
            action: Some(UpdatesAction::CheckKiln { state }),
        } => updates::set_check_kiln(state == "on"),
        Command::Cache {
            action: CacheAction::Clear,
        } => footprint::clear_cache(),
        Command::Logs { source, lines } => logs::logs(source.as_deref(), lines),
        Command::Plan {
            target: PlanTarget::Hosts { names, tld, file },
        } => plan_hosts(&names, tld.as_deref(), file),
        Command::Plan {
            target: PlanTarget::Uninstall,
        } => uninstall::uninstall(uninstall::Options {
            dry_run: true,
            ..uninstall::Options::default()
        }),
        Command::History { all } => history::history(all),
        Command::Uninstall { options } => uninstall::uninstall(options),
        Command::Service { action } => run_service(action),
        Command::Db {
            action,
            name,
            database,
        } => run_db(action, name, database.as_deref()),
        Command::Env { action } => run_env(action),
        Command::Tool { action } => run_tool(action),
        Command::Services { action } => run_services(action),
        Command::State {
            action: StateAction::Info { dir },
        } => state_info(dir),
        Command::Hosts {
            action: HostsAction::Set { names, tld, yes },
        } => tld_arg(tld.as_deref())
            .and_then(|tld| apply::site_entries(&names, tld))
            .and_then(|(tld, entries)| apply::hosts_set(tld, entries, yes)),
        Command::Hosts {
            action: HostsAction::Clear { tld, yes },
        } => tld_arg(tld.as_deref()).and_then(|tld| apply::hosts_set(tld, Vec::new(), yes)),
        Command::Ca { action } => run_ca(action),
        Command::Hosts {
            action: HostsAction::Sync { yes },
        } => web::hosts_sync(yes),
        Command::Link { name, path, secure } => web::link(name, path, secure),
        Command::Unlink { name } => web::unlink(&name),
        Command::Proxy {
            name,
            target,
            secure,
        } => proxy::proxy(&name, &target, secure),
        Command::Unproxy { name } => proxy::unproxy(&name),
        Command::Park { path } => web::park(path),
        Command::Unpark { path } => web::unpark(path),
        Command::Secure { name } => web::set_secure(&name, true),
        Command::Unsecure { name } => web::set_secure(&name, false),
        Command::Sites => web::sites(),
        Command::Isolate { version, site } => php::isolate(&version, site),
        Command::Unisolate { site } => php::unisolate(site),
        Command::Which { path } => php::which(path),
        Command::Composer {
            action: ComposerAction::Install,
        } => php::composer_install(),
        Command::Shims {
            action: ShimsAction::Install { yes },
        } => apply::shims_install(yes),
        Command::Php { action } => run_php(action),
        Command::Node { action } => run_node(action),
        Command::Web { action } => run_web(action),
        Command::New {
            name,
            template,
            path,
            php,
            secure,
            db,
        } => new::new(
            &name,
            &template,
            path,
            php.as_deref(),
            secure,
            db.as_deref(),
        ),
        Command::Import {
            from: ImportFrom::Herd { path, force },
        } => project::import_herd(path, force),
        Command::App { action } => run_app(action),
        Command::Daemon { action } => run_daemon(action),
        Command::Undo { id, force, yes } => apply::undo(id, force, yes),
    }
}

fn run_node(action: NodeAction) -> Result<(), String> {
    match action {
        NodeAction::Available => node::available(),
        NodeAction::Install { version } => node::install(&version),
        NodeAction::List => node::list(),
        NodeAction::Remove { version } => node::remove(&version),
        NodeAction::Default { version } => node::set_default(&version),
    }
}

fn run_mcp(action: Option<McpAction>) -> Result<(), String> {
    match action {
        None => mcp::serve(),
        Some(McpAction::AllowChanges { state }) => mcp::set_allow_changes(state == "on"),
    }
}

fn run_web(action: WebAction) -> Result<(), String> {
    match action {
        WebAction::Install { archive } => web::install(archive),
        WebAction::Start {
            http_port,
            https_port,
            auto_ports,
        } => web::start(http_port, https_port, auto_ports),
        WebAction::Stop => web::stop(),
        WebAction::Reload => web::reload(),
        WebAction::Restart => web::restart(),
        WebAction::Status => web::status(),
        WebAction::Config => web::print_config(),
    }
}

fn run_app(action: AppAction) -> Result<(), String> {
    match action {
        AppAction::Enable { yes } => app::enable(yes),
        AppAction::Disable { yes } => app::disable(yes),
    }
}

fn run_daemon(action: DaemonAction) -> Result<(), String> {
    match action {
        DaemonAction::Start => daemon::start(),
        DaemonAction::Stop => daemon::stop(),
        DaemonAction::Status => daemon::status(),
        DaemonAction::Enable { yes } => daemon::enable(yes),
        DaemonAction::Disable { yes } => daemon::disable(yes),
        DaemonAction::Call { method, params } => daemon::call(&method, params.as_deref()),
    }
}
