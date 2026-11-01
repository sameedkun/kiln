# Spike 03 results: a Redis-compatible cache on Windows

**Date:** 2026-11-01 · **Verdict:** **Garnet, once a release fixes Lua scripts ignoring
`SELECT`** ([microsoft/garnet#2258](https://github.com/microsoft/garnet/issues/2258), fix in
[microsoft/garnet#2311](https://github.com/microsoft/garnet/pull/2311), open). Until then,
Windows has no cache service, and Kiln says so. · **Plan:** [03-windows-cache.md](../03-windows-cache.md)
· **ADR:** [0035](../../adr/0035-services.md) §8 (amended)

## Environment

| Item | Value |
|---|---|
| Windows | `kiln-win11` test VM, Windows 11 Pro 10.0.26200, from its `clean` checkpoint (Herd installed, no .NET) |
| Garnet | v2.2.3, `win-x64-based-readytorun.zip` (SHA-256 `5cd13068…bf9f16`, GitHub's asset digest), run as `kiln-garnet.exe --bind 127.0.0.1 --port 6379 --lua` |
| .NET | runtime 10.0.14, `dotnet-runtime-10.0.14-win-x64.zip` (SHA-512 from Microsoft's release metadata), unpacked into the spike folder and found through `DOTNET_ROOT` |
| Control | `kiln-ubuntu` test VM, Ubuntu 24.04, Valkey 9.0.7 (`valkey-9.0.7-noble-x86_64.tar.gz`, SHA-256 from valkey.io), `--bind 127.0.0.1 --save ""` |
| App | Laravel 13.39.0 made by Kiln's Composer with Kiln's PHP 8.4.27, plus `predis/predis` (Windows PHP has no `phpredis`), `CACHE_STORE=redis` |
| Check | [`KilnSpike.php`](../../../spikes/windows-cache/KilnSpike.php), 22 checks |

## Results

### Laravel's checks

| Run | Passed | Failed |
|---|---|---|
| Valkey 9.0.7 (control) | **22 / 22** | — |
| Garnet 2.2.3, `--lua` | 19 / 22 | `Cache::add`, tag flush, rate limiter |
| Garnet 2.2.3, `--lua`, `REDIS_CACHE_DB=0` | **22 / 22** | — |
| Garnet 2.2.3 without `--lua` | 9 / 22 | everything that runs a script: locks, `add`, tags, the whole queue, all three limiters |

The first control run caught three mistakes in the checks, which failed on Valkey too:
- a string-versus-number comparison in `add`;
- a session test that went through a second session store;
- the rate limiter's attempt count, which comes back as a string.

They were fixed, the control passed 22 of 22, and Garnet was run again with the fixed
checks.

### Why the three fail on Garnet

What the failing checks reported:

- `add('b', 'first')` returned true, but `get('b')` returned `null`.
- The rate limiter allowed 4 of 4 attempts and counted 1.
- `flush()` on one tag left its entry readable.

Each runs a Lua script on Laravel's cache connection, which uses **database 1**
(`REDIS_CACHE_DB`). The raw commands ([`probe.php`](../../../spikes/windows-cache/probe.php),
[`probe-db.php`](../../../spikes/windows-cache/probe-db.php)) show why:

| On a connection that selected database 1 | Garnet 2.2.3 |
|---|---|
| `SET` then `GET` | works in database 1 |
| `EVAL "redis.call('set', …)"` | writes to **database 0** |
| `EVAL "redis.call('get', …)"` of a key in database 1 | `nil` |
| `EVAL "redis.call('del', …)"` / `zrem` of a key in database 1 | 0, the key stays |

The same scripts on database 0 behave correctly, and the raw `add` script works there. That
is why everything on Laravel's default connection, database 0, passes:
- the queue's Lua scripts;
- locks (Laravel takes cache locks through the `default` connection);
- `Redis::throttle` and `funnel`.

This is Garnet's open issue
[microsoft/garnet#2258](https://github.com/microsoft/garnet/issues/2258), "Lua scripts ignore
SELECT and always run against database 0" (opened 2026-10-18). Its fix,
[microsoft/garnet#2311](https://github.com/microsoft/garnet/pull/2311), was opened 2026-10-28
and wasn't merged on 2026-11-01. v2.2.5 (2026-10-28) doesn't contain it.

### Why not work around it with one database

With `REDIS_CACHE_DB=0`, all 22 checks pass. But Laravel keeps the cache in its own database
so that `php artisan cache:clear` (`FLUSHDB`) only clears the cache. In one shared database,
clearing the cache would also delete queued jobs and every session, without a word. That's
the half-working service ADR 0035 §8 rules out.

### Garnet itself

| Question | Answer |
|---|---|
| Runs with nothing installed? | **Not by itself.** The Windows build is framework-dependent: "You must install .NET to run this application", with no runtime found. With Microsoft's .NET 10 runtime zip unpacked into its own folder and `DOTNET_ROOT` set for the process, it runs. Nothing is registered: no `HKLM\SOFTWARE\dotnet`, no `Program Files\dotnet`, no machine or user `DOTNET_ROOT` |
| Size | Garnet 80.1 MB (84 files) plus the .NET runtime 76.9 MB (193 files) |
| Under a Kiln name | Yes: renamed `kiln-garnet.exe`, it runs, and its process name is `kiln-garnet` |
| Listens | Only where told: `127.0.0.1:6379` |
| Start-up | 2.4–2.8 s to accepting connections |
| Memory after the checks | 73 MB working set, 373 MB private (default `--memory 16g` log, mostly reserved) |
| Lua | Off by default; `--lua` is needed. Without it, every script fails with "Lua scripting support disabled" |
| Persistence | In memory by default. `--checkpointdir`, `--recover` and `--aof` exist; not tested here |
| Reports itself as | `redis_version: 7.4.3`, `garnet_version: 2.2.3` |

## Decision

1. **Garnet is the cache for Windows, once a release contains the `SELECT` fix and passes
   this spike's 22 checks unchanged,** with Laravel's own default of a separate cache
   database. Until then, `kiln service add cache` on Windows says there is no cache
   service for Windows yet, and why.
2. **When it ships, P3-06 runs it as:**
   - Microsoft's .NET runtime zip, kept by Kiln in its own folder, pinned by Microsoft's
     SHA-512, and given to Garnet through `DOTNET_ROOT`, never installed. Kiln takes on
     updating that runtime with .NET's security patches.
   - `kiln-garnet.exe` on loopback with `--lua` and a smaller `--memory`. The size is
     measured then.
3. **Linux uses Valkey** from valkey.io, as ADR 0035 planned (P3-06).
4. **Rerunning the spike is the gate:** `spikes/windows-cache/run-windows.ps1` with the new
   release's pin.

## Commands

```powershell
# Windows (host, elevated): resets kiln-win11, then runs guest.ps1 inside
spikes\windows-cache\run-windows.ps1
```

```bash
# Control (host, Git Bash), after tests/vm/ubuntu/run.sh built Kiln in the VM
sh spikes/windows-cache/run-ubuntu.sh <vm-address>
```

The probes ran in the VM against the app the spike made, with Garnet started as above:
`php probe.php C:\kiln-spike\app` and `php probe-db.php C:\kiln-spike\app`.
