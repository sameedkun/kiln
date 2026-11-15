# 0087. Notifications: mail caught, a process that failed, a long job done

- **Status:** accepted
- **Date:** 2026-11-15
- **Task:** P4E-08 · Builds on [ADR 0048](0048-services-supervisor.md), [0053](0053-api-v2-and-json-output.md),
  [0060](0060-dev-processes.md), [0084](0084-site-commands.md)

## Context

Three things happen while the user is in their editor or browser, and they find out late
or never:
- a test email lands in Mailpit;
- a queue worker or Vite crashes, and the supervisor restarts it;
- a long `kiln run` (a test suite) finishes.

Lerd notifies on these; the user asked for what Lerd has.

The app has no notification plugin available offline. It does already link the `windows`
crate (Tauri brings it), which shows Windows' own toasts.

## Decision

1. **kilnd collects notifications**, each with an id, a time, a kind, a title and a line:

   | Kind | When | Says |
   |---|---|---|
   | `mail` | a running Mailpit's message count goes up (polled every few seconds on its own loopback port); the first look only sets the count | the newest subject, who from and to, and how many came |
   | `process` | the services supervisor logs that a program failed (non-zero exit) or couldn't start; at most one per program every ten minutes | the program, and that Kiln restarts it |
   | `job` | an API job that ran 20 seconds or more ends, such as a test run from the Commands tab | the command, and whether it worked |

   It keeps the newest 50 in `notifications.json` in its state folder, so they survive a
   restart. It publishes each as the event `notification`.
   - The read `notifications.list` gives them, newest first.
   - The method `notifications.clear` empties the list.
2. **The app shows each as a Windows notification**, from its Rust side, which receives
   kilnd's events once, whatever windows are open.
   - It is a toast under the app's identifier (`com.github.sameedkun.kiln`). Windows
     shows it for an installed Kiln, whose Start menu shortcut carries that id.
   - Where Windows refuses, such as a build run from its folder, nothing breaks: the list
     in the app still has it.
   - Text is escaped for the toast's XML.
   - The app setting "Windows notifications" (on by default) turns the toasts off; the
     list still fills.
3. **In the app:** a Notifications page lists the newest 50, with a count of the unread
   ones since the page was last opened, and Clear.
4. **Nothing leaves this PC**, and no one but the user sees them. A mail notification shows
   a test email's subject and addresses, which the user's own app sent.

## Consequences

- The user hears about caught mail, a crashing worker and a finished test run without
  watching Kiln.
- kilnd polls Mailpit every few seconds, a loopback request per running instance.
- Toasts depend on Kiln being installed. The list doesn't.
