# 0085. Opening a site, or a file at a line, in the user's editor (`kiln code`)

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-06 · Builds on [ADR 0055](0055-site-pages.md), [0059](0059-log-viewer.md)

## Context

From a site page the user can open the site in a browser and its folder in Explorer, but
not in their editor. A Laravel error in the log viewer names `C:\Sites\shop\app\Http\...\X.php:42`,
which the user copies into their editor by hand. Lerd opens projects and log lines in the
user's editor; the user asked for what Lerd has.

## Decision

1. **Editors Kiln knows** on Windows: VS Code, Cursor, PhpStorm, Zed and Sublime Text. Each
   is found in its usual install folders:
   - `%LOCALAPPDATA%\Programs\…` for user installs, JetBrains Toolbox's included;
   - `%ProgramFiles%\…`, with a JetBrains IDE's newest version folder first;
   - else its program on `PATH`.

   Only `.exe` files are run: never a `.cmd` or `.bat`, so no shell parses the path. Each
   editor opens a file at a line its own way:

   | Editor | Folder | File at a line |
   |---|---|---|
   | VS Code, Cursor | `<exe> <folder>` | `<exe> -g <file>:<line>` |
   | PhpStorm | `<exe> <folder>` | `<exe> --line <line> <file>` |
   | Zed, Sublime Text | `<exe> <folder>` | `<exe> <file>:<line>` |

2. **`kiln editor`** (`--json`) lists the editors found, the chosen one, and the one in
   use: the chosen one if found, else the first found in the order above.

   **`kiln editor use <id>`** chooses one, and **`kiln editor use auto`** goes back to
   detection. The choice is the setting `editor`. Only Kiln's state changes, so it isn't
   journaled.
3. **`kiln code [<site>] [--goto <file>[:<line>]]`** opens the site's folder in the editor,
   or the file at the line.
   - Without a site, it opens the site of the current folder.
   - The file may be relative to the site's folder or absolute. Either way it must be
     inside that folder once resolved, so a `..` or a link can't reach out of the project.
     A Laravel stack trace's `vendor/…` lines qualify.
   - The editor starts on its own, without a console. Kiln doesn't wait for it.
4. **API and app:**
   - `code` and `editor use` are on the allow-list; they make their change without asking.
   - The editor list is the read `editor.get`.
   - The site page gets an "Open in <editor>" button.
   - Settings gets the editor choice.
   - In the log viewer, a path ending in a line, written `file.php:42` or `file.php(42)`
     as PHP and Laravel print them, opens there for a site's own logs.
5. **Not yet:** macOS and Linux locations, other editors, a custom command line.

## Consequences

- One click from a log line to the code that failed.
- The API can start only a known editor, on a folder or file inside one of Kiln's sites.
