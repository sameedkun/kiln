# Phase 0 spikes

Time-boxed experiments that produce evidence for an ADR. Spike code lives in `spikes/` (outside
the Cargo workspace) and is thrown away afterwards. Only results and the ADR survive.

| # | Spike | Feeds | Time box | Status |
|---|---|---|---|---|
| 01 | [Web server: nginx vs Caddy on Windows](01-web-server.md) | ADR 0005 | 3 days | planned |
| 02 | [Windows DNS: hosts block vs NRPT + stub](02-windows-dns.md) | ADR 0006 (opt-in mode) | 3 days | planned |

## Ground rules for every spike

- **Nothing system-wide without explicit approval.** Binaries are portable downloads in
  `spikes/<name>/.cache/` (git-ignored), checksum-verified, and deleted after the spike.
- **Don't disturb existing tools.** The development machine runs Laravel Herd, which owns
  `127.0.0.1:80/443`, a hosts block, and the PHP/nginx processes. Spikes use ports
  **8080/8443** (both verified free on 2026-10-19), never stop foreign processes, and never edit
  the host machine's hosts file, NRPT table or trust stores.
- Anything that has to change system state runs in a **disposable VM** (see spike 02).
- Results go in `docs/spikes/results/NN-*.md`, with raw numbers, versions and the exact
  commands used.
