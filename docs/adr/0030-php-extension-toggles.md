# 0030. PHP extension toggles

- **Status:** accepted
- **Date:** 2026-10-29
- **Task:** P2-07 · Builds on [ADR 0013](0013-downloads-and-php-runtimes.md), [0018](0018-history-undo-uninstall.md), [0022](0022-linux-php-runtimes-and-fpm.md), [0025](0025-own-static-linux-php-builds.md), [0029](0029-php-patch-updates-and-rollback.md)

## Context

Kiln writes each PHP build's `php.ini` once, at install ([ADR 0013](0013-downloads-and-php-runtimes.md) §11):

- **Windows:** PHP's `php.ini-development`, then a Kiln section that enables the
  extensions most frameworks need (`kiln_php::ini::DEFAULT_EXTENSIONS`). The build ships
  about 40 more as `ext/php_<name>.dll` (`ldap`, `xsl`, `gmp`, `bz2`, …), all off.
- **Linux, Kiln's static builds:** every extension is compiled in. A static binary can't
  load another one, and can't leave out one it has ([ADR 0022](0022-linux-php-runtimes-and-fpm.md)).
- **Linux, a distribution's PHP** (`--system`): extensions are `.so` files in PHP's
  `extension_dir`, loaded by the distribution's `conf.d` files in `/etc`, plus anything in
  Kiln's `php.ini`.

Turning one on means finding `php.ini`, knowing the line to write, and restarting PHP's
workers. Turning one off for a test means editing it back by hand. The Phase 2 exit
criterion asks that turning an extension on and off works and is undoable.

## Decision

1. **Commands:** `kiln php ext list`, `kiln php ext on <name>…` and
   `kiln php ext off <name>…`, each with `--php <version>`.
   - **Which PHP:** the default PHP, or the installed build `--php` names (`8.4` is its
     newest patch, `8.4.26` that patch).
   - **Names:** as `php -m` prints them, any case; `php_zip.dll` and `zip.so` mean `zip`.
   - **One PHP build at a time.** Each patch has its own `php.ini`; §6 says how a toggle
     follows an update.
2. **PHP says what's loaded:** Kiln runs the build's `php -c <its php.ini> -m`, not its own
   reading of `php.ini`. `list` shows, for each extension:
   - **on** or **off** for one it can toggle;
   - **built in** for one compiled into PHP (`date`, `json`, … and everything in a static
     build);
   - **from your distribution** for one a system PHP loads from `/etc/php`.
3. **What can be toggled:**
   - **Windows:** every `ext/php_<name>.dll` the build ships. `opcache` and `xdebug` are Zend
     extensions and get `zend_extension=`; the others `extension=`. Xdebug isn't shipped;
     on-demand Xdebug stays in Phase 4.
   - **Linux, static builds:** nothing. `on` and `off` say why and point to `--system`.
   - **Linux, system PHP:** `on` for any `.so` in its `extension_dir` that isn't loaded.
     `off` only for one Kiln's `php.ini` loads. For one the distribution's `conf.d` loads,
     Kiln names the command (`sudo phpdismod -v 8.3 <name>`). Kiln never edits `/etc`.
4. **The edit is one line, journaled** as `php.ini.extension` ([ADR 0018](0018-history-undo-uninstall.md)),
   user scope, no administrator rights:
   - **On:** appends `extension=<name>` (or `zend_extension=`) at the end of `php.ini`,
     where it wins over earlier lines.
   - **Off:** comments out every active line that loads it, wherever it is, as
     `;[kiln off] extension=<name>`. The marker tells Kiln's lines from PHP's own commented
     examples.
   - **Undo** (`kiln undo`) removes the appended line, or uncomments the marked lines.
     Nothing else in the file changes, so the user's own edits stay. A line already gone
     counts as undone.
   - Already on or already off: nothing to do, nothing journaled.
5. **Checked and applied live:**
   - **After `on`,** Kiln runs `php -m` again. If the extension didn't load (a missing
     library, say), the toggle is undone at once and PHP's startup warning is shown.
   - **Running workers of that build restart** without downtime. A new pool starts beside
     the old one, Caddy switches to it, and the old one retires ([ADR 0014](0014-php-cgi-supervisor.md)).
     `kiln undo` of a toggle restarts them the same way.
   - **The `php` and `composer` shims** start PHP afresh each time, so they see the change at
     once.
6. **Toggles follow the build's lifecycle:**
   - **`kiln php update`** replays the toggles in effect on the old patch onto the new one,
     as new journaled operations. Those for an extension the new build doesn't ship are
     skipped, and the update says so.
   - **`kiln php rollback` and `kiln php remove`** undo the toggles on the folder they
     delete, so `kiln history` never lists changes to a file that's gone.
7. **Other `php.ini` settings stay hand edits.** `kiln php ext list` prints the file's path;
   its Kiln section says "edit freely". A settings command can come later if it's needed.

## Consequences

- **Windows:** any extension the build ships is one command away, and undoable.
- **Linux:** the static builds stay all-or-nothing. That's the trade [ADR 0025](0025-own-static-linux-php-builds.md)
  made, and `list` makes it visible. Loadable extensions need a system PHP.
- **The journal records the change, not the extension's state.** Turning one off and on
  again makes two entries. Undoing the older one restores its own lines and leaves the
  newer one's alone.
- **A failed load is caught before a site sees it,** at the cost of one more `php -m` run
  (a fraction of a second).

## Alternatives considered

- **Rewrite Kiln's whole section from a stored list:** simple to apply, but a byte-exact
  undo would need the old file, and it would also undo the user's own edits.
- **Delete the line instead of commenting it out:** undo would have to remember where it
  was and what it said. The marker keeps both in the file.
- **A `conf.d` scan directory per build:** neat on Linux, but the Windows builds don't use
  one, and two mechanisms would mean twice the testing.
- **Edit the distribution's `conf.d` with `sudo`:** that's the package manager's file, and
  shared with every other user of that PHP. Kiln names the command instead.
