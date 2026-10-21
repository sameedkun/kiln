# 0012. Caddy admin endpoint on a per-user Unix socket; live reload

- **Status:** accepted
- **Date:** 2026-10-21
- **Task:** P1-07 · Amends [ADR 0011](0011-caddy-integration.md) §4 (admin disabled) and §13 (restart on change) · Resolves [I-20](../issues.md#i-20)

## Context

Caddy applies new config without dropping connections through its admin API (`POST /load`).
By default that API listens on TCP `localhost:2019`, where **any local process, from any user
account, can reconfigure or stop the server** ([I-20](../issues.md#i-20)). P1-06 therefore
turned the admin endpoint off and restarted Caddy on every site change. That works, but each
change briefly drops the server and anything in flight.

Two ways to protect the admin endpoint were on the table:

1. **A Unix domain socket.** Windows 10 1803+ supports `AF_UNIX`. Access to the socket is
   controlled by the socket file's ACL, so only the user (plus SYSTEM and Administrators)
   can connect.
2. **A random loopback TCP port plus `enforce_origin` / `origins`.** Origin checks stop
   browsers and DNS rebinding, but not other local processes. Any user on the machine can
   still connect to a loopback port.

## Spike (2026-10-21, Caddy 2.11.7, Windows 11)

- `admin.listen = "unix/C:/…/admin.sock"` works on Windows.
  - Caddy creates the socket file, which inherits the folder's ACL.
  - Admin requests over the socket work (`curl --unix-socket`).
  - `POST /stop` shuts Caddy down and removes the socket.
  - No TCP admin port is opened.
- **Path limit:** `sun_path` holds 108 bytes. A 125-byte path failed with a misleading
  "unix socket is already in use by another process".
- **Live reload under load:**
  - With curl, 20 `POST /load` reloads while 1,500 sequential site requests ran gave 0 failed
    requests, and every reload was applied. Each reload took about 150 ms through the
    `caddy reload` CLI.
  - A tighter Rust client opening about 1,400 new connections per second found the real
    figure. About one new connection per 3–7 reloads is closed **without a response** at the
    listener handoff. That held whether a reload changed only a route (3–7 per 20) or added
    and removed the HTTPS server (6–7 per 20). With no reloads it was 0 of 2,700.
  - For comparison, a P1-06 restart takes about 540 ms, and **every** request in that window
    fails. On Windows a refused connection also takes about 2 s to report.
    Tracked as [I-47](../issues.md#i-47).
- **Windows quirk:** on every load, Caddy replaces its admin listener ("stopped previous
  server" → "admin endpoint started"). The next admin request then gets an EOF once; the
  retry succeeds. `caddy reload` doesn't retry, so it failed every other time. Tracked as
  [I-46](../issues.md#i-46).

## Decision

1. **The admin endpoint listens on a Unix socket**: `<local data>/caddy/admin.sock`
   (`%LOCALAPPDATA%\Kiln\caddy\admin.sock` on Windows), on every platform. No TCP admin
   endpoint, ever. `persist: false` stays.
2. **Access control is the file system.**
   - Windows: the socket inherits the per-user profile ACL (the user, SYSTEM,
     Administrators). Other standard users can't open it.
   - Unix: Kiln creates the run directory with mode `0700`.
   - `kiln doctor` (P1-12) will flag a run directory that grants access to broad groups.
3. **Path length:** the generated config refuses socket paths over 107 bytes with a clear
   error. A TCP fallback isn't added. If real users hit the limit, the fallback is a random
   loopback port with a random `origins` host. That would be a new ADR.
4. **Kiln talks to the API directly**, without spawning `caddy` CLI processes:
   - Windows: a small `AF_UNIX` connect in `kiln-sys` (Winsock), returned as an ordinary socket
     stream. Path bytes are UTF-8, as Go (Caddy) passes them.
   - Unix: `std::os::unix::net::UnixStream`.
   - A minimal HTTP/1.1 exchange: `Connection: close`, `Content-Length` or chunked responses.
5. **Live reload:** a site change renders the full config and `POST`s it to `/load`.
   - Caddy validates and applies it atomically, and keeps the old config if it's rejected.
     Kiln shows Caddy's error.
   - Requests are retried (up to 6 times, 50 ms apart) when the connection fails or closes
     before a response, which covers [I-46](../issues.md#i-46). That's safe because loading
     the same config twice is a no-op.
   - The config file on disk is rewritten too, so the next `kiln web start` uses it.
6. **Stop is graceful:** `POST /stop`, then wait up to 10 s for the process to exit. Only if
   the admin call fails or the process doesn't exit does Kiln fall back to the verified
   terminate from [ADR 0011](0011-caddy-integration.md) §12. Any leftover socket file is
   removed when the server is known to be stopped.
7. `kiln web reload` is added. `link`, `unlink`, `secure`, `unsecure` reload instead of
   restarting.

## Consequences

- Site changes no longer take the server down. Running requests finish. At the moment of a
  reload, a brand-new connection can occasionally be closed without a response: about one per
  3–7 reloads under a saturating load, which a person clicking `kiln secure` won't normally
  hit. Refreshing the page fixes it ([I-47](../issues.md#i-47)).
- No network-reachable control surface. Other users on the machine can't reconfigure Kiln's
  server.
- `unsafe` grows slightly in `kiln-sys` (one Winsock connect), covered by tests.
- Administrators can still reach the socket, as they can reach everything else in the profile.
- Windows depends on `AF_UNIX`, available since Windows 10 1803 (2018). Older builds aren't
  supported anyway.

## Alternatives considered

- **Random loopback port + origin checks:** reachable by every local user; rejected while the
  socket works.
- **Named pipe:** Caddy has no named-pipe listener.
- **Shell out to `caddy reload` / `caddy stop`:** no retry (it fails every other time on
  Windows, I-46) and a process per call. Rejected.
- **Keep restarting (ADR 0011):** simpler, but drops connections on every site change.
