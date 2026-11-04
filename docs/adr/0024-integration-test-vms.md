# 0024. Integration tests in VMs: snapshot, full session, uninstall, compare; Herd alongside

- **Status:** accepted; its VMs were deleted on 2026-11-04 ([ADR 0039](0039-windows-first-and-services-catalog.md), amendment): routine runs are on GitHub Actions, and the scripts remain to rebuild a VM
- **Date:** 2026-10-23
- **Task:** P1-15 · Builds on [ADR 0018](0018-history-undo-uninstall.md), [0019](0019-coexistence.md), [0020](0020-linux-dns-and-privilege.md)–[0023](0023-linux-shims-and-path.md); lessons [I-34](../issues.md#i-34)–[I-36](../issues.md#i-36)

## Context

Phase 1's exit criteria are that a full Kiln session followed by `kiln uninstall` leaves a
machine as it was, and that Kiln changes nothing of Laravel Herd's and survives Herd
restarting ([plan](../plan.md#phase-1-mvp-on-windows--linux)). Unit and CI tests can't show
this: they run without UAC, polkit, real trust stores, a desktop keyring or Herd.

What exists:
- the Ubuntu 24.04 Desktop VM (`kiln-ubuntu`, [dev-setup](../dev-setup.md));
- spike 02's Windows 11 VM script. Its last two builds didn't boot ([I-36](../issues.md#i-36)),
  probably because the "press any key to boot from DVD" prompt was answered for a fixed 40 s
  while the 16 GB host was short of memory ([I-35](../issues.md#i-35)).

The host can't run both VMs at once.

## Decision

### The test

1. Each VM runs one **scenario** script (`tests/vm/<os>/scenario.*`), driven from the host
   (SSH for Ubuntu, PowerShell Direct for Windows):
   1. **Snapshot** everything Kiln may touch (§2).
   2. **Full session:** install PHP and Caddy, create the CA, link and park sites (PHP and
      static, HTTP and HTTPS), sync the hosts file, trust the CA (browsers and, on Linux,
      the system store), install the shims, start the server, and check it serves:
      - HTTP and HTTPS pages through the real hosts names;
      - PHP through the shim in a new shell.
   3. `kiln uninstall --yes`.
   4. **Snapshot again and compare.** Any difference fails the test, and the report lists
      it.
2. **What is compared.**
   - **Byte for byte:**
     - Linux: `/etc/hosts`, `~/.profile`, `~/.bashrc`, `~/.zshrc`, the system trust bundle
       (`/etc/ssl/certs/ca-certificates.crt`) and the anchor folder listing.
     - Windows: the hosts file, and the user `PATH` value with its registry type.
   - **Logically:** databases Kiln changes through their own tools, whose bytes legitimately
     differ after an add and a delete:
     - NSS databases: the `certutil -L` listings;
     - the desktop keyring: Kiln's items;
     - the Windows `CurrentUser\Root` store: thumbprints.
   - **Gone:** Kiln's folders, its processes, its listening ports, its journal (the state
     folder).
3. **Test-VM-only settings** make privilege prompts automatic. They exist only in these
   disposable VMs, are set by the VM scripts, and are recorded in the report:
   - **Ubuntu:** a polkit rule allowing the test user `pkexec` without a password
     (`/etc/polkit-1/rules.d/49-kiln-test.rules`), as the user already has password-less
     `sudo` there.
   - **Windows:** auto-logon of the test user, and UAC that elevates administrators without
     asking (`ConsentPromptBehaviorAdmin = 0`; UAC stays on, so Kiln's helper is still
     launched through `runas` as in real use).
   - The scenario that needs a desktop (Windows' certificate dialog) runs in the logged-on
     session through a scheduled task. Windows asks before adding and before deleting a
     root. Each dialog opens behind whatever window has focus, so keystrokes sent from the
     host can miss it, and its buttons are panes that UI Automation can't press. The
     scenario finds the dialog through UI Automation, minimizes the windows that could cover
     it, and reports where Yes is in screen pixels. The host clicks there with the VM's
     synthetic mouse; clicks made inside the session by a scheduled task don't reach the
     dialog.

### Herd coexistence (Windows)

4. The Windows VM gets Laravel Herd (free edition), installed and running before the first
   snapshot. Its state joins the comparison:
   - Herd's hosts block, byte for byte;
   - its settings (`~\.config\herd\config`: `config.json`, `php.json`, Valet's
     `config.json`, its nginx configs), by hash;
   - its server listening on port 80.
5. During the session the scenario also checks:
   - Kiln moved off the ports Herd holds (80, and 443 once Herd serves a site over HTTPS),
     said why, and serves its sites at the URLs it printed;
   - `kiln doctor` reports Herd under "Other tools";
   - **Herd restarting** (quit and start) doesn't stop Kiln's server, whose image name
     `kiln-caddy.exe` Herd's `nginx.exe` clean-up doesn't match
     ([I-11](../issues.md#i-11)), and Kiln's sites still answer afterwards;
   - Herd's own sites still answer after `kiln uninstall`.

### Building the Windows VM

6. `tests/vm/windows/new-vm.ps1` builds `kiln-win11` from the Windows 11 ISO, with the spike's
   unattended install, fixing [I-36](../issues.md#i-36):
   - Dynamic memory (2–4 GB), so the build fits beside an idle host.
   - The DVD prompt is answered by watching the VM's screen instead of for a fixed time.
     After Setup's first reboot, both DVDs are ejected, so later reboots can only come from
     the disk.
   - A `clean` checkpoint is taken only once the test user is logged in and the uptime is
     stable ([I-34](../issues.md#i-34)).
7. Before each run, both VMs return to their `clean` checkpoint, so every run starts from the
   same machine.

## Consequences

- Phase 1's two exit criteria get a repeatable, scripted check instead of manual runs.
- The test-VM settings make privilege prompts automatic. The prompts themselves stay
  covered by the manual checks already done (I-42, P1-04a/b).
- A run downloads PHP and Caddy again each time (uninstall deletes Kiln's cache), which
  costs a few minutes on slow mirrors.
- The runs are manual (`tests/vm/windows/run.ps1`, `tests/vm/ubuntu/run.sh`), not CI:
  GitHub's runners can't host these VMs.

## Alternatives considered

- **CI-hosted VMs:** GitHub's hosted runners can't run these desktop VMs (no Hyper-V
  guests with a desktop session), and Herd needs an interactive install.
- **Containers for Linux:** no desktop keyring, polkit agent, browsers or systemd-resolved
  as a desktop has them.
- **Byte comparison of every database:** NSS and the registry rewrite pages on any change;
  comparing them logically is what "as it was" means for them.
