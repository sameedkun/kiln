# Plan

Kiln serves PHP projects at `https://<name>.test` on Windows, Linux and macOS. Per-project PHP
versions, trusted local HTTPS and managed services. Every system change it makes can be
previewed and undone.

## Principles (non-negotiable)

1. **Reversible.** DNS, certificates, PATH and services go through `kiln-ops`: preview,
   journal, verify, undo. Uninstall restores the machine. ([ADR 0004](adr/0004-reversible-operation-engine.md))
2. **Unprivileged by default.** One tiny helper with a fixed list of operations does the few
   admin tasks. No generic "run as admin".
3. **Loopback only.** Nothing listens beyond `127.0.0.1` / `::1` unless the user opts in per site.
   No firewall rules by default.
4. **Coexistence.** Never stop, modify or delete other tools' processes, files or config
   (Herd, XAMPP, Laragon, system PHP/nginx/MySQL). Kiln ships its server binaries under
   Kiln-specific image names ([I-11](issues.md#i-11)).
5. **Clean room.** No code, text or assets from proprietary tools. Upstream open source is
   reused only under its license, with attribution.

## Phases

| Phase | Goal | Platforms | Estimate | Status |
|---|---|---|---|---|
| **0** | Foundations: repo, CI, operation engine, key decisions via spikes | all (CI) | 2–3 weeks | **done** (one optional recheck open) |
| **1** | MVP CLI: serve PHP sites over trusted HTTPS, per-site PHP, clean uninstall | Windows, Linux | 6–8 weeks | **next** |
| 2 | Developer tooling: Node, Composer, Laravel installer, `kiln.toml`, daemon + local API | Windows, Linux | 4–5 weeks | later |
| 3 | Services (MySQL, MariaDB, PostgreSQL, Valkey, Mailpit) + macOS bring-up | + macOS | 6–8 weeks | later |
| 4 | Tauri GUI, log viewer, dev processes, Xdebug on demand, dump capture | all | 6–8 weeks | later |
| 5 | Installers, signed updates, MCP server, share tunnels, external security review → 1.0 | all | 4–6 weeks | later |
| 6 | Backlog: insights, SPX profiler, Windows wildcard DNS, LAN sharing, ARM64 Windows | — | — | later |

Staffing assumption: 2 core Rust engineers, a part-time UI engineer from Phase 4, and part-time
QA/DevOps for VM test infrastructure. That gives about 8–10 months to 1.0.

## Phase 0: foundations (done)

Gate: ADRs accepted ✓ · operation engine passes its property tests ✓ · CI green on 3 OSes ✓.
Details in [progress.md](progress.md#phase-0).

## Phase 1: MVP on Windows + Linux

**Exit criteria**

- On clean Windows 11 and Ubuntu 24.04 VMs, `kiln park ~/Code` serves 3 sample apps
  (Laravel, WordPress, static) at `https://*.test` with a trusted certificate, and per-site
  PHP versions work.
- `kiln uninstall` restores the hosts file, PATH and trust stores **byte-identical** to a
  snapshot taken before install. This is verified automatically in CI VMs.
- With Herd installed and running on the Windows VM, nothing Herd owns changes. Kiln reports
  port conflicts clearly, and it survives Herd restarting.

**Tasks** (IDs are stable; status is tracked in [progress.md](progress.md#phase-1))

| ID | Task | Depends on | Notes / findings to honour |
|---|---|---|---|
| P1-01 | State store (SQLite) and journal migration from JSONL | — | **done** 2026-10-21 (`kiln-state`) |
| P1-02 | `kiln-helper` (Windows): fixed operation list, named pipe with an ACL for the installing user, signed requests | P1-01 | **done** 2026-10-21 ([ADR 0009](adr/0009-windows-privileged-helper.md)) |
| P1-03 | `HostsBlock` DNS provider (Windows): `ReplaceFileW`, read-only refusal, sharing-violation retry, create-if-missing | P1-02 | **done** 2026-10-21 (`kiln_platform::hosts_op`, `kiln_helper::session`) |
| P1-04 | Linux DNS: detect systemd-resolved / NetworkManager, hosts fallback via polkit | P1-01 | — |
| P1-05 | Local CA: `.test`-only (name-constrained), key protected by DPAPI/libsecret, user-store trust by default, Firefox/NSS | P1-02 | **done** 2026-10-21 on Windows ([ADR 0010](adr/0010-local-certificate-authority.md)); libsecret + NSS import with P1-04 |
| P1-06 | Caddy integration: typed JSON config, loopback bind, `skip_install_trust`, catch-all 404, Kiln-named binary | P1-05 | [I-11](issues.md#i-11), [I-16](issues.md#i-16), [I-17](issues.md#i-17) |
| P1-07 | Caddy admin API off TCP (Unix socket on Windows, or random port plus origin enforcement) | P1-06 | [I-20](issues.md#i-20) |
| P1-08 | PHP runtime provider: windows.php.net NTS (SHA-256 verified); static-php-cli / distro on Linux | P1-01 | — |
| P1-09 | Process supervisor: php-cgi pools with **staggered** recycling, PHP-FPM on Linux, health checks | P1-08 | [I-14](issues.md#i-14), [I-15](issues.md#i-15) |
| P1-10 | Sites: park / link / unlink, framework drivers ported from upstream laravel/valet (MIT) | P1-06 | Attribution in THIRD_PARTY_NOTICES |
| P1-11 | Per-site PHP version (`kiln isolate`), shims for `php` / `composer` | P1-08, P1-10 | — |
| P1-12 | `kiln doctor`: ports with owning process, DNS, trust, runtimes, journal consistency, Schannel revocation hint | P1-03..P1-09 | [I-12](issues.md#i-12), [I-13](issues.md#i-13) |
| P1-13 | `kiln plan` / `kiln undo` / `kiln uninstall --dry-run` | P1-01 | — |
| P1-14 | Coexistence detection (Herd, XAMPP, Laragon, system nginx/PHP) | P1-12 | [I-11](issues.md#i-11) |
| P1-15 | Integration test VMs (Windows 11, Ubuntu 24.04): byte-identical uninstall, Herd coexistence | all | Lessons [I-34](issues.md#i-34)–[I-36](issues.md#i-36) |

**Suggested order:** P1-01 → P1-02 → P1-03 → P1-05 → P1-06/07 → P1-08/09 → P1-10/11 →
P1-13 → P1-12/14 → P1-15. Linux tasks (P1-04, the Linux half of P1-08) run in parallel.

## Phase 2–5 scope (summary)

- **Phase 2:** per-user Node shims (honour `.nvmrc`); Laravel installer; PHP patch updates with
  rollback; extension toggles; `kiln.toml` plus `herd.yml` import; proxies; configurable TLD;
  automatic cert renewal; `kilnd` daemon with a versioned local API and per-user autostart.
- **Phase 3:** `ServiceProvider` plug-ins with multiple instances, a port allocator and
  autostart; `.env` key-level wiring with a journal; macOS adapters (resolver file plus stub,
  Keychain, launchd, `SMAppService`, notarization).
- **Phase 4:** Tauri tray app (sites, PHP/Node, services, onboarding that previews every system
  change, journal viewer with undo); log viewer; dev processes; Xdebug cookie routing; dump
  capture via an open-source collector.
- **Phase 5:** MSIX/winget, Homebrew cask, deb/rpm/AppImage; signed update manifests; `kiln mcp`;
  share-tunnel plug-ins; workspaces; external security review; SBOM.

## Out of scope

Agent studio, dictation and AI widgets; Forge/Vapor/Cloud integrations; site preview
screenshots; Docker orchestration.
