# Kiln documentation

Kiln is a free, open-source, cross-platform local PHP development environment ("Kiln" is a
working name). Start here.

| Document | What it is for | Update when |
|---|---|---|
| [plan.md](plan.md) | The roadmap: phases, scope, exit criteria, and the Phase 1 task breakdown | Scope or order changes |
| [progress.md](progress.md) | Where we are: phase status, what's done, in progress, next, and a dated log | Every working session |
| [issues.md](issues.md) | Every problem hit so far (bugs, environment, design surprises), with cause, fix and status | A problem is found or resolved |
| [dev-setup.md](dev-setup.md) | Building, testing and running spikes on a developer machine, plus the machine footprint | Tooling changes |
| [adr/](adr/) | Architecture decision records: *why* things are the way they are | A significant decision is made |
| [spikes/](spikes/) | Time-boxed experiments and their measured results | A spike is planned or finished |

## Decisions so far

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/0001-record-architecture-decisions.md) | Record decisions as ADRs | accepted |
| [0002](adr/0002-rust-core-and-tauri-gui.md) | Rust for core, CLI and helper; Tauri for the GUI | accepted |
| [0003](adr/0003-license-and-dependency-policy.md) | Apache-2.0, permissive dependencies, copyleft services downloaded at runtime | accepted |
| [0004](adr/0004-reversible-operation-engine.md) | Every system change goes through a reversible, journaled operation engine | accepted, implemented |
| [0005](adr/0005-web-server.md) | Caddy as the web server, driven by its JSON admin API | accepted (spike 01) |
| [0006](adr/0006-windows-dns-strategy.md) | Windows `.test` resolution via a managed hosts block; NRPT rejected for now | accepted (spike 02) |
| [0007](adr/0007-platform-delivery-order.md) | Windows and Linux first, macOS from Phase 3 | accepted |
| [0008](adr/0008-unsafe-confined-to-kiln-sys.md) | `unsafe` only in the audited `kiln-sys` crate (enforced in CI) | accepted (resolves I-38) |
| [0009](adr/0009-windows-privileged-helper.md) | Windows helper: per-batch UAC, private single-instance pipe, HMAC-signed messages, fixed operation list | accepted, implemented (P1-02) |
| [0010](adr/0010-local-certificate-authority.md) | Local CA: `.test`-only (name-constrained) root, DPAPI-protected key, 30-day intermediate for Caddy, per-user reversible trust; Firefox/NSS plan | accepted, implemented (P1-05) |
| [0011](adr/0011-caddy-integration.md) | Caddy integration: generated JSON, sites HTTP by default with per-site HTTPS, pinned `kiln-caddy` binary, admin endpoint off, background process control | accepted, implemented (P1-06) |
| [0012](adr/0012-caddy-admin-channel.md) | Caddy admin endpoint only on a per-user Unix socket; live reload over `/load` with retry; graceful stop | accepted, implemented (P1-07) |
| [0013](adr/0013-downloads-and-php-runtimes.md) | Verified HTTPS downloads (digest known first, OS trust store, cache); PHP NTS runtimes from windows.php.net, latest patch per version only; php.ini defaults; no system-wide installs | accepted, implemented (P1-08) |
| [0014](adr/0014-php-cgi-supervisor.md) | php-cgi pools: separate `kiln-supervisor`, `pools.json` desired state, staggered recycling, kill-on-close job; Caddy `php_fastcgi` routes with time-bounded retries and no passive checks; live pool switch | accepted, implemented (P1-09) |
| [0015](adr/0015-parked-folders-and-drivers.md) | Parked folders (every subfolder a site; links win) and framework drivers that only detect the document root; hosts sync covers parked sites; no Valet code | accepted, implemented (P1-10) |
| [0016](adr/0016-per-site-php-and-shims.md) | Per-site PHP pins (`kiln isolate`), one pool per version in use, `kiln-shim` as `php`/`composer`, verified Composer, reversible user-PATH prepend | accepted, implemented (P1-11) |
| [0017](adr/0017-kiln-doctor.md) | `kiln doctor`: read-only health checks as pure rules over gathered facts, fixes as commands, `--json`; port owners, container detection, Schannel note | accepted, implemented (P1-12) |
| [0018](adr/0018-history-undo-uninstall.md) | `kiln history`, `kiln undo` (newest by default), `kiln uninstall` with full plan/dry run: stop → undo newest first → delete Kiln folders (state last); safety checks | accepted, implemented (P1-13) |
| [0019](adr/0019-coexistence.md) | Coexisting with Herd, XAMPP, Laragon, WampServer: read-only detection, port owners named, shared `.test` names warned, free ports picked automatically (80→8080…, 443→8443…), chosen ports never moved | accepted, implemented (P1-14) |
| [0020](adr/0020-linux-dns-and-privilege.md) | Linux: `.test` names in `/etc/hosts` (nsswitch `files` first; systemd-resolved reads it too); `pkexec kiln-helper --stdio` per batch (sudo fallback), key on stdin; keeps mode/owner/SELinux xattrs; Caddy `.tar.gz` pins; port owners from `/proc`; P1-04 split into parts a–d | accepted, part a implemented |

## Conventions

- Evidence over opinion: claims about behaviour link to a test, a spike result or an ADR.
- Dates are absolute (`YYYY-MM-DD`).
- Status words: **done**, **in progress**, **blocked**, **next**, **later**, **dropped**.
- Issue IDs (`I-NN`) and task IDs (`P1-NN`) are stable. Reference them in commits and PRs.
