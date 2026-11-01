# Spike 03: a Redis-compatible cache on Windows

**Feeds:** [ADR 0035](../adr/0035-services.md) §8 (P3-02, P3-06) · **Time box:** 1 day ·
**Runs in:** the Windows test VM (`kiln-win11`, with Herd), and the Ubuntu test VM for the
control. **Not on the dev machine.**

## Question

Laravel projects use Redis for their cache, sessions, queue and rate limits. On Linux, Kiln
runs Valkey (BSD, the Redis fork), which publishes builds for Ubuntu. Valkey has no Windows
build and doesn't support Windows; its project points to WSL.

Can Kiln give Windows users a cache service that Laravel can't tell apart from Redis, with
nothing installed system-wide?

## Candidates

| Candidate | Why | Ruled out because |
|---|---|---|
| **Garnet** (Microsoft Research, MIT) | Speaks Redis's protocol (RESP); official Windows builds on GitHub; Lua scripting behind `--lua` | (under test) |
| Valkey or Redis under WSL | The real thing | Needs WSL, a system-wide Windows feature and a Linux distribution; Kiln installs nothing system-wide |
| Old Windows ports of Redis (MSOpenTech 3.x, tporadowski 5.x) | Native | Unmaintained for years; Redis 5 or older |
| Memurai | Native Redis for Windows | Proprietary; the free edition is for development only, with its own terms |
| Building Valkey for Windows ourselves | The real thing | Valkey relies on POSIX `fork` and more; it isn't a port Kiln can maintain |

## What is checked

A Laravel artisan command, [`KilnSpike.php`](../../spikes/windows-cache/KilnSpike.php),
uses everything Laravel uses Redis for. It goes through the `predis` client, since the
Windows PHP builds have no `phpredis`; Linux uses the same client for a fair comparison.

| Area | Checks | Why it matters |
|---|---|---|
| Cache | put/get/has/forget, `add` (SET NX), increment, many, remember/forever, a 1 s TTL, flush | the everyday cache |
| Locks | a second holder refused, release by the owner only, `block()` waiting for expiry, `restoreLock`, `forceRelease` | owner-checked release is a Lua script |
| Tags | put and get under tags, flushing one tag, `cache:prune-stale-tags` | tagged cache uses sorted sets |
| Sessions | written, then read back by id through a second store | `SESSION_DRIVER=redis` |
| Queue | a job runs; a delayed job waits and then runs; a released job runs again; a failing job lands in `failed_jobs` | pop, migrate and release are Lua scripts |
| Rate limits | `RateLimiter::attempt` 3 of 4; `Redis::throttle` and `Redis::funnel` | the latter two are Lua scripts |
| Other | pipeline, transaction (MULTI/EXEC), SCAN | used under the hood |

**Also recorded for Garnet:**
- whether it runs on a Windows with no .NET installed;
- whether it still works when renamed to a Kiln name (`kiln-garnet.exe`, [I-11](../issues.md#i-11));
- its size, start-up time and memory;
- which addresses it listens on;
- its persistence options;
- what fails without `--lua`.

## How it runs

- **Windows:** `spikes/windows-cache/run-windows.ps1` resets the VM to `clean` with Kiln's
  binaries, then copies in Garnet v2.2.3 (`win-x64-based-readytorun.zip`, SHA-256 checked
  against GitHub's asset digest) and the check. `guest.ps1` then runs in the VM:
  - it makes a Laravel app with Kiln's PHP 8.4 and Composer, plus `predis`;
  - it starts Garnet on `127.0.0.1:6379` with `--lua`, runs the checks, and runs them
    again without `--lua`.
- **Control:** `spikes/windows-cache/run-ubuntu.sh` runs the same check against Valkey
  9.0.7 (Ubuntu 24.04 build, SHA-256 from valkey.io) in the Ubuntu VM. It ends with
  `kiln uninstall --delete-data`.

## Pass criteria

- Every check passes on Garnet, as it does on Valkey. A check that fails on both is a
  mistake in the check, which gets fixed and both rerun.
- Garnet runs from its folder with nothing installed and under a Kiln name, listening
  only where it is told.
- If Garnet passes, Windows' cache is Garnet under the kind `cache` (ADR 0035 §8). If it
  doesn't, Windows has no cache service in Phase 3, and Kiln says so.

Results: [results/03-windows-cache.md](results/03-windows-cache.md).
