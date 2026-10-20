# Spike 01 results: nginx vs Caddy on Windows

**Date:** 2026-10-20 · **Verdict:** **Caddy** (93 vs 80 points; see [scoring](#scoring)) · **ADR:** [0005](../../adr/0005-web-server.md)

## Environment

| Item | Value |
|---|---|
| Machine | Intel i7-9850H (6 cores / 12 threads), 16 GB, Windows 11 Pro, laptop on the "Balanced" power plan, Defender real-time protection on |
| nginx | 1.30.6 (stable), official Windows zip, PGP signature verified (nginx.org key `D6786CE3…9AF75C0A`) |
| Caddy | 2.11.7 windows_amd64, SHA-512 verified against `checksums.txt` |
| PHP | 8.4.26 NTS x64 (windows.php.net, SHA-256 verified), `php-cgi.exe` FastCGI over loopback TCP, OPcache on, `validate_timestamps=0` |
| App | Laravel 13.37 welcome page, `APP_ENV=production`, `artisan optimize`; static site with a 1 MB file and a 45-byte page |
| Load | oha 1.16.0 on the same machine, 32 connections (64 for small static), 10 s runs, servers **interleaved** over 3 rounds, medians reported |
| Ports | 127.0.0.1:8080/8443 only. Laravel Herd (on 80/443) kept running throughout |

## Results

### PHP (Laravel, stateless sessions), median of 3 rounds

| Scenario | nginx req/s | nginx p99 | Caddy req/s | Caddy p99 |
|---|---|---|---|---|
| 1 php-cgi worker, HTTP/1.1 | 101 | 492 ms | **183** | 284 ms |
| 4 workers, HTTP/1.1 | 399 | 155 ms | 413 | 136 ms |
| 4 workers, TLS + HTTP/2 | 296 | 189 ms | **403** | 128 ms |
| 8 workers, HTTP/1.1 | 460 | 171 ms | **572** | 122 ms |

Errors: **0 in every run** for both servers.

### Static files over TLS, median of 3 rounds

| Scenario | nginx | Caddy |
|---|---|---|
| 1 MB file, HTTP/1.1 | 68 req/s (~68 MB/s) | **1,673 req/s (~1.7 GB/s)** |
| 1 MB file, HTTP/2 | 71 req/s | **937 req/s** |
| 45-byte page, HTTP/2, 64 conns | 3,376 req/s | **7,793 req/s** |

nginx's Windows build is documented as limited: it uses `select()`, only one worker process
does real work, and it handles about 1,024 connections. Large-file throughput on loopback is
where that shows most.

### Robustness under load (4 workers, all zero-error after the fixes below)

| Test | nginx | Caddy |
|---|---|---|
| 20 config reloads in 25 s, alternating +20/−20 sites | 0 errors · reload avg 261 ms, max 328 ms | 0 errors · reload avg 310 ms, max 545 ms |
| Removed sites actually gone after reload | yes | yes |
| `php-cgi` killed 3× during load | 0 errors (`fastcgi_next_upstream`) | 0 errors (dial retry) |
| Worker recycling, `PHP_FCGI_MAX_REQUESTS` 50/60/70/80 | 0 errors, 86 recycles | 0 errors, 101 recycles |

### Footprint

| | nginx | Caddy |
|---|---|---|
| Binary | 6.3 MB | 50.7 MB |
| Working set under load | 25 MB (2 processes) | 66 MB (1 process) |

### TLS and PKI (both servers)

- A root restricted to `.test` names (permitted DNS subtree `test`) works with both. Leaf certs
  verify; an `example.com` leaf signed by the same root is **rejected by OpenSSL** ("permitted
  subtree violation") **and by Windows Schannel** (`0x4000`).
- **Caddy can issue leaf certificates itself from our root** (`pki` app → intermediate → leaf);
  the chain verifies against our root alone. With nginx, Kiln must issue and renew every leaf.
- With `skip_install_trust`, Caddy never touched `CurrentUser\Root` or `LocalMachine\Root`
  (verified before and after).
- HTTP/2 was negotiated by both (ALPN `h2`). Caddy also opened HTTP/3 (UDP, loopback).

## Findings that change Kiln's design

1. **Herd kills every `nginx.exe` on the machine when it starts.** Herd 2.0.x logs
   `[nginx] killed leftover nginx.exe processes` at startup. Herd auto-updated during this spike
   (2.0.0 → 2.0.2) and twice killed the spike's nginx, regardless of path. The killing
   is by image name: our `php-cgi.exe` workers survived. Renaming the binary to
   `kiln-nginx.exe` avoided it. **Kiln must ship server binaries under Kiln-specific image
   names**, and the coexistence tests must include "Herd restarts while Kiln runs".
2. **Synchronized worker recycling causes outages.** With equal `PHP_FCGI_MAX_REQUESTS`, all
   workers exit together. The proxy's passive health check (nginx default `max_fails=1
   fail_timeout=10s`; Caddy `fail_duration`) then quarantines every worker for 10 s, and every
   request fails with `no live upstreams` / 502, even though workers are back within
   milliseconds. The fix, verified at 0 errors:
   - stagger recycle limits per worker (the supervisor owns this);
   - disable passive quarantine for local workers (nginx `max_fails=0`; Caddy: no
     `fail_duration`);
   - Caddy: retry by time (`lb_try_duration 2s`, `lb_try_interval 50ms`) **without**
     `lb_retries`, which capped retries and left about 9% errors.
3. **Caddy binds to all interfaces by default** (`:8080`). Kiln must set `default_bind
   127.0.0.1` (verified: TCP and UDP listeners then loopback only).
4. **Unknown hosts.** Caddy answers an unmatched host with an empty `200`, and nginx serves its
   first (default) server. Kiln must render an explicit catch-all that returns `404` with a
   "site not found, run `kiln link`" page.
5. **Windows revocation checks.** Schannel clients (Git's curl, System32 curl, some .NET and
   PowerShell paths) fail with "revocation status unknown" against a local CA that has no
   CRL/OCSP. Browsers soft-fail. Kiln's docs and `kiln doctor` should explain this.
6. **Laravel file sessions under synthetic load** produced 31,179 session files and cut
   throughput about 10× (session GC scans the directory). That was a benchmark artifact, fixed
   with stateless sessions, but worth a `kiln doctor` hint for apps hammered by tests.
7. **Caddy's admin API** listens on `localhost:2019` (TCP), so any local process can reconfigure
   it. Kiln must bind it to a per-user Unix socket with a restrictive ACL, or at minimum enforce
   an origin and a random port. **Follow-up: verify Unix-socket admin on Windows.**

## Scoring

| Criterion | Weight | nginx | Caddy | Basis |
|---|---|---|---|---|
| Reload safety | 25 | 25 | 25 | 0 errors across 20 reloads each |
| Works with our name-constrained CA without reimplementing PKI | 20 | 12 | 20 | Caddy issues and renews leaves from our root. nginx needs Kiln-side issuance |
| php-cgi pool reliability | 20 | 20 | 19 | Both 0 errors after tuning. Caddy's `lb_retries` default trap |
| Generator simplicity and testability | 15 | 9 | 14 | Caddy: typed JSON over the admin API (serde structs, snapshot-testable, no reload signal). nginx: text templates plus a signal |
| Familiarity and user customization | 10 | 10 | 6 | PHP developers know nginx snippets |
| Footprint and performance | 10 | 4 | 9 | Caddy is 2–25× faster on static files and has better PHP p99. nginx is smaller (6 vs 51 MB) |
| **Total** | 100 | **80** | **93** | |

The difference is 13 points, above the 10-point threshold, and Caddy removes a whole component
(leaf issuance and renewal). **Recommendation: Caddy.**

## Limitations

- Same-machine load generation on a thermally limited laptop. Absolute numbers are indicative
  only; interleaving makes the *relative* comparison fair.
- The first pass used Laravel file sessions. Those PHP numbers are superseded by the stateless
  rerun above. The static-file and robustness results are unaffected.
- WordPress and Unicode/long-path document roots were not tested (time box). Both are queued for
  the Phase 1 integration suite.
- HTTP/3 performance was not measured.

## Reproduce

Scripts are in [`spikes/web-server/`](../../../spikes/web-server): `render.sh` (configs),
`workers.sh` (php-cgi pool supervisor), `server.sh`, `bench.sh`, `run-all.sh`, `run-php.sh`.
Binaries are downloaded and verified into the git-ignored `.cache/` as listed in the spike plan.
