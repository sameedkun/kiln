# 0019. Coexisting with other local PHP tools

- **Status:** accepted
- **Date:** 2026-10-22
- **Task:** P1-14 · Builds on [ADR 0011](0011-caddy-integration.md), [0015](0015-parked-folders-and-drivers.md), [0017](0017-kiln-doctor.md); [I-11](../issues.md#i-11)

## Context

Developers rarely start from a clean machine. On the dev machine (surveyed read-only,
2026-10-22):

- **Laravel Herd** is installed in `C:\Program Files\Herd`, with its data in `~\.config\herd`.
  Its `nginx.exe` (`…\Herd\resources\app.asar.unpacked\resources\bin\nginx\nginx.exe`) listens
  on **127.0.0.1:80 and :443**.
- Herd writes 44 `.test` names into the hosts file, between `# Herd generated Hosts. Do not
  change.` and `# End Herd generated Hosts`.
- Herd's `php.bat` and `composer.bat` come first on the user's `PATH`.
- Herd kills every `nginx.exe` when it starts ([I-11](../issues.md#i-11)).

XAMPP (`C:\xampp`), Laragon (`C:\laragon`, also `*.test`) and WampServer (`C:\wamp64`) are
common too. IIS / `http.sys` can hold port 80 as the System process (PID 4).

Kiln already survives this: own binary name, own hosts block, ports chosen by hand. But the
user has to know to pass `--http-port 8080`, and nothing tells them that `blog.test` in the
browser may be Herd's site rather than Kiln's.

## Decision

1. **Kiln never changes another tool.** It doesn't stop their processes, edit their hosts
   blocks, change their config or reorder their `PATH`. It only detects and explains.
   (`kiln shims install` does put Kiln's shims ahead of others, but only when asked, and it
   is undoable, [ADR 0016](0016-per-site-php-and-shims.md).)
2. **Detection (new crate `kiln-coexist`, read-only, Windows first):**
   - **Installed tools**, from well-known folders: Herd (`%ProgramFiles%\Herd`,
     `~\.config\herd`), XAMPP (`C:\xampp`), Laragon (`C:\laragon`), WampServer (`C:\wamp64`,
     `C:\wamp`).
   - **Who holds a port**, from the owning process's path: the tool's folder → that tool;
     otherwise the program name (nginx, Apache `httpd.exe`, Caddy, IIS). PID 4 is reported
     as the Windows HTTP service (IIS / `http.sys`).
   - **Names other tools claim in the hosts file:** every name outside Kiln's block. Names
     inside a block marked by a tool (Herd's markers above) are attributed to that tool.
3. **Ports picked automatically.** `kiln web start` without `--http-port` / `--https-port`
   uses the first free port from a fixed list:
   - HTTP: 80, then 8080, 8000, 8081, 8888;
   - HTTPS: 443, then 8443, 4443, 9443.

   A port counts as free when Kiln can bind it on 127.0.0.1 and, where available, ::1.
   - The choice is saved, so URLs stay the same across restarts, and explained:
     `port 80 is used by Laravel Herd (nginx.exe); Kiln uses 8080`.
   - Automatic ports are re-picked the same way if their port is taken later.
   - A port the user chose with `--http-port` / `--https-port` is never changed silently. A
     conflict there is an error that names the owner and suggests the first free port; a
     refused port isn't saved. The user can run `kiln web start --auto-ports` to go back to
     automatic ports. A port saved before this ADR counts as chosen.
   - Each saved port has a marker saying who picked it (`web.http_port_auto`,
     `web.https_port_auto`), written together with the port ([I-53](../issues.md#i-53)).
4. **Same name, two tools:** if a Kiln site's name is also in another tool's hosts entries,
   `kiln doctor` warns. So do `kiln link` and `kiln park`.
   - The message names the tool and says which URL opens which site, e.g.
     `blog.test is also a Herd site: http://blog.test/ opens Herd's; Kiln's is
     http://blog.test:8080/`.
   - Both entries point to 127.0.0.1, so the port decides which server answers.
5. **`kiln doctor`** gets an **Other tools** area:
   - installed tools (info);
   - their ports (info);
   - shared site names (warning);
   - where `php` on `PATH` comes from (info);
   - with Herd present, a note that Herd stops every `nginx.exe` when it starts, and that
     Kiln's server is `kiln-caddy.exe`, so it isn't affected ([I-11](../issues.md#i-11)).
6. Linux and macOS detection (Valet, Docker-based stacks, distro nginx/Apache) comes with
   P1-04 and Phase 3.

## Consequences

- A first `kiln web start` next to Herd just works, on 8080/8443, and says why.
- Users see name collisions before they wonder why a page shows the wrong site.
- The list of known tools is data in one place; adding one (e.g. DDEV) is a small change.

## Alternatives considered

- **Take ports 80/443 by stopping the other server:** violates rule 1 and the project brief.
- **Random free ports:** URLs would change on every start.
- **Read Herd's configuration for its site list:** more precise, but it depends on another
  tool's private file formats. The hosts file is the shared, stable interface.
