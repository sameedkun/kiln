# 0034. Phase 2 in the VM scenarios: a reboot inside the session, timed restarts

- **Status:** accepted
- **Date:** 2026-10-31
- **Task:** P2-11 · Extends [ADR 0024](0024-integration-test-vms.md) · Builds on [ADR 0026](0026-kilnd-daemon-and-local-api.md)

## Context

Phase 2's exit criteria ([plan](../plan.md#phase-2-developer-tooling-windows--linux)) say
the VM scenarios on Windows (with Herd) and Ubuntu verify all of them. Each Phase 2 task
added its checks to both scenarios as it went. Going through the criteria one by one, three
were not fully verified:

1. **`kilnd` starts at login.**
   - The scenarios only checked that `kiln daemon enable` writes the login item.
   - P2-02's real reboots ran in separate scripts (`autostart.ps1`, `autostart.sh`) with a
     small session of their own. Nothing compared the machine after uninstalling a Kiln
     whose daemon the login had started.
2. **The web server restarts for under a second** during a renewal forced through the API.
   - The scenarios checked that the server restarted and that HTTPS worked within 20 s.
   - They never measured how long sites were unreachable.
3. **`npm`** follows the project's Node.
   - The `node` shim was checked in each project.
   - `npm` was only used for building the Laravel app, which runs on the default Node.

The other criteria already had checks in both scenarios:

- `kiln.toml` and `herd.yml` projects, through the site and the shims;
- `php update` and `rollback`;
- extension toggles and their undo (Windows; Kiln's static Linux builds can't toggle);
- `kiln new` over HTTPS;
- a proxy to Vite;
- uninstall coverage of the login item, Node and the shims.

## Decision

1. **A reboot inside the session.** Each scenario becomes two parts with a real reboot
   between them:
   - **The session part** runs from the first snapshot to `kiln daemon enable`. The
     daemon is left running, as a user would leave it. Then it asks the host for a reboot.
   - **The host** (`run.ps1`, `run.sh`):
     - reboots the VM gracefully;
     - waits for the test user's automatic login and a minute for the daemon's first
       watchdog pass;
     - then starts the after-reboot part in the logged-in session.
   - **The after-reboot part** checks what came back on its own:
     - `kilnd` is running, started by the login item, not by the test;
     - the web server and the PHP pools are back, from new processes;
     - the sites answer: static over HTTP, PHP over trusted HTTPS on each site's own PHP;
     - `kiln daemon status` says the server runs and that the daemon starts at login.

     Then it runs the existing stages: uninstall, compare with the first snapshot, and the
     `.internal` stage of [ADR 0033](0033-configurable-tld.md).
   - **Herd (Windows):** the test VM's Herd has no login item, and only its app starts its
     server. So the after-reboot part opens the app, as its user would, and waits for Herd's
     site before the comparison, which includes Herd's port 80. Kiln keeps the ports it
     chose next to Herd, so the two don't collide after the reboot either.
   - **Carried between the parts,** in the test folder rather than in temporary folders,
     which a reboot can clear:
     - the first snapshot;
     - the URLs that `kiln web start` printed;
     - whether anything has failed so far.
   - P2-02's reboot scripts stay. They are quicker, and `-HardReset` covers an unclean
     power-off, which the scenario doesn't.
2. **The renewal restart is timed.**
   - While the API call `ca.renew {"force":true}` runs, a poller requests the static site
     over HTTP every 50 ms.
   - The longest stretch without an answer must be under a second. The scenario reports it
     either way.
   - HTTP shows whether the server is listening. HTTPS also needs Caddy to issue the
     certificates from the new intermediate, which it does in the background after the
     restart ([I-69](../issues.md#i-69)), so HTTPS keeps its own check within 20 s.
   - The first timed run found 8.1 s on Windows ([I-73](../issues.md#i-73)). The restart
     after a CA change now restarts only Caddy (`WebServer::restart`):
     - the PHP workers keep running, since they don't depend on the CA;
     - the new config is written and validated by Caddy while the old server still serves.
       A config Caddy rejects leaves the running server as it was.
     - Only then does the old server stop, and the new one start at once.
3. **`npm` is checked through the project's Node.**
   - In `toml-app`, which pins Node 20, `npm version` must name Node 20: npm prints the
     versions of the Node it runs on.
   - Node 20 rather than the 22 the criterion names: 22 is the default, so only a
     different pin shows that the pin is followed.
4. **The checks are listed by exit criterion** in progress.md's P2-11 section. That makes it
   easy to see which check covers which criterion.

## Consequences

- One run checks a whole Phase 2 session across a reboot, then that uninstall still leaves
  the machine as it was. The `kilnd` that uninstall stops was started by the login, not by
  a command.
- A run takes a few minutes longer: a reboot and a wait for the login.
- A restart after a renewal, a new root or a site change that meets a changed CA no longer
  restarts PHP, and costs sites about a tenth of a second instead of several seconds.
- On Windows, a connection to a port with no listener isn't refused at once: the client
  retries for about 2 s. A request that arrives in the restart's window is answered after
  one retry, about half a second later, rather than failing. The timed check measures what
  a client sees, so it includes that.
- The runner scripts now need to reboot the VM, so they can't be pointed at a machine that
  must stay up. They only ever target the test VMs.
- If the after-reboot part fails because the daemon didn't come back, the uninstall still
  runs, so the VM isn't left half set up.

## Alternatives considered

- **Running the login item's command instead of rebooting.** Rejected. It shows that the
  command works, not that the login runs it, nor that a server whose processes ended at
  shutdown comes back.
- **Running the reboot scripts after the scenario.** Rejected. They set up their own small
  session, and nothing is compared after that session's uninstall.
- **Timing the restart over HTTPS.** Rejected. That would time Caddy's background
  certificate issuance, not the restart.
