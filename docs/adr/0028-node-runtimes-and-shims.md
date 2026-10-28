# 0028. Node runtimes: nodejs.org builds, signature-checked, and shims that step aside

- **Status:** accepted
- **Date:** 2026-10-28
- **Task:** P2-05 · Builds on [ADR 0013](0013-downloads-and-php-runtimes.md), [0016](0016-per-site-php-and-shims.md), [0023](0023-linux-shims-and-path.md), [0027](0027-project-config-kiln-toml.md)

## Context

Laravel projects build their front end with Node (Vite). Phase 2 asks for Node versions per
project, like PHP: `kiln node install`, and `node` / `npm` / `npx` / `corepack` commands
that follow the project's `kiln.toml`, `.nvmrc` or `.node-version`.

Three facts shape the design:

- **nodejs.org publishes everything Kiln needs:**
  - `dist/index.json` lists every release, with its LTS codename and the platforms built.
  - Each release folder has `SHASUMS256.txt`, and `SHASUMS256.txt.sig`: a detached OpenPGP
    signature by the releaser who cut it.
  - The release keys, current and former, are published in the
    [`nodejs/release-keys`](https://github.com/nodejs/release-keys) repository. Today they are
    29 keys, RSA (2048–4096 bits) and Ed25519, some signing through a subkey.
- **Many developers already have Node:** from nvm, Volta, fnm, the official installer or
  their distribution. Kiln must not break it (plan principle: never override an existing
  developer tool without approval).
- **npm, npx and corepack are scripts, not programs.** On Windows they are `.cmd` files.
  Starting a `.cmd` file means going through `cmd.exe`, whose argument quoting is unsafe for
  arbitrary arguments.

## Decision

1. **Builds come from nodejs.org**, for the platforms Kiln supports:
   - **Windows:** `node-v<version>-win-x64.zip`.
   - **Linux:** `node-v<version>-linux-x64.tar.gz` or `-linux-arm64.tar.gz`. The gzip
     archive, because Kiln already reads gzip and has no xz decoder.
   - Releases come from `index.json`. Nothing older than Node 18 is offered: its builds
     predate today's archive layout and are past end of life.
2. **The checksums are signature-checked, then the archive is checked against them:**
   - Kiln downloads `SHASUMS256.txt` and `SHASUMS256.txt.sig`, and verifies the signature
     against Node's release keys.
   - The keys are copied verbatim from `nodejs/release-keys/keys` into
     `crates/kiln-node/keys/`. All of them, current and former, because a release keeps the
     signature of whoever cut it.
   - Only then is the archive's SHA-256 taken from that file. The archive is downloaded and
     checked as for PHP ([ADR 0013](0013-downloads-and-php-runtimes.md)).
   - **A signature by a key Kiln doesn't know is refused,** with the key's fingerprint and
     "update Kiln". A new releaser means a Kiln update; this is the price of not trusting
     TLS alone.
   - **No new crates:** a small OpenPGP reader in `kiln-node` understands exactly what is
     needed:
     - v4 public keys and subkeys (RSA, and Ed25519 in its v4 "EdDSA" form);
     - v4 binary-document signatures with SHA-256, SHA-384 or SHA-512.

     The arithmetic is `ring`'s, already used by the TLS stack. Anything else is refused.
   - **Not checked:** key expiry, and revocation inside the files. The files are the trust
     anchor; a key Node revokes is removed from Kiln's copy.
3. **Install layout:** `<local data>/node/<version>/`, with the archive's top folder
   removed.
   - **Marker:** an install-in-progress marker until `node --version` reports the version,
     as for PHP.
   - **`kiln-node.json`** beside it records the version and its LTS codename, so the shims
     can match `lts/*` offline.
   - **Inside Kiln's folder, so not journaled,** like PHP builds. `kiln uninstall` removes
     them with the folder.
   - **Linux symbolic links:** the Linux archive has some (`bin/npm`, …). Relative links
     that stay inside the version's folder are recreated. Any other link is refused, with
     the archive.
4. **Version specs**, the same for `kiln node install`, `.nvmrc` and `.node-version`:
   - `22`, `22.11`, `22.11.0`, with or without a leading `v`: the newest match;
   - `lts` or `lts/*`: the newest LTS. `lts/jod`: the newest of that LTS line;
   - `node`, `latest`, `current` or `stable`: the newest release.

   **`kiln.toml`** takes numbers only ([ADR 0027](0027-project-config-kiln-toml.md)), since
   a committed file should name a line. Anything else in `.nvmrc`, such as `lts/-1`, is
   reported, and the default is used.
5. **Which Node runs in a folder:**
   - **Search:** from the folder up to the file-system root. In each folder Kiln checks
     `kiln.toml`'s `[node] version`, then `.nvmrc`, then `.node-version`. The first one
     found wins. A JavaScript project needn't be a Kiln site, so this doesn't look for one.
   - **Otherwise the default:** `kiln node default <spec>` (setting `node.default`), else
     the newest installed version.
   - **A version that isn't installed:** the shim says so on stderr and uses the default.
     As for PHP, nothing is downloaded behind the user's back; `kiln which` and
     `kiln doctor` show the command to run.
6. **Shims: `node`, `npm`, `npx` and `corepack`** are more copies of `kiln-shim`, written by
   `kiln shims install` beside `php` and `composer`.
   - **No `.cmd` files:** `npm`, `npx` and `corepack` run the chosen `node` with the
     package's own entry script, for example `node …/npm/bin/npm-cli.js`. This needs no
     `cmd.exe` on Windows.
   - **The child's `PATH`** starts with that version's folder, so scripts that call `node`
     get the same version.
   - **Kiln has no Node: the shims step aside.** They run the next command of the same name
     on `PATH` after Kiln's shims folder, with the same arguments. Installing the shims
     never breaks an existing Node. Only installing a Node with Kiln makes the shims choose.
   - **Windows, machine-wide Node:** Windows puts the machine `PATH` before the user's, so a
     machine-wide Node, such as from Node's installer, still comes first. Kiln doesn't edit
     the machine `PATH`: that needs an administrator and overrides another tool.
     `kiln shims install` and `kiln doctor` say when this is the case.
7. **Commands:**
   - `kiln node available`: the newest release of each line, with its LTS name.
   - `kiln node install <spec>`, `kiln node list`, `kiln node remove <version>`.
   - `kiln node default <spec>`.
   - `kiln which` also shows what `node` runs, and why.
8. **Global packages** (`npm install -g`) go into that version's folder, and are removed
   with it. Their commands are not on `PATH`; run them with `npx`. The same goes for
   `corepack enable`. A later task may add a shim folder for them.

## Consequences

- **Node per project, team-wide:** a team commits `.nvmrc` or `kiln.toml`, and `npm run
  build` in that folder uses that version, on Windows and Linux alike.
- **Stronger integrity than TLS alone:** a compromised nodejs.org or CDN can't ship a
  modified build. Kiln releases must follow Node's key changes, which happen a few times a
  year at most.
- **About 180 KB of key files in `kiln`.** The shim doesn't contain them.
- **Coexistence:** `kiln shims install` changes nothing for a user who never installs a
  Node with Kiln.

## Alternatives considered

- **Trusting `SHASUMS256.txt` over TLS,** as nvm and fnm do: simpler, but a single
  compromised server could then change both the archive and its checksum.
- **An OpenPGP crate** (`pgp`, `sequoia-openpgp`): complete, but each brings tens of crates,
  and `rsa` carries an open advisory (RUSTSEC-2023-0071, a timing attack on decryption,
  which Kiln never does) that would need an exception. Kiln needs one narrow verification.
- **Pinning SHA-256s in Kiln,** as for Linux PHP ([ADR 0025](0025-own-static-linux-php-builds.md)):
  Node releases every few weeks across four lines, too often to pin.
- **Running `npm.cmd`:** it goes through `cmd.exe`, whose quoting Rust refuses for some
  arguments (the "BatBadBut" class of problems), and it is slower.
- **Shadowing an existing Node as soon as the shims are on PATH:** surprising, and against
  the coexistence rule.
