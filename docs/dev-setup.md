# Developer setup

## Requirements

| Platform | Needs |
|---|---|
| Windows 10/11 | Visual Studio 2022 Build Tools (or Community) with the **C++ workload** and a Windows SDK (tested: 10.0.26100); rustup with the **MSVC** host |
| Linux | rustup, a C toolchain (`build-essential` / `gcc`) |
| macOS | rustup, Xcode Command Line Tools |

The Rust version is pinned in `rust-toolchain.toml` (1.99.1). Install it explicitly rather
than relying on auto-install ([I-09](issues.md#i-09)):

```sh
rustup toolchain install 1.99.1 --profile minimal --component clippy rustfmt
```

The GNU toolchain on Windows is **not** supported: its bundled `dlltool` can't link current
`windows-sys` ([I-06](issues.md#i-06)).

Optional: `cargo install cargo-deny --locked` to run the license and advisory checks locally.

`kiln-state` compiles SQLite from source (`rusqlite` with `bundled`), so a C compiler is
required on every platform. That's MSVC on Windows; the toolchains listed above cover this.

## Where things are

| Crate | Role | `unsafe` |
|---|---|---|
| `kiln-core` | Domain types (TLD, host names; `ALLOWED_TLDS`, `Tld::new` for a choice, `Tld::recorded` for reading back) | forbidden |
| `kiln-ops` | Operation engine and journals; `testing` feature exports the journal contract suite | forbidden |
| `kiln-platform` | Adapter traits, hosts block editor, `HostsBlockOp` (+ `PrivilegedHosts`), `TrustCaOp` (Windows), `UserPathOp`, `PhpExtOp`, operation descriptions | forbidden |
| `kiln-state` | SQLite state store (`StateStore`): journal, settings, sites, parked folders, PHP versions, PHP pins, proxies, service instances; JSONL import | forbidden |
| `kiln-pki` | Local CA: name-constrained root, intermediate, protected key store | forbidden |
| `kiln-web` | Caddy config (typed JSON), pinned release install, start/reload/restart/stop/status of `kiln-caddy` (restart validates the new config while the old server serves), admin client over its Unix socket; proxy targets (`ProxyTarget`, loopback only) | forbidden |
| `kiln-fetch` | Verified HTTPS downloads: expected digest first, streaming hash, OS trust store, cache; a stalled or broken download resumes with a Range request (I-74) | forbidden |
| `kiln-php` | PHP runtimes: windows.php.net index, safe extraction, php.ini defaults, start-up check | forbidden |
| `kiln-node` | Node runtimes ([ADR 0028](adr/0028-node-runtimes-and-shims.md)): the nodejs.org index, version specs, a minimal OpenPGP check of `SHASUMS256.txt` against the release keys in `keys/`, safe zip and tar.gz extraction, start-up check | forbidden |
| `kiln-supervisor` | Supervisor library and `kiln-supervisor` binary: php-cgi and PHP-FPM pools, staggered recycling, respawn; programs such as services, stopped gracefully ([ADR 0036](adr/0036-service-framework.md)); CLI handle | forbidden |
| `kiln-services` | Service providers ([ADR 0035](adr/0035-services.md), [ADR 0036](adr/0036-service-framework.md)): the `ServiceProvider` trait and its contract tests, pinned releases and their install under Kiln names, instance folders; whole-tree archives (`.zip`, `.tar.gz`, `.tar.xz`) unpacked through a filter; Mailpit, MySQL, MariaDB ([ADR 0038](adr/0038-mysql-and-mariadb.md)), PostgreSQL ([ADR 0040](adr/0040-postgresql.md)) | forbidden |
| `kiln-sites` | Which sites are served: links + parked folders, framework drivers (document root), each project's `kiln.toml` (settings, aliases; ADR 0027) and reading a Herd `herd.yml` | forbidden |
| `kiln-runtime` | The development TLD (`tld` setting); which PHP and Node run what (default, pins, folder → site; the nearest `kiln.toml` / `.nvmrc` / `.node-version`); the `kiln-shim` binary (`php`, `composer`, `node`, `npm`, `npx`, `corepack`) | forbidden |
| `kiln-doctor` | `kiln doctor` rules: facts → findings (ok/info/warn/fail with fixes), text and JSON reports | forbidden |
| `kiln-coexist` | Other local tools: installed tools, port owners by program path (web servers, PHP, databases, caches, mail catchers, Docker), names other tools claim in the hosts file, free-port picking, system services from `sc.exe` / `systemctl` output (read-only) | forbidden |
| `kiln-sys` | Safe wrappers over OS FFI (`replace_file`, `rename_replace`, pipes, SIDs, UAC, process handles, processes without admin rights, kill-on-close jobs, background processes, `AF_UNIX` connect and listen, user registry strings, console Ctrl+C, TCP listener owners, process parents, DPAPI, cert stores) | **allowed here only** ([ADR 0008](adr/0008-unsafe-confined-to-kiln-sys.md)) |
| `kiln-helper` | Privileged helper: protocol, validation, hosts policy, server/client, lazy `HelperSession`, `kiln-helper` binary | forbidden |
| `kiln-daemon` | `kilnd`, the optional per-user daemon ([ADR 0026](adr/0026-kilnd-daemon-and-local-api.md)): JSON-RPC 2.0 messages, the per-user socket, server, client, web-stack watchdog, rotating log | forbidden |
| `kiln-cli` | The `kiln` binary (`plan hosts`, `state info`, `hosts set/sync/clear`, `ca init/info/trust/renew`, `link`, `unlink`, `park`, `unpark`, `secure`, `unsecure`, `sites`, `web install/start/stop/reload/restart/status/config`, `php available/install/list/remove/default/workers/update/rollback`, `php ext list/on/off`, `node available/install/list/remove/default`, `isolate`, `unisolate`, `which`, `composer install`, `shims install`, `doctor`, `daemon start/stop/status/call/enable/disable`, `import herd`, `new`, `proxy`, `unproxy`, `history`, `undo`, `uninstall`, `plan uninstall`) | forbidden |

## Everyday commands

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

More property-test cases (CI uses 512):

```sh
PROPTEST_CASES=20000 cargo test --workspace
```

End-to-end test against a real Caddy (skipped unless `KILN_TEST_CADDY` is set; uses only
temporary folders and free loopback ports):

```sh
KILN_TEST_CADDY=/path/to/caddy.exe cargo test -p kiln-web --test caddy
KILN_TEST_CADDY=/path/to/caddy.exe KILN_TEST_PHP="$LOCALAPPDATA/Kiln/php/8.4.27" cargo test -p kiln-web --test php
KILN_TEST_CADDY=… KILN_TEST_PHP="$LOCALAPPDATA/Kiln/php/8.4.27" KILN_TEST_PHP2="$LOCALAPPDATA/Kiln/php/8.3.36" KILN_TEST_COMPOSER="$LOCALAPPDATA/Kiln/bin/composer.phar" cargo test -p kiln-web --test multi_php -p kiln-runtime --test shim
```

Run manual checks that touch `%APPDATA%` / `%LOCALAPPDATA%` from a normal terminal, not from a
packaged app's terminal ([I-45](issues.md#i-45)).

Read-only preview against the real hosts file (writes nothing):

```sh
cargo run -p kiln-cli -- plan hosts blog shop
```

## Windows shell notes (Git Bash)

- Use `MSYS_NO_PATHCONV=1` for tools taking `/CN=…`-style arguments ([I-07](issues.md#i-07)).
- Use `/c/...` paths in `PATH`, and `%windir%\System32\whoami.exe` for Windows groups ([I-08](issues.md#i-08)).

## Linux test VM (Ubuntu 24.04 Desktop)

The reference Linux system for P1-04 and P1-15: a real desktop with NetworkManager,
systemd-resolved, polkit and Firefox, from Canonical's Hyper-V image.

1. `tests/vm/ubuntu/new-vm.ps1` (Hyper-V Administrators; about 25 GB free on E:). It
   downloads the image (4.6 GB, resumable), checks Canonical's SHA-256, creates the Gen2 VM
   `kiln-ubuntu` (4 CPUs, 1–4 GB dynamic memory, 64 GB disk) and opens its console.
2. In the console: Ubuntu's first-boot setup, then a Terminal. Paste the SSH setup command
   (it installs `openssh-server`, authorizes the key in `E:\VMs\kiln-ubuntu\ssh`, and gives
   the test user password-less `sudo` for unattended builds; polkit still asks). Then grow
   the disk once: `sudo growpart /dev/sda 1 && sudo resize2fs /dev/sda1`.
3. From Windows (Git Bash): `git archive HEAD | ssh -i /e/VMs/kiln-ubuntu/ssh/id_ed25519
   <user>@<vm-ip> 'mkdir -p ~/kiln && tar -x -C ~/kiln'`, then build and test there with
   rustup (`build-essential`, `curl`, `git`, `pkg-config` from apt).

Over SSH nobody can answer polkit's password prompt: Kiln's privileged commands fail safely
there ("approval was declined; nothing was changed"). Check them in the VM's own Terminal.
The console uses Hyper-V's enhanced session (clipboard works); `tests/vm/type.ps1` types
into a VM in a basic session. Closing the console window doesn't stop the VM:
`vmconnect.exe localhost kiln-ubuntu` reopens it.

More VM notes:
- **Its address changes** when the host restarts (Hyper-V's Default Switch), and the VM may
  be off. `Start-VM kiln-ubuntu`, then find the address from its MAC:
  `Get-NetNeighbor -LinkLayerAddress ((Get-VMNetworkAdapter -VMName kiln-ubuntu).MacAddress -replace '(..)(?!$)','$1-')`.
- **Black screen after logging in:** the enhanced session started a second desktop for a
  user already logged in at the console. Turn off View → Enhanced session to reach the
  console session.
- **Headless Chrome** as a TLS probe:
  `google-chrome --headless=new --password-store=basic --user-data-dir=$(mktemp -d) --host-resolver-rules="MAP hello.test 127.0.0.1" --dump-dom https://hello.test:8443/`.
  The verdict is in its log (`Page load failed: net::ERR_CERT_…`). Without
  `--password-store=basic` it hangs once the desktop keyring is unlocked. Headless Firefox
  (snap) can't screenshot over SSH; check Firefox in its window.
- `kiln ca init` over SSH uses the keyring only if it is already unlocked (someone is logged
  in to the desktop). Otherwise it keeps the key in a `0600` file and says so.
- **Counting or killing processes over SSH:** `pgrep -f <pattern>` / `pkill -f` also match
  the remote shell whose command line contains the pattern, which kills your own SSH
  session. Match by process name instead (`ps -C php-fpm`, `ps -C kiln-supervisor -o pid=`).
- **Long downloads in the VM:** run them with `setsid nohup … &` so a dropped SSH session
  doesn't stop them, resume with `curl -C -`, and compare sizes with the server's
  `Content-Length` before hashing. `dl.static-php.dev` is slow and drops connections.

## Integration tests in VMs (P1-15)

[ADR 0024](adr/0024-integration-test-vms.md): snapshot the machine, run a full Kiln session,
`kiln uninstall --yes`, snapshot again, compare. Run them by hand before a release; CI can't
host these VMs.

- **What they cover:**
  - a static site;
  - a new Laravel project made with Kiln's `composer` shim;
  - WordPress;
  - a PHP site pinned to a second PHP version;
  - HTTPS through Kiln's CA;
  - the shims;
  - `kiln doctor`;
  - Phase 2: project config, Node, `kiln new`, PHP updates and extensions, proxies and
    `kilnd`;
  - a reboot after `kiln daemon enable`, checking what the login brought back
    ([ADR 0034](adr/0034-phase-2-vm-scenarios.md));
  - then uninstall and the comparison.
- **Two parts around a reboot:** `scenario.*` runs the session and asks for a reboot. The
  runner reboots the VM, waits for the automatic login, and runs `after-reboot.*`. Shared
  helpers are in `lib.*`. The work folder is `~/kiln-test` on Ubuntu (kept when a run
  fails) and `C:\kiln-test` on Windows.
- **Ubuntu:** `tests/vm/ubuntu/run.sh <vm-address>` from Git Bash. It copies the working
  tree in, builds it, and runs both parts. One-time setup: `setup-test-vm.sh` (a polkit
  rule for the test user, test VM only). Shut the Windows VM down first.
  - A run cut off midway leaves Kiln set up, and the next run refuses to start. Run
    `~/kiln/target/debug/kiln uninstall --yes` in the VM first.
  - After a host restart the VM's address changes. Find it by its MAC, and check that its
    host key matches the one in `known_hosts` before adding the new address.
- **On CI (Windows, ADR 0039):** `.github/workflows/scenario.yml` runs
  `tests/ci/windows-services.ps1 -Kiln <kiln.exe>` on `windows-latest` on every push to
  `main`: services, ports, data and uninstall, with the VM's snapshot before and after. Its
  log and snapshots are kept as the run's `windows-scenario` artifact. There's no desktop
  and no Herd, so Herd coexistence and the certificate dialog stay with the VM, checked by
  hand before a release.
- **Windows:** `tests/vm/windows/run.ps1 -Bin <folder>` from an elevated PowerShell (Hyper-V).
  - Build the binaries first, with the C runtime linked in:
    `$env:RUSTFLAGS='-C target-feature=+crt-static'; cargo build --release --target-dir target\vm-win`.
  - The VM `kiln-win11` (Windows 11 Pro, user `kiln`, `E:\VMs\kiln-win11`) is built by
    `new-vm.ps1`. `setup-test-vm.ps1` turns off the UAC prompt for administrators and adds
    the Herd test site `~\Herd\herdcheck`.
  - Laravel Herd (free) is installed in it: `Herd-setup.exe /S` as an elevated scheduled
    task, then its onboarding by keyboard. Choices: no Defender exclusion, firewall prompt
    cancelled, AI providers and Herd Pro skipped. The `clean` checkpoint is Herd installed
    and running, with no Kiln.
  - `run.ps1 -Reset` first returns the VM to `clean` and starts it, retrying for a few
    minutes while the host is short of the ~4 GB it needs. Run one VM at a time: shut the
    Ubuntu VM down first.
  - PowerShell Direct (`Invoke-Command -VMName kiln-win11 -Credential (Import-Clixml
    E:\VMs\kiln-win11\vm-credential.xml)`) runs commands in it without a network.
  - Things that need the desktop run as a scheduled task with an Interactive logon.
  - The host clicks with the VM's synthetic mouse at positions the guest reports in screen
    pixels. The guest's resolution changes between boots, so never scale from a screenshot.
  - Synthetic mouse clicks don't reach Herd's window (Electron), but the keyboard does.
  - `C:\kiln-test\scenario.log` keeps the report.

## Running the spikes

Spikes live in `spikes/` (outside the Cargo workspace). Downloads go to each spike's
git-ignored `.cache/` and are checksum- or signature-verified.

| Spike | How |
|---|---|
| Web server | `spikes/web-server/run-all.sh`, then `run-php.sh` (needs ports 8080/8443 free) |
| Windows DNS, Part 1 | Build `spikes/windows-dns/hosts-writer` with `RUSTFLAGS="-C target-feature=+crt-static"`, copy it to `sandbox/tools/`, then `wsb start -c (Get-Content sandbox\part1.wsb -Raw)` and `wsb exec … -r System` ([I-21](issues.md#i-21)) |
| Windows DNS, Part 2 | `vm/new-vm.ps1 -Iso <Win11.iso>` (Hyper-V Administrators, ~30 GB free on the target drive), then `vm/run-part2.ps1`. Use `vm/screen.ps1` to see a VM's display without a console |

Rules: never use ports 80/443, never stop processes you didn't start (match by executable path,
not name), never edit the host machine's hosts file, NRPT table or trust stores. Anything that
does goes in Sandbox or a VM.

## Current machine footprint (from Phase 0 work)

Things that exist on the development machine because of this project, and how to remove them:

| Item | Location | Remove with |
|---|---|---|
| Spike 02 test VM (~20 GB) | `E:\VMs\kiln-spike02` (Hyper-V VM `kiln-spike02`) | `Remove-VM kiln-spike02 -Force`, then delete the folder |
| Empty folder | `D:\VMs` | delete manually |
| Spike caches (PHP, Caddy, nginx, Laravel app, payloads, VM credential) | `spikes/*/.cache/` | delete the folders |
| `cargo-deny` | `%USERPROFILE%\.cargo\bin` | `cargo uninstall cargo-deny` |
| Hyper-V, Windows Sandbox, Hyper-V Administrators membership | Windows features / local group | enabled by the user for spikes |

Not changed by any Phase 0 work: the host hosts file, PATH, certificate stores, firewall,
services, and the existing Laravel Herd installation.
