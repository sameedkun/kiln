# 0020. Linux: hosts-file DNS and per-batch privilege through polkit

- **Status:** accepted
- **Date:** 2026-10-22
- **Task:** P1-04 (part a) · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0006](0006-windows-dns-strategy.md), [0008](0008-unsafe-confined-to-kiln-sys.md), [0009](0009-windows-privileged-helper.md)

## Context

Everything in Phase 1 works on Windows. On Linux, Kiln can link sites and run Caddy, but
nothing that needs root works: `kiln hosts sync`, CA trust, `kiln undo` of system changes. PHP
runtimes and shims are Windows-only too.

That is too much for one step, so P1-04 is split:

| Part | Scope |
|---|---|
| **a (this ADR)** | DNS for `.test` names, the privileged helper behind polkit, undo of system changes |
| b | CA: key in the Secret Service (libsecret), trust in the system store and NSS (Chrome, Firefox) |
| c | PHP: runtimes for Linux, PHP-FPM pools under the supervisor |
| d | `php` / `composer` shims on `PATH` |

How Linux desktops resolve names (checked against Ubuntu 24.04 Desktop, the reference system;
Fedora and Debian work the same way here):

- glibc's resolver reads `/etc/nsswitch.conf`. Ubuntu's line is
  `hosts: files mdns4_minimal [NOTFOUND=return] dns`, so `/etc/hosts` (`files`) wins before
  any DNS server is asked. musl (Alpine) always reads `/etc/hosts` first.
- systemd-resolved, behind NetworkManager on Ubuntu, Fedora and most desktops, also answers
  from `/etc/hosts` on its stub at 127.0.0.53 (`ReadEtcHosts=yes` is the default). So does a
  program that queries the stub directly instead of going through glibc.
- Chrome's built-in resolver reads `/etc/hosts`. Firefox with DNS over HTTPS checks
  `/etc/hosts` first (`network.trr.exclude-etc-hosts`, on by default).
- Go programs with the pure-Go resolver read `/etc/hosts` too.

## Decision

### DNS: Kiln's block in `/etc/hosts`

1. As on Windows ([ADR 0006](0006-windows-dns-strategy.md)), `.test` names go in Kiln's
   marker-delimited block in `/etc/hosts`, written by the same code
   (`kiln_platform::hosts`): every byte outside the block is kept, including other tools'
   blocks; only loopback addresses; ambiguous markers are refused.
2. `kiln doctor` detects how this machine resolves names, to explain problems, not to choose
   a strategy:
   - the `hosts:` line of `/etc/nsswitch.conf`: a warning if `files` is missing or comes
     after `dns` / `resolve`, since `/etc/hosts` would then be skipped or overridden;
   - who manages `/etc/resolv.conf`: systemd-resolved (its stub file), NetworkManager, or
     something else (info only);
   - the existing check that every site resolves to this computer.

### Privilege: `pkexec kiln-helper --stdio`, once per batch

3. When a batch needs root, `kiln` starts `pkexec /abs/path/kiln-helper --stdio`. polkit
   asks for the administrator password: a desktop dialog, or a text prompt in the terminal
   when there is no desktop agent (e.g. over SSH). One prompt per user action, as with UAC.
   No daemon is installed and nothing keeps running.
   - Without `pkexec` (minimal systems), `kiln` uses `sudo` instead, which asks on the
     terminal. Without either, the command fails with what to install, and nothing changes.
4. **The channel is the child's stdin and stdout.** Only `kiln` and the helper hold these
   pipes, so no other process can connect, squat or race, which on Windows needed the pipe
   ACL and first-instance flag.
   - The protocol is unchanged: the same frames, HMAC, `seq` rules and operations as
     [ADR 0009](0009-windows-privileged-helper.md) (protocol v2).
   - The session key is sent as the first line on stdin, **never in arguments**: on Linux any
     user can read any process's command line from `/proc`.
5. **Fixed targets, no inherited input.** `pkexec` clears the environment. The helper takes no
   paths from `kiln`: the hosts file is always `/etc/hosts`. The requesting user comes from
   `PKEXEC_UID` (or `SUDO_UID`) and is only logged.
6. **Session limits** as on Windows: one session, the fixed operation list, exit on
   `Shutdown`, end of input, any authentication failure, or after 120 s.
7. The helper runs from wherever `kiln` is, as root. That is the same trust as
   `sudo ./kiln-helper`: whoever can replace the binary is already that user. Packages
   (Phase 5) install it root-owned under `/usr/libexec/kiln` with its own polkit action.

### Writing `/etc/hosts`

8. The same write policy as Windows: create a missing file (reported, so undo removes it),
   keep every byte outside Kiln's block, write atomically.
   - The replacement gets the original's mode, owner **and extended attributes**, including
     the SELinux label (`security.selinux`), before it is renamed over `/etc/hosts`. A file
     created with the wrong label breaks name resolution for confined services on Fedora.
   - A symlinked `/etc/hosts` is refused with an explanation: replacing it would replace the
     link, not the file.
   - If the rename is refused because `/etc/hosts` is a mount point (containers, WSL), the
     helper writes the file in place instead.
   - An immutable file (`chattr +i`) is reported with how to unlock it, never unlocked.
9. Unix FFI (extended attributes) lives in `kiln-sys` ([ADR 0008](0008-unsafe-confined-to-kiln-sys.md))
   through the `libc` crate.

### Undo

10. `kiln undo`, `kiln history` and `kiln uninstall` use the same journal and engine as on
    Windows. Reverting a hosts change goes through the same helper.

## Consequences

- `kiln hosts sync`, `kiln undo` and `kiln uninstall` work on Linux, with one password prompt
  per action.
- No wildcard names (same as Windows); `kiln park` keeps the hosts block in sync.
- The stdin/stdout channel is simpler than the Windows pipe and is tested end to end
  without root: the tests run the server over in-process pipes with a temporary hosts file.
  The `pkexec` launch itself is checked by hand in the Ubuntu test VM.
- Parts b–d of P1-04 still have to land before Linux matches Windows.

## Alternatives considered

- **systemd-resolved routing domain `~test` to a Kiln DNS stub:** wildcards, but it needs a
  resident daemon, a privileged link or `resolved.conf.d` change, and it only covers machines
  running resolved. Reconsider in Phase 2 together with `kilnd`.
- **NetworkManager's dnsmasq plugin:** not the default on Ubuntu or Fedora, and switching it
  changes how the whole machine resolves names.
- **A setuid helper:** a permanent privilege on disk for every user of the machine. pkexec asks
  each time.
- **A polkit action file:** needs root to install, so it belongs to packaging (Phase 5);
  `pkexec`'s generic action works for a user-installed Kiln today.
- **Key in an argument or environment variable:** readable through `/proc`, or cleared by
  pkexec.
