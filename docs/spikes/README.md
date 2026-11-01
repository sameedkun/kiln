# Spikes

Time-boxed experiments that produce evidence for an ADR. Spikes 01–02 ran in Phase 0, 03 in
Phase 3. Spike code lives in `spikes/` (outside the Cargo workspace) so results can be
reproduced; downloaded binaries never enter git.

| # | Spike | Feeds | Time box | Status |
|---|---|---|---|---|
| 01 | [Web server: nginx vs Caddy on Windows](01-web-server.md) | ADR 0005 | 3 days | **done**: Caddy ([results](results/01-web-server.md)) |
| 02 | [Windows DNS: hosts block vs NRPT + stub](02-windows-dns.md) | ADR 0006 | 3 days | **done**: hosts block only; NRPT rejected ([part 1](results/02-windows-dns-part1.md), [part 2](results/02-windows-dns-part2.md)) |
| 03 | [A Redis-compatible cache on Windows (Garnet)](03-windows-cache.md) | ADR 0035 §8 | 1 day | **done**: Garnet, once it fixes Lua ignoring `SELECT`; no Windows cache until then ([results](results/03-windows-cache.md)) |

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
