# 0013. Verified downloads; PHP runtimes from windows.php.net

- **Status:** accepted
- **Date:** 2026-10-21
- **Task:** P1-08 · Builds on [ADR 0003](0003-license-and-dependency-policy.md), [0011](0011-caddy-integration.md)

## Context

Kiln needs to fetch two kinds of binaries:

- **Caddy:** one pinned release per Kiln version. P1-06 only accepted a file the user had
  downloaded ([ADR 0011](0011-caddy-integration.md) §10).
- **PHP:** on Windows, the official builds from windows.php.net. A user wants several minor
  versions (8.1 … 8.5), and patches arrive monthly, so pinning every PHP hash in Kiln's source
  isn't practical.

What the PHP project publishes (checked 2026-10-21):

- `https://windows.php.net/downloads/releases/releases.json` redirects to
  `https://downloads.php.net/~windows/releases/releases.json`.
  - It lists each minor version (7.4 … 8.5) with its **latest** patch and, per build, the zip
    file name and its **SHA-256**.
  - The x64 non-thread-safe build is `nts-vs16-x64` (8.0–8.3) or `nts-vs17-x64` (8.4+). Caddy
    talks FastCGI to `php-cgi.exe`, so Kiln wants NTS.
- `sha256sum.txt` beside it has the same hashes.
- Older patches live in `/releases/archives/`, which has **no checksum file**.
- The zips aren't signed. The integrity anchor is the TLS connection to php.net.

## Decision

### Downloads (new crate `kiln-fetch`)

1. **Never download without knowing the expected digest first.** Every download carries an
   expected SHA-256 or SHA-512. Kiln hashes the file while streaming it to `<name>.partial`,
   and only moves it into place with `rename_replace` if the digest matches. A mismatch
   deletes the partial file and reports both values.
2. **HTTPS only.** Plain `http://` URLs are refused, and a redirect may only lead to another
   HTTPS URL. GitHub, for example, redirects release downloads to its CDN.
3. **Server certificates are checked against the operating system's trust store**
   (`rustls-platform-verifier`), so corporate TLS-inspecting proxies work where the browser
   works. HTTP(S) proxies come from the usual environment variables. Bundled Mozilla roots
   (`webpki-roots`) aren't used; their CDLA licence isn't on Kiln's allow-list.
   - `rustls-platform-verifier` pulls in `webpki-root-certs` (CDLA) only on `wasm32`.
   - `deny.toml` now lists the platforms Kiln ships for (Windows, Linux and macOS, x64 and
     arm64). Licences are judged for what is actually built, not for unrelated targets.
4. **Client:** `ureq` 3 with rustls and the `ring` backend, the same TLS stack Kiln already
   uses in tests. That means no OpenSSL and no C toolchain.
5. **Cache:** verified files are kept in `<local data>/downloads/`. A cached file is reused
   only after it is hashed again and matches, so a reinstall works offline. Each download is
   capped at 512 MB.
6. **Timeouts:** 20 s to connect and 60 s for the response headers.
   - The body has up to **60 minutes** in total. ureq's body timeout covers the whole transfer,
     not idle time. A 60 s value failed a real 34 MB PHP download at about 110 KB/s.
   - There's no separate stall detection yet, so a dead connection can wait out that hour.
     Ctrl+C is safe: the partial file never reaches the cache.

### Trust per source

7. **Caddy:** the SHA-512 is pinned in Kiln's source ([ADR 0011](0011-caddy-integration.md)
   §10). `kiln web install` now downloads the pinned archive itself; `--archive` stays for
   offline installs.
8. **PHP:** the SHA-256 comes from `releases.json`, fetched over HTTPS from the fixed host
   `downloads.php.net/~windows/releases/`.
   - File names from the index must be plain names (no `/`, `\` or `..`), so the index can
     only point at files in that same folder.
   - The verified hash is recorded with the installed version.
   - What this protects against: corrupted or truncated downloads, tampering on the way or in
     the cache, and a modified install being mistaken for the original. What it doesn't: a
     compromised php.net, which would serve a bad file and a matching hash. That is the same
     trust as downloading PHP by hand from windows.php.net.
9. **Only verifiable versions are installed.** That's the latest patch of each minor version
   listed in `releases.json`. Asking for an older patch (only in `/archives/`, which has no
   checksums) is refused, with the reason and the installable version.

### PHP runtimes (new crate `kiln-php`)

10. **Install** (`kiln php install 8.4` or `8.4.27`):
   - resolve the version in the index and pick the `nts-vs*-x64` build;
   - download and verify it;
   - extract it straight into `<local data>/php/<version>` (e.g.
     `%LOCALAPPDATA%\Kiln\php\8.4.27`), which holds a `.kiln-install-incomplete` marker until
     the check in §12 passes.
   - A folder with the marker is never treated as installed; the next install starts it over,
     and a failure removes it.
   - An earlier design extracted to `<version>.partial` and renamed the folder at the end. The
     first real install failed at that rename with "Access is denied". Renaming a folder of
     freshly written executables fails while antivirus is still scanning them, and inside a
     packaged app's container ([I-44](../issues.md#i-44)). See [I-49](../issues.md#i-49).
   - Extraction refuses entries that would escape the folder (absolute paths, `..`) and stops
     at a total size cap.
   - Re-installing an installed version is a no-op.
11. **php.ini:** Kiln writes `php.ini` from the bundled `php.ini-development`.
    - `extension_dir = "ext"` is set, and a fixed list of extensions Laravel and WordPress
      commonly need is enabled, each only if its DLL ships in that build: `curl`, `fileinfo`,
      `gd`, `intl`, `mbstring`, `exif`, `openssl`, `pdo_mysql`, `pdo_pgsql`, `pdo_sqlite`,
      `sqlite3`, `sodium`, `zip`.
    - The original `php.ini-development` is left untouched for reference.
12. **Check after install:** Kiln runs `php.exe -v` and requires `PHP <version>` in the output.
    - If Windows can't start it (STATUS_DLL_NOT_FOUND, usually a missing Visual C++
      2015–2022 runtime), Kiln says so and links Microsoft's official redistributable.
    - Kiln **doesn't install** the redistributable: it's a system-wide change outside Kiln's
      own folders. The half-installed version is removed.
13. **State:** migration v3 adds `php_versions` (version, folder, archive name, SHA-256,
    installed at). `kiln php list` shows installed versions; `kiln php available` shows the
    index; `kiln php remove <version>` deletes the folder and the row.
14. PHP installs aren't journaled operations. Like Caddy, they only touch Kiln's own
    folders. Running PHP for sites (php-cgi pools behind Caddy) is P1-09. Per-site versions and
    `php` / `composer` shims are P1-11. Linux runtimes (static-php-cli / distro packages) come
    with P1-04.

## Consequences

- `kiln web install` and `kiln php install` work with no manual download.
- Every byte Kiln runs from the internet was checked against a digest that Kiln knew before
  the download started.
- Older PHP patches can't be installed. That's deliberate, since they couldn't be verified.
- A new dependency tree (ureq, rustls-platform-verifier). License and advisory checks stay
  enforced by `cargo-deny`.

## Alternatives considered

- **Pin PHP hashes in Kiln's source:** strongest anchor, but every PHP patch would need a Kiln
  release. That may come back later as an optional signed manifest.
- **reqwest:** heavier (tokio), with no benefit for a few sequential downloads.
- **`webpki-roots` bundle:** CDLA-Permissive-2.0 isn't on the allow-list, and it ignores
  enterprise roots.
- **Thread-safe (TS) PHP builds:** meant for Apache's mod_php. Kiln runs `php-cgi` processes,
  which want NTS.
