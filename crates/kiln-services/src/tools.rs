//! Database web tools served as Kiln sites ([ADR 0043]): phpMyAdmin and Adminer, each a
//! pinned release in its own folder with a few files Kiln writes beside it. They list Kiln's
//! instances from `servers.json`, which the CLI rewrites whenever services change, and log
//! in to them without a password: the servers listen on 127.0.0.1 only.
//!
//! [ADR 0043]: ../../../docs/adr/0043-phpmyadmin-and-adminer.md

use std::path::{Path, PathBuf};

use crate::Release;

/// One web tool.
#[derive(Debug)]
pub struct Tool {
    /// e.g. `phpmyadmin`, as `kiln tool add` takes it.
    pub name: &'static str,
    pub display: &'static str,
    pub license: &'static str,
    /// Its site name: `pma` is served as `pma.test`.
    pub site: &'static str,
    pub release: Release,
}

pub static TOOLS: [Tool; 2] = [
    Tool {
        name: "phpmyadmin",
        display: "phpMyAdmin",
        license: "GPL-2.0, downloaded from phpmyadmin.net at your request",
        site: "pma",
        // Pinned from the `.sha256` file published beside the archive.
        release: Release {
            version: "5.2.3",
            os: "any",
            arch: "any",
            archive: "phpMyAdmin-5.2.3-english.zip",
            url: "https://files.phpmyadmin.net/phpMyAdmin/5.2.3/phpMyAdmin-5.2.3-english.zip",
            sha256: "a59e54436489a9676edc0a8b2294ade4092bdad1c25919f857b20ec96b944920",
            executable: "index.php",
            tree: Some("phpMyAdmin-5.2.3-english"),
        },
    },
    Tool {
        name: "adminer",
        display: "Adminer",
        license: "Apache-2.0 or GPL-2.0",
        site: "adminer",
        // The single-file build, pinned from GitHub's asset digest.
        release: Release {
            version: "6.2.0",
            os: "any",
            arch: "any",
            archive: "adminer-6.2.0.php",
            url: "https://github.com/vrana/adminer/releases/download/v6.2.0/adminer-6.2.0.php",
            sha256: "9047c00dfc8258c6046c6720d7d201eaa0ca016fa0fc554bf0c4c137306e1c16",
            executable: "adminer-6.2.0.php",
            tree: None,
        },
    },
];

/// The tool called `name`.
#[must_use]
pub fn tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// A PHP string literal of `text`, single-quoted.
fn php_string(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// A path as PHP reads it on every OS: forward slashes.
fn php_path(path: &Path) -> String {
    php_string(&path.to_string_lossy().replace('\\', "/"))
}

impl Tool {
    /// The site's folder under `tools` (Kiln's `<local data>/tools/<name>`): the release's
    /// own folder, which Kiln's files go into.
    #[must_use]
    pub fn root(&self, tools: &Path) -> PathBuf {
        crate::release_folder(&tools.join(self.name), &self.release)
    }

    /// The files Kiln writes into [`Tool::root`] after unpacking, with their contents:
    /// configuration reading `servers` (the instance list) at every request. `secret` is
    /// 32 random characters, for phpMyAdmin's cookie encryption.
    #[must_use]
    pub fn files(&self, tools: &Path, servers: &Path, secret: &str) -> Vec<(&'static str, String)> {
        let header = "<?php\n// Written by Kiln (ADR 0043) when it installed this tool.\n";
        let read_servers = format!(
            "$kiln_servers = json_decode((string) @file_get_contents({}), true);\n\
             $kiln_servers = is_array($kiln_servers) ? $kiln_servers : [];\n",
            php_path(servers)
        );
        match self.name {
            "phpmyadmin" => vec![(
                "config.inc.php",
                format!(
                    "{header}// Kiln's MySQL and MariaDB instances, logged in as root: they listen on\n\
                     // 127.0.0.1 only.\n\
                     declare(strict_types=1);\n\n\
                     $cfg['blowfish_secret'] = {secret};\n\
                     $cfg['TempDir'] = {temp};\n\
                     $cfg['VersionCheck'] = false;\n\
                     $cfg['SendErrorReports'] = 'never';\n\n\
                     {read_servers}\
                     $i = 0;\n\
                     foreach ($kiln_servers as $server) {{\n    \
                         if (!in_array($server['kind'] ?? '', ['mysql', 'mariadb'], true)) {{\n        \
                             continue;\n    \
                         }}\n    \
                         $i++;\n    \
                         $cfg['Servers'][$i] = [\n        \
                             'verbose' => (string) $server['name'],\n        \
                             'host' => '127.0.0.1',\n        \
                             'port' => (string) $server['port'],\n        \
                             'auth_type' => 'config',\n        \
                             'user' => 'root',\n        \
                             'password' => '',\n        \
                             'AllowNoPassword' => true,\n    \
                         ];\n\
                     }}\n",
                    secret = php_string(secret),
                    temp = php_path(&tools.join(self.name).join("tmp")),
                ),
            )],
            _ => vec![
                (
                    "index.php",
                    format!(
                        "{header}include __DIR__ . '/{}';\n",
                        self.release.executable
                    ),
                ),
                (
                    // Adminer loads this from the folder it runs in.
                    "adminer-plugins.php",
                    format!(
                        "{header}// Kiln's database instances in the login form, logged in without a\n\
                         // password: they listen on 127.0.0.1 only.\n\n\
                         {read_servers}\
                         $kiln_drivers = ['mysql' => 'server', 'mariadb' => 'server', 'postgres' => 'pgsql'];\n\
                         $kiln_users = ['mysql' => 'root', 'mariadb' => 'root', 'postgres' => 'postgres'];\n\
                         $kiln_list = [];\n\
                         foreach ($kiln_servers as $server) {{\n    \
                             $kind = $server['kind'] ?? '';\n    \
                             if (isset($kiln_drivers[$kind])) {{\n        \
                                 $kiln_list[(string) $server['name']] = [\n            \
                                     'driver' => $kiln_drivers[$kind],\n            \
                                     'server' => '127.0.0.1:' . (int) $server['port'],\n            \
                                     'username' => $kiln_users[$kind],\n        \
                                 ];\n    \
                             }}\n\
                         }}\n\n\
                         class KilnServers extends Adminer\\Plugin {{\n    \
                             private array $servers;\n\n    \
                             function __construct(array $servers) {{\n        \
                                 $this->servers = $servers;\n        \
                                 $chosen = $servers[$_POST['auth']['server'] ?? ''] ?? null;\n        \
                                 if ($chosen) {{\n            \
                                     $_POST['auth']['driver'] = $chosen['driver'];\n            \
                                     $_POST['auth']['username'] = $chosen['username'];\n        \
                                 }}\n    \
                             }}\n\n    \
                             function credentials() {{\n        \
                                 $server = $this->servers[Adminer\\SERVER] ?? null;\n        \
                                 return $server ? [$server['server'], $server['username'], ''] : [Adminer\\SERVER, '', ''];\n    \
                             }}\n\n    \
                             function login($login, $password) {{\n        \
                                 return isset($this->servers[Adminer\\SERVER]);\n    \
                             }}\n\n    \
                             function loginFormField($name, $heading, $value) {{\n        \
                                 if ($name == 'server') {{\n            \
                                     return $heading . Adminer\\html_select('auth[server]', array_keys($this->servers), Adminer\\SERVER) . \"\\n\";\n        \
                                 }}\n        \
                                 if (in_array($name, ['driver', 'username', 'password'], true)) {{\n            \
                                     return '';\n        \
                                 }}\n    \
                             }}\n\
                         }}\n\n\
                         return [new KilnServers($kiln_list)];\n"
                    ),
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_pinned_and_named_for_their_sites() {
        for tool in &TOOLS {
            assert_eq!(tool.release.sha256.len(), 64, "{}", tool.name);
            assert!(tool.release.url.ends_with(tool.release.archive));
            assert!(super::tool(tool.name).is_some());
        }
        assert_eq!(tool("phpmyadmin").unwrap().site, "pma");
        assert!(tool("nope").is_none());
    }

    #[test]
    fn each_tool_lives_in_its_releases_folder() {
        let tools = Path::new("/k/tools");
        assert_eq!(
            tool("phpmyadmin").unwrap().root(tools),
            Path::new("/k/tools/phpmyadmin/phpMyAdmin-5.2.3-english")
        );
        assert_eq!(
            tool("adminer").unwrap().root(tools),
            Path::new("/k/tools/adminer/adminer-6.2.0")
        );
    }

    #[test]
    fn the_configs_read_the_server_list_and_quote_paths_for_php() {
        let tools = Path::new(r"C:\Users\o'neil\Kiln\tools");
        let servers = tools.join("servers.json");
        let pma = tool("phpmyadmin").unwrap().files(tools, &servers, "abc");
        assert_eq!(pma[0].0, "config.inc.php");
        assert!(pma[0].1.contains("$cfg['blowfish_secret'] = 'abc';"));
        assert!(
            pma[0]
                .1
                .contains(r"'C:/Users/o\'neil/Kiln/tools/servers.json'")
        );
        assert!(pma[0].1.contains("'AllowNoPassword' => true"));
        let adminer = tool("adminer").unwrap().files(tools, &servers, "abc");
        let names: Vec<_> = adminer.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["index.php", "adminer-plugins.php"]);
        assert!(
            adminer[0]
                .1
                .contains("include __DIR__ . '/adminer-6.2.0.php';")
        );
        assert!(
            adminer[1]
                .1
                .contains("class KilnServers extends Adminer\\Plugin")
        );
    }
}
