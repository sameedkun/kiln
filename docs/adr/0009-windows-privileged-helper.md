# 0009. Windows privileged helper: per-batch elevation, private named pipe, authenticated messages

- **Status:** accepted
- **Date:** 2026-10-21
- **Task:** P1-02 · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0006](0006-windows-dns-strategy.md), [0008](0008-unsafe-confined-to-kiln-sys.md)

## Context

A few Kiln operations need administrator rights on Windows: writing Kiln's block in the hosts
file now, and machine-wide trust or similar later. Everything else runs unprivileged. The
helper is the only code that ever runs elevated, so it is the main security boundary:

- **Attackers we design against:** other local users; low-integrity or sandboxed processes;
  network clients; a process that races to create or connect to the helper's channel first;
  replayed or modified messages; and a non-elevated program steering the elevated helper
  through its inputs (arguments, environment, file paths).
- **Out of scope:** malware already running *as the same user* at the same integrity as `kiln`.
  It can drive Kiln's UI or patch its memory anyway. UAC is the user's consent boundary for
  that case.

Existing tools run a resident SYSTEM service that accepts hosts updates on a localhost HTTP
port with no authentication we could find, and they change the hosts file's ACLs while
writing. We want neither.

## Decision

### Lifecycle: elevate per batch, no resident service

1. When a batch of operations needs elevation, `kiln` (unprivileged) starts
   `kiln-helper.exe` with **UAC** (`ShellExecuteExW`, verb `runas`, hidden window). The user
   sees one consent prompt per user action.
2. The helper serves **exactly one client connection** and exits on `Shutdown`, on disconnect,
   on any authentication failure, or after a session deadline (default 120 s). No service is
   installed and nothing stays running.

### Channel: a private, single-instance named pipe

3. `kiln` picks a random pipe name (`\\.\pipe\kiln-helper-<128-bit hex>`) and a fresh 256-bit
   session key, and passes both to the helper with its own process id: `--pipe`, `--key`,
   `--client-pid`. A non-elevated process can't read an elevated process's command line.
4. The helper creates the pipe with:
   - `FILE_FLAG_FIRST_PIPE_INSTANCE` and one instance, so if anyone created the name first,
     creation fails and the helper exits (no squatting);
   - `PIPE_REJECT_REMOTE_CLIENTS` (local only);
   - a protected DACL granting read/write **only to the invoking user's SID** (taken from the
     client process, see 5) and to SYSTEM: `D:P(A;;GA;;;SY)(A;;GRGW;;;<user-sid>)`;
   - a medium mandatory label (`S:(ML;;NW;;;ME)`) so the medium-integrity client can write to
     a pipe created at high integrity.
5. After a client connects, the helper checks `GetNamedPipeClientProcessId` equals
   `--client-pid`. Any other peer is rejected and the helper exits. The DACL user SID is that
   process's token user, which also covers over-the-shoulder elevation, where the helper runs
   as a *different* admin account than the person using Kiln.
6. The client opens the pipe with `SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`, so a rogue
   server could never impersonate the client at a useful level.

### Messages: signed both ways, ordered, size-limited

7. Frames are a 4-byte little-endian length followed by UTF-8 JSON, at most 1 MiB.
8. Every request and response carries `seq` and `mac = HMAC-SHA256(key, "kiln-helper/1" ‖
   direction ‖ seq ‖ body)`, where `direction` is `req` or `resp`. That gives:
   - integrity and authenticity: only the holder of the session key can talk, and the client
     knows the answers come from the helper it launched;
   - no reflection of a request back as a response;
   - no replay or reordering: `seq` must increase strictly.

   MACs are compared in constant time. The first failure ends the session.

### Operations: a fixed list, validated inside the helper

9. The helper understands a closed `enum` of operations and has **no generic command
   execution**:
   - `Ping` → protocol version and helper PID.
   - `HostsSetBlock { tld, entries }` → replace Kiln's hosts block (an empty list removes it).
     The helper validates **everything again itself**: the TLD (public TLDs refused), every
     name under that TLD, loopback IPs only, at most 10,000 entries. It reuses
     `kiln_platform::hosts`, which keeps foreign bytes, refuses ambiguous markers and works
     on bytes ([I-04](../issues.md#i-04)).
   - `Shutdown`.

   Later tasks add operations (CA trust in P1-05) through new ADR-reviewed variants, never a
   passthrough.
10. **Fixed targets.** The hosts path is derived from `GetSystemDirectoryW`, never from
    `SystemRoot` or any other environment variable or argument, because the elevated
    process inherits the caller's environment. The helper accepts no file paths from the client.
11. **Hosts write policy** ([ADR 0006](0006-windows-dns-strategy.md)): write with
    `kiln_sys::write_replacing` (`ReplaceFileW`, keeps ACEs, [I-25](../issues.md#i-25)); refuse
    a read-only file with a clear error code; retry sharing violations with backoff (5
    attempts, 50 ms → 800 ms); create a missing file and report `created: true` so the
    receipt can delete it on revert. Never change ACLs.

### Code layout

12. Win32 FFI (pipe creation, client PID, token SID, system directory, `ShellExecuteExW`)
    lives in `kiln-sys` ([ADR 0008](0008-unsafe-confined-to-kiln-sys.md)). The protocol,
    validation, executor, server loop, client and the `kiln-helper` binary live in the new
    crate `kiln-helper` with no `unsafe`. The server is a library function, so tests can run
    it on a real pipe in-process (unelevated, with a temporary hosts file), while the
    binary always uses the real system path.

### Amendment 1 (2026-10-21, P1-03): protocol v2

Undoing an operation that *created* the hosts file must return to "no hosts file".
`HostsSetBlock` gains `remove_file_if_empty: bool` (default `false`). When it is set and the
file would end up with **zero bytes** after Kiln's block is removed, the helper deletes the
file instead of leaving it empty. A file with any other content is never deleted. The
response reports `deleted`. This is a narrowing of `HostsSetBlock`, not a new capability:
the helper still writes or deletes only the fixed hosts path. The protocol label and version
are now `kiln-helper/2` / `2`; client and helper must match exactly.

## Consequences

- One UAC prompt per user action that touches the system, and none for everything else.
- An attacker who isn't the same user at medium integrity can't open the pipe (DACL), can't
  pre-create it (first instance), can't talk to it without the key (HMAC), can't redirect the
  write (fixed target from the system directory), and can't request anything outside the
  fixed list.
- UAC launching can't be automated in CI. It is covered by a manual test
  (`cargo test -p kiln-sys -- --ignored elevat`), and everything after launch is tested
  automatically on a real pipe.
- Linux uses polkit/pkexec for the same operations (P1-04). The protocol crate is shared.

## Alternatives considered

- **A resident SYSTEM service with a localhost HTTP API:** a permanent elevated attack surface,
  and authentication must be retrofitted. Rejected.
- **Elevated `cmd`/PowerShell scripts:** quoting and injection risks, no structured validation.
  Rejected.
- **Passing the key via a file or environment:** files need careful ACLs, and the environment
  is inherited and readable. The command line of an elevated process is the narrowest channel
  available without a broker.
