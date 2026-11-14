# 0077. OpenSearch and Elasticsearch

- **Status:** accepted
- **Date:** 2026-11-14
- **Task:** P4C-13 · Builds on [ADR 0036](0036-service-framework.md), [0042](0042-meilisearch-rustfs-mongodb.md)

## Context

DBngin now offers Elasticsearch on Windows, and the user asked for it in Kiln; asked to
choose, they wanted both Elasticsearch and OpenSearch (2026-11-14). Kiln already has
Meilisearch. Both ship as Windows zips with their own Java:

- **OpenSearch 3.9.0** (Apache-2.0): the full bundle (1.0 GB zip; no smaller Windows build),
  its plugins included. `opensearch.bat` runs one JVM: its options from `jvm.options`, then
  `org.opensearch.bootstrap.OpenSearch`.
- **Elasticsearch 9.5.6** (AGPL-3.0, SSPL or Elastic License 2.0, the user's choice; Kiln
  offers it under the AGPL option): 674 MB zip. `elasticsearch.bat` runs a small launcher JVM
  (`ServerLauncher`) with the environment `elasticsearch-env.bat` sets, which starts the
  server's JVM.

Both publish SHA-512 only; Kiln pins SHA-256, computed on a fresh runner by the pin workflow,
and CI checks the vendor's SHA-512 against the same file.

## Decision

1. **Two service kinds, `opensearch` and `elasticsearch`**, from their pinned zips. Each runs
   its bundled Java under a Kiln name (`kiln-opensearch.exe`, `kiln-elasticsearch.exe`,
   beside `java.exe` so the JVM is found); Elasticsearch's server JVM, started by its launcher,
   runs from the same `bin` folder, which is how Kiln knows it's its own.
2. **Settings as `-E` options**, so they show on the command line and in Kiln's checks:
   `network.host=127.0.0.1`, the instance's `http.port` (usual 9200) and `transport.port`
   (usual 9300), `path.data` and `path.logs` in the instance's folder, `discovery.type=single-node`.
3. **Security off** (OpenSearch's security plugin disabled; Elasticsearch's security neither
   enabled nor auto-configured): they listen on loopback only, like Kiln's databases, and TLS
   with generated passwords would only stand in the way of local development.
   Elasticsearch's machine-learning process and GeoIP downloads are off too.
4. **A fixed heap of 512 MB**: Elasticsearch otherwise takes half the PC's memory.
5. **A config folder per instance**, copied from the release's when the instance is made, so
   instances of one version don't share a keystore or edits.
6. **Their own CI workflow** ("Search engines"): the vendor's SHA-512, start, health on
   loopback only, a document indexed and found, removal with no JVM left. Run when their code
   changes, nightly and on demand, not in every services Scenario.

## Consequences

- Each instance needs about 1 GB of memory once running, and its download is large: the Add
  dialog shows the size as it downloads.
- Laravel Scout has no first-party driver for either; packages that do read their own `.env`
  keys, so Kiln wires none.
