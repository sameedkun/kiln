# 0072. phpredis with every new PHP

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-08 · Amends [ADR 0067](0067-pecl-extensions.md)

## Context

ADR 0067 made phpredis something to add per PHP version (`kiln php ext add redis`). Herd
ships it with every PHP, and the user asked for the same (2026-11-14): Laravel's Redis
connection defaults to the `phpredis` client, so a fresh app pointed at Kiln's Redis fails
until it's added or `REDIS_CLIENT=predis` is set.

## Decision

1. **`kiln php install <version>`** on Windows adds phpredis right after a new PHP is
   installed, when Kiln has a build pinned for it (8.2 to 8.5, non-thread-safe x64): the
   same checked download and journaled `php.ini` line as `kiln php ext add redis`, so
   History can undo it and `kiln php ext off redis` turns it off. `--without-redis` skips
   it.
2. **A failure is a note**, not a failed install: the PHP is installed either way, and
   the note says `kiln php ext add redis` tries again. A PHP with no pinned build gets
   nothing and no message.
3. **`kiln php update`** doesn't add it on its own: the new patch carries over the old
   one's PECL files and switches (ADR 0067 §3), so a version where the user turned
   phpredis off stays without it.
4. **The app's PHP install dialog** says phpredis comes with it and where its switch is.

## Consequences

- A new PHP downloads one more small file (about 1 MB) from downloads.php.net.
- Uninstalling still removes it with the PHP folder.
