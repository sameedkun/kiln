# 0014. php-cgi pools: a Kiln supervisor with staggered recycling behind Caddy

- **Status:** accepted
- **Date:** 2026-10-21
- **Task:** P1-09 · Builds on [ADR 0005](0005-web-server.md), [0011](0011-caddy-integration.md), [0012](0012-caddy-admin-channel.md), [0013](0013-downloads-and-php-runtimes.md)

## Context

Linked sites must run PHP. On Windows there is no PHP-FPM:
- Each `php-cgi.exe -b 127.0.0.1:<port>` process serves **one request at a time** on its own
  socket.
- `PHP_FCGI_CHILDREN` is ignored, because there's no fork.
- After `PHP_FCGI_MAX_REQUESTS` requests (default 500) the process **exits**, which keeps
  leaks in check.

So something must keep a pool of workers alive and restart each one when it exits.

Spike 01 found the two ways this goes wrong:

- **[I-14](../issues.md#i-14):** with equal limits, every worker exits at the same moment. With
  passive health checks (`fail_duration`), the proxy then quarantines every upstream for 10 s.
  That caused a 10 s outage although the workers were back within milliseconds.
- **[I-15](../issues.md#i-15):** capping retries by count (`lb_retries`) still left about 9%
  errors. Bounding them by time (`lb_try_duration 2s`, `lb_try_interval 50ms`) took errors to
  0 over about 100 recycles.

Another tool on the machine (Herd) also runs `php-cgi.exe`, and kills processes by image name
([I-11](../issues.md#i-11)), so Kiln must only ever touch its own processes.

## Decision

### The supervisor

1. **`kiln-supervisor`** is a small separate binary next to `kiln` (like `kiln-helper`). It
   runs in the background, started by `kiln web start`.
   - It isn't a mode of `kiln.exe`, because Windows locks a running executable: rebuilding or
     upgrading `kiln` would fail while sites are being served.
   - It's managed like Caddy: `supervisor.pid` records the PID and executable, so stop/status
     verify the process image first ([ADR 0011](0011-caddy-integration.md) §12).
   - It logs to `supervisor.log`; php-cgi's stderr goes to `php-errors.log`.
   - The code that starts, verifies and stops background processes moves from `kiln-web` into
     `kiln-sys::background`, shared by both.
2. **Desired state is a file.** `pools.json` in the run directory lists each pool: PHP
   version, `php-cgi.exe` path, `php.ini`, worker ports and recycle limits.
   - The supervisor re-reads it when it changes (polling every 250 ms) and reconciles: it
     starts missing workers and stops workers that are no longer listed.
   - No control channel is needed; the file is written atomically.
   - Deleting `pools.json` is the graceful stop: the supervisor ends its workers and exits.
     Kiln terminates it (verified) only if it hasn't exited within 5 s.
3. **Workers are only ever Kiln's own children.** On Windows every worker is assigned to a job
   object with `KILL_ON_JOB_CLOSE`, held by the supervisor.
   - When the supervisor exits for any reason (stop, crash, terminate), Windows ends its
     workers.
   - Nothing is ever killed by image name, so Herd's `php-cgi.exe` processes are never touched.
4. **Respawn:** a worker that exits is started again on the **same port**, so Caddy's upstream
   list never changes.
   - **The exit code tells recycling from failure.** `php-cgi` exits 0 when it reaches
     `PHP_FCGI_MAX_REQUESTS`, which is restarted at once whatever its uptime. A non-zero exit
     is a failure, and is logged.
   - A failure within 1 s of starting backs off (0.25 s, doubling up to 5 s), so a broken
     `php.ini` doesn't spin the CPU. An earlier design treated any exit within 1 s as a crash;
     the real-PHP test showed busy workers legitimately recycle that fast and were wrongly held
     back ([I-51](../issues.md#i-51)).

### Recycling, staggered ([I-14](../issues.md#i-14))

5. Worker *i* of *N* gets `PHP_FCGI_MAX_REQUESTS = base + i × base / N`, with base 500. For
   N = 4 that's 500, 625, 750 and 875.
   - Load is spread evenly, so the workers reach their limits at different times, and each
     respawned worker keeps its own limit, so they stay apart.
   - At most one worker is restarting at a time under steady load.

### Caddy routes

6. When a pool exists, **every site** gets the route Caddy's own `php_fastcgi` shorthand
   expands to (generated as typed JSON, checked against `caddy adapt` output):
   - `vars.root` is set to the site root;
   - a directory with an `index.php` redirects to its trailing-slash form (308);
   - `try_files {path} {path}/index.php index.php` handles front controllers;
   - `*.php` goes to `reverse_proxy` with the `fastcgi` transport (`split_path .php`);
   - everything else goes to `file_server`.

   Laravel's `public/` folder is linked as the site root until P1-10 adds framework drivers.
7. **Load balancing ([I-14](../issues.md#i-14), [I-15](../issues.md#i-15)):** `least_conn`
   (each worker takes one request at a time), `try_duration` 2 s, `try_interval` 50 ms, and
   `retry_match` GET/HEAD.
   - There are **no passive health checks**: no `health_checks`, `fail_duration` or
     `lb_retries`.
   - A recycling worker's refused connection is simply retried on another worker.
   - Tests assert these settings on the generated config.

### Lifecycle

8. **Which PHP:** in P1-09 one pool serves all sites. It uses `php.default` (set by
   `kiln php default <version>`), or else the newest installed version. Workers per pool are
   set by `php.workers` (default 4, allowed 1–32). Per-site versions come with P1-11.
9. **No PHP installed:** sites stay static, as in P1-06, and `kiln web start` says how to add
   PHP.
10. **Start:** pick free loopback ports, write `pools.json`, start the supervisor, wait until
    every worker port accepts connections, then start Caddy with those upstreams.
    - **Stop:** Caddy first (graceful, ADR 0012), then the supervisor (its job takes the
      workers with it).
11. **Changing the default version (or worker count) live:** write `pools.json` with the old
    and new pools, wait until the new one is ready, reload Caddy to the new upstreams, wait
    `RETIRE_GRACE` (5 s) while Caddy's previous config drains, then write `pools.json` with
    the new pool only. `kiln php default` and `kiln php workers` do this when the server runs.
    - **Measured cost:** the one Caddy reload can lose one request.
    - That happens when a request still in Caddy's previous config hits a worker that is
      recycling at that instant. Caddy cancels the old config's context, so it can't retry as
      it normally would, and the client gets a 502 ([I-50](../issues.md#i-50)).
    - With test limits of 10–16 requests per worker this happened on almost every switch.
      With the real limit of 500, the window is a fraction of a millisecond per reload.
      Steady-state recycling has no such window: 0 failures in every run.
12. **Windows only** for now. On Linux, PHP-FPM manages its own workers; that comes with P1-04.

## Consequences

- Linked sites serve PHP with no manual process management, and recycling doesn't cause
  outages.
- Kiln runs two background processes (Caddy, supervisor) plus N workers, all under Kiln's own
  folders and processes. Ending the supervisor can't leave workers behind.
- Workers are restarted, but a request that is *inside* a worker when it crashes still fails.
  That's normal for PHP.

## Alternatives considered

- **No supervisor; `PHP_FCGI_MAX_REQUESTS=0` workers started by the CLI:** no recycling (leaks
  build up), and a crashed worker would stay dead.
- **Caddy managing php-cgi:** Caddy has no process manager.
- **A named-pipe control channel to the supervisor:** more moving parts than a watched file,
  for no benefit at this size.
- **Equal limits / round-robin / passive health checks:** what spike 01 showed to cause
  outages (I-14, I-15).
