# 0058. History, Doctor and Kiln's settings in the app

- **Status:** accepted
- **Date:** 2026-11-13
- **Task:** P4-07 · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0019](0019-coexistence.md), [0053](0053-api-v2-and-json-output.md), [0054](0054-desktop-app-shell.md)

## Context

Three pages of the app had nothing behind them yet: History (a placeholder), Doctor (none),
and Kiln's own settings (the Settings page only had the app's). Everything they need exists
in the CLI except one read: there was no single answer to "what are Kiln's settings now".
`kiln web start` prints the ports it uses, `kiln php workers` sets the worker count, and the
TLD is read inside `kiln-runtime`, but nothing lists them.

## Decision

1. **History, Doctor and Settings are pages of the main window.** History and Doctor sit in
   the sidebar's second group, after Logs; Settings stays in the footer.
2. **`kiln settings`** (a read; `settings.get` through the API) answers with:
   - `tld`: the development TLD;
   - `http` and `https`: each `{port, auto}`, the port the web server uses (or will use) and
     whether Kiln picked it (`auto`) or the user chose it;
   - `php_workers`: the workers each PHP version runs.
   It changes nothing.
3. **History** lists `history.list`, newest first, grouped by day (Today, Yesterday, then
   dates). A switch, Show undone, asks for `all` and shows undone and rolled-back operations,
   greyed with their state. Undo is offered on operations still in effect (and on stuck or
   half-done ones) and always goes through `undo <id>`'s preview, so the user sees what is put
   back first. The undo engine decides whether newer operations must go first; the page
   doesn't guess.
4. **Doctor** shows `doctor.report`, grouped by area in the doctor's order. By default only
   what isn't fine (notes, checks, problems) is shown, with a count of passed checks; a switch
   shows everything. A status is a dot and a word (OK, Note, Check, Problem). Each finding's
   fix is the command to run, shown as a command: the page doesn't run fixes itself, because
   many are judgement calls (stopping another program, replacing a login item) and the ones
   that aren't already have a button on their own page. Check again reads the report again.
5. **Kiln's settings** join the Settings page in a section of their own:
   - **Web server ports**: Change… opens a dialog with Let Kiln pick (`web start
     --auto-ports`) or two ports (`--http-port`, `--https-port`), checked as numbers from 1 to
     65535 and different from each other. When the web server is running it is a job of
     `web stop` then `web start` with those options, each a job with its own output; the
     dialog says sites don't answer for a second or two. A port Kiln picked moves back to 80
     and 443 when they're free again (I-93); a chosen one never moves.
   - **PHP workers**: a choice of counts up to Kiln's limit (32), applied with `php workers N`.
   - **Development TLD**: shown, not changed here. Changing it renames every site, re-issues
     certificates and rewrites the hosts file; that stays a CLI step until it has a preview of
     its own.

## Consequences

- No page is a placeholder except Logs (P4-08).
- The ports question from the user's run (Kiln on 8080 with Herd quit) has an answer in the
  app as well as the fix: the Settings page says which ports are in use and who picked them.
- Doctor's fixes are copied into a terminal for now. Running a fix from the page can come
  later, per finding, once each fix has a preview.
