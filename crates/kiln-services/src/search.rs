//! Full-text search engines with their own Java ([ADR 0077]): `OpenSearch` (Apache-2.0) and
//! Elasticsearch (offered under its AGPL option), as `DBngin` offers on Windows. Each instance
//! listens on loopback, single-node, with security off (it can't be reached from beyond this
//! PC), a modest fixed heap, and a config folder of its own copied from the release's.
//!
//! [ADR 0077]: ../../../docs/adr/0077-search-engines.md

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{EXE_SUFFIX, InstanceDirs, Launch, PortRole, Release, ServiceProvider};

/// The heap each instance gets: enough for development, far from Elasticsearch's own default
/// of half the PC's memory.
const HEAP: &str = "512m";

const HTTP: PortRole = PortRole {
    role: "http",
    usual: 9200,
    what: "the REST API, where apps connect",
};
const TRANSPORT: PortRole = PortRole {
    role: "transport",
    usual: 9300,
    what: "node-to-node traffic (a single node keeps it to itself)",
};

fn port(ports: &BTreeMap<String, u16>, role: &PortRole) -> u16 {
    ports.get(role.role).copied().unwrap_or(role.usual)
}

/// The instance's own config folder, copied from the release's when it's made.
fn conf_dir(dirs: &InstanceDirs) -> PathBuf {
    dirs.instance().join("config")
}

/// Copies `from` into `to`, folders and all (a release's small config folder).
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .map_err(|e| format!("cannot copy to {}: {e}", target.display()))?;
        }
    }
    Ok(())
}

/// The data, logs and temp folders, and the instance's config folder from the release's.
fn init(dirs: &InstanceDirs) -> Result<(), String> {
    // OpenSearch refuses to start without its temp folder (`java.io.tmpdir`).
    for dir in [
        &dirs.data,
        &dirs.instance().join("logs"),
        &dirs.instance().join("tmp"),
    ] {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let conf = conf_dir(dirs);
    if !conf.exists() {
        copy_tree(&dirs.bin.join("config"), &conf)?;
    }
    Ok(())
}

/// The settings both take as `-E` options: loopback, its ports, its folders, one node.
fn settings(dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Vec<String> {
    vec![
        "-Enetwork.host=127.0.0.1".to_owned(),
        format!("-Ehttp.port={}", port(ports, &HTTP)),
        format!("-Etransport.port={}", port(ports, &TRANSPORT)),
        format!("-Epath.data={}", dirs.data.display()),
        format!("-Epath.logs={}", dirs.instance().join("logs").display()),
        "-Ediscovery.type=single-node".to_owned(),
    ]
}

/// The program, a Kiln-named link to the bundled Java beside `java.exe` (it finds the JVM
/// from its own folder).
fn java(bin: &Path, name: &str) -> PathBuf {
    bin.join("jdk")
        .join("bin")
        .join(format!("{name}{EXE_SUFFIX}"))
}

/// The Windows builds' bundled JDK, which Kiln runs itself.
fn keep_all(_path: &str) -> bool {
    true
}

// --- OpenSearch -------------------------------------------------------------------------

#[derive(Debug)]
pub struct Opensearch;

/// Pinned by SHA-256 computed on a runner (`OpenSearch` publishes SHA-512: the Scenario checks
/// that too). Released 2026-09-29.
const OPENSEARCH: &[Release] = &[Release {
    version: "3.9.0",
    os: "windows",
    arch: "x86_64",
    archive: "opensearch-3.9.0-windows-x64.zip",
    url: "https://artifacts.opensearch.org/releases/bundle/opensearch/3.9.0/opensearch-3.9.0-windows-x64.zip",
    sha256: "82cf611bc9868266e04fa9d957b03aeaa4dd12002d207fd41cf0360b281e1593",
    executable: "jdk/bin/java.exe",
    tree: Some("opensearch-3.9.0"),
}];

impl ServiceProvider for Opensearch {
    fn kind(&self) -> &'static str {
        "opensearch"
    }

    fn display_name(&self) -> &'static str {
        "OpenSearch"
    }

    fn license(&self) -> &'static str {
        "Apache-2.0"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[HTTP, TRANSPORT]
    }

    fn releases(&self) -> &'static [Release] {
        OPENSEARCH
    }

    fn executable_name(&self) -> &'static str {
        "kiln-opensearch"
    }

    fn executable(&self, bin: &Path) -> PathBuf {
        java(bin, self.executable_name())
    }

    fn keep(&self, path: &str) -> bool {
        keep_all(path)
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        init(dirs)
    }

    fn logs(&self, dirs: &InstanceDirs) -> Vec<PathBuf> {
        vec![dirs.log.clone()]
    }

    /// The JVM as `opensearch.bat` starts it, with its `jvm.options` for the bundled JDK
    /// written out and a fixed heap; then `OpenSearch` with its security plugin off (loopback
    /// only, so no TLS or demo certificates).
    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let home = &dirs.bin;
        let tmp = dirs.instance().join("tmp");
        let mut args = vec![
            format!("-Xms{HEAP}"),
            format!("-Xmx{HEAP}"),
            "-XX:+UseG1GC".to_owned(),
            format!("-Djava.io.tmpdir={}", tmp.display()),
            "-XX:+HeapDumpOnOutOfMemoryError".to_owned(),
            format!("-XX:HeapDumpPath={}", dirs.data.display()),
            format!(
                "-javaagent:{}",
                home.join("agent").join("opensearch-agent.jar").display()
            ),
            "--add-opens=java.base/java.nio=ALL-UNNAMED".to_owned(),
            "--add-modules=jdk.incubator.vector".to_owned(),
            "-Dorg.apache.lucene.store.MMapDirectory.sharedArenaMaxPermits=1".to_owned(),
            "-Dopensearch".to_owned(),
            format!("-Dopensearch.path.home={}", home.display()),
            format!("-Dopensearch.path.conf={}", conf_dir(dirs).display()),
            "-Dopensearch.distribution.type=zip".to_owned(),
            "-Dopensearch.bundled_jdk=true".to_owned(),
            "-cp".to_owned(),
            format!("{}", home.join("lib").join("*").display()),
            "org.opensearch.bootstrap.OpenSearch".to_owned(),
        ];
        args.extend(settings(dirs, ports));
        args.push("-Eplugins.security.disabled=true".to_owned());
        Launch {
            program: self.executable(home),
            args,
            env: BTreeMap::from([
                ("OPENSEARCH_HOME".to_owned(), home.display().to_string()),
                (
                    "OPENSEARCH_PATH_CONF".to_owned(),
                    conf_dir(dirs).display().to_string(),
                ),
                ("OPENSEARCH_TMPDIR".to_owned(), tmp.display().to_string()),
            ]),
            cwd: Some(home.clone()),
            stop_command: None,
        }
    }
}

// --- Elasticsearch ----------------------------------------------------------------------

#[derive(Debug)]
pub struct Elasticsearch;

/// Pinned by SHA-256 computed on a runner (Elastic publishes SHA-512: the Scenario checks that
/// too). Released 2026-11-02.
const ELASTICSEARCH: &[Release] = &[Release {
    version: "9.5.6",
    os: "windows",
    arch: "x86_64",
    archive: "elasticsearch-9.5.6-windows-x86_64.zip",
    url: "https://artifacts.elastic.co/downloads/elasticsearch/elasticsearch-9.5.6-windows-x86_64.zip",
    sha256: "10c809a277f6004629edd9a646c211cd1d013853f596475c0f226647b534cc9b",
    executable: "jdk/bin/java.exe",
    tree: Some("elasticsearch-9.5.6"),
}];

impl ServiceProvider for Elasticsearch {
    fn kind(&self) -> &'static str {
        "elasticsearch"
    }

    fn display_name(&self) -> &'static str {
        "Elasticsearch"
    }

    fn license(&self) -> &'static str {
        "AGPL-3.0 (one of Elasticsearch's licence options; also SSPL and Elastic License 2.0)"
    }

    fn ports(&self) -> &'static [PortRole] {
        &[HTTP, TRANSPORT]
    }

    fn releases(&self) -> &'static [Release] {
        ELASTICSEARCH
    }

    fn executable_name(&self) -> &'static str {
        "kiln-elasticsearch"
    }

    fn executable(&self, bin: &Path) -> PathBuf {
        java(bin, self.executable_name())
    }

    fn keep(&self, path: &str) -> bool {
        keep_all(path)
    }

    fn init_data(&self, dirs: &InstanceDirs, _ports: &BTreeMap<String, u16>) -> Result<(), String> {
        init(dirs)
    }

    /// Its own launcher, as `elasticsearch.bat` runs it (with the environment
    /// `elasticsearch-env.bat` sets), which starts the server's JVM with `ES_JAVA_OPTS`.
    /// Security off and not auto-configured (loopback only), no machine learning process, no
    /// downloads of the IP location database, a fixed heap.
    fn launch(&self, dirs: &InstanceDirs, ports: &BTreeMap<String, u16>) -> Launch {
        let home = &dirs.bin;
        let lib = home.join("lib");
        let classpath = lib.join("*").display().to_string();
        let mut args = vec![
            "-Xms4m".to_owned(),
            "-Xmx64m".to_owned(),
            "-XX:+UseSerialGC".to_owned(),
            "-cp".to_owned(),
            lib.join("tools")
                .join("server-launcher")
                .join("*")
                .display()
                .to_string(),
            "org.elasticsearch.server.launcher.ServerLauncher".to_owned(),
        ];
        args.extend(settings(dirs, ports));
        args.extend([
            "-Expack.security.enabled=false".to_owned(),
            "-Expack.ml.enabled=false".to_owned(),
            "-Eingest.geoip.downloader.enabled=false".to_owned(),
        ]);
        let java = home.join("jdk").join("bin").join("java.exe");
        Launch {
            program: self.executable(home),
            args,
            env: BTreeMap::from([
                ("ES_HOME".to_owned(), home.display().to_string()),
                (
                    "ES_PATH_CONF".to_owned(),
                    conf_dir(dirs).display().to_string(),
                ),
                ("ES_DISTRIBUTION_TYPE".to_owned(), "zip".to_owned()),
                (
                    "ES_JAVA_HOME".to_owned(),
                    home.join("jdk").display().to_string(),
                ),
                ("JAVA".to_owned(), java.display().to_string()),
                ("JAVA_TYPE".to_owned(), "bundled JDK".to_owned()),
                ("ES_CLASSPATH".to_owned(), classpath.clone()),
                ("ES_MODULEPATH".to_owned(), lib.display().to_string()),
                (
                    "LAUNCHERS_CLASSPATH".to_owned(),
                    format!(
                        "{classpath};{};{}",
                        lib.join("launchers").join("*").display(),
                        lib.join("java-version-checker").join("*").display()
                    ),
                ),
                (
                    "SERVER_CLI_CLASSPATH".to_owned(),
                    format!(
                        "{classpath};{}",
                        lib.join("tools").join("server-cli").join("*").display()
                    ),
                ),
                ("ES_JAVA_OPTS".to_owned(), format!("-Xms{HEAP} -Xmx{HEAP}")),
            ]),
            cwd: Some(home.clone()),
            stop_command: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_listen_on_loopback_single_node_with_their_ports_and_security_off() {
        let dirs = InstanceDirs::new(
            Path::new("/k/services"),
            "opensearch",
            "3.9.0",
            "opensearch-3",
        );
        let ports = BTreeMap::from([("http".into(), 9201), ("transport".into(), 9301)]);
        let os = Opensearch.launch(&dirs, &ports);
        assert!(os.args.contains(&"-Enetwork.host=127.0.0.1".to_owned()));
        assert!(os.args.contains(&"-Ehttp.port=9201".to_owned()));
        assert!(os.args.contains(&"-Etransport.port=9301".to_owned()));
        assert!(
            os.args
                .contains(&"-Eplugins.security.disabled=true".to_owned())
        );
        assert!(os.args.contains(&format!("-Xmx{HEAP}")));
        let es = Elasticsearch.launch(&dirs, &ports);
        assert!(
            es.args
                .contains(&"-Expack.security.enabled=false".to_owned())
        );
        assert!(es.args.contains(&"-Ediscovery.type=single-node".to_owned()));
        assert_eq!(es.env["ES_JAVA_OPTS"], format!("-Xms{HEAP} -Xmx{HEAP}"));
        assert_eq!(
            es.program,
            dirs.bin
                .join("jdk")
                .join("bin")
                .join(format!("kiln-elasticsearch{EXE_SUFFIX}"))
        );
    }

    #[test]
    fn a_new_instance_gets_its_own_copy_of_the_config() {
        let root = tempfile::tempdir().unwrap();
        let dirs = InstanceDirs::new(root.path(), "opensearch", "3.9.0", "opensearch-3");
        fs::create_dir_all(dirs.bin.join("config").join("jvm.options.d")).unwrap();
        fs::write(dirs.bin.join("config").join("opensearch.yml"), "# release").unwrap();
        init(&dirs).unwrap();
        assert!(conf_dir(&dirs).join("opensearch.yml").is_file());
        assert!(conf_dir(&dirs).join("jvm.options.d").is_dir());
        assert!(dirs.instance().join("logs").is_dir());
        assert!(dirs.instance().join("tmp").is_dir());
    }
}
