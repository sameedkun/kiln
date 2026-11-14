# 0073. Stripe webhooks for a site

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-09 · Builds on [ADR 0060](0060-dev-processes.md)

## Context

Laravel apps with Cashier take Stripe's events at `/stripe/webhook`. Locally that needs
`stripe listen --forward-to …` running beside the site, which the user asked Kiln to keep
running like its other dev processes (2026-11-14). The Stripe CLI is Apache-2.0 and
publishes a SHA-256 for each build (`stripe-windows-checksums.txt`, and GitHub's digests).

## Decision

1. **A project process, `stripe`**, added like the queue worker (`kiln service add stripe
   --project <folder>`) and shown under a site's Dev processes. It's the first project
   process with a program of its own: `Runtime::Own` (the pinned Stripe CLI 1.55.11 for
   Windows x64, downloaded and checked when it's added, run as `kiln-stripe.exe`). Project
   processes on PHP or Node still download nothing.
2. **No `stripe login`.** The key is the project's own: Cashier's `STRIPE_SECRET` in its
   `.env`, read at each start and given to the process as `STRIPE_API_KEY` in its
   environment, so it's never on a command line, in Kiln's state or in History. A project
   without `STRIPE_SECRET` or `APP_URL` is refused, saying which is missing.
3. **Forwarded to the site:** `<APP_URL>/stripe/webhook`; `--skip-verify` for an HTTPS
   `APP_URL`, whose certificate is Kiln's own local one. Stripe's telemetry is off
   (`STRIPE_CLI_TELEMETRY_OPTOUT`).
4. **The signing secret** (`whsec_…`) is what `stripe listen` prints when it starts, so it's
   in that process's log on this PC, as Stripe writes it; the site's `.env` needs it as
   `STRIPE_WEBHOOK_SECRET`. Kiln doesn't copy it anywhere.

## Consequences

- Use test keys: forwarding live events to a laptop isn't what this is for, and the app
  says "test events".
- The Scenario can't reach Stripe with a real key; it checks the pin against Stripe's own
  checksums file, the refusal, and that a key in `.env` reaches the process but not its
  command line or log.
