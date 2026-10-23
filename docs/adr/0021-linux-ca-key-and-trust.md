# 0021. Linux CA: root key in the Secret Service, trust in NSS databases and the system store

- **Status:** accepted
- **Date:** 2026-10-23
- **Task:** P1-04 (part b) · Builds on [ADR 0010](0010-local-certificate-authority.md), [0020](0020-linux-dns-and-privilege.md)

## Context

[ADR 0010](0010-local-certificate-authority.md) gives Kiln a root CA that can only vouch for
`.test` names, with its key encrypted by DPAPI and trust added per user on Windows. On Linux,
the key is protected by file permissions only, and `kiln ca trust` doesn't work.

Measured in the Ubuntu 24.04 Desktop test VM on 2026-10-23 (Kiln's root, an HTTPS site at
`https://hello.test:8443/`):

| Trust added to | curl / OpenSSL tools | Google Chrome 156 |
|---|---|---|
| nothing | untrusted | `ERR_CERT_AUTHORITY_INVALID` |
| system store (`update-ca-certificates`) | **trusted** | still `ERR_CERT_AUTHORITY_INVALID` |
| user NSS database `~/.pki/nssdb` (`certutil -A -t C,,`) | (unchanged) | **trusted**; untrusted again after `certutil -D` |

- Firefox (snap, 158) keeps its own NSS database per profile
  (`~/snap/firefox/common/.mozilla/firefox/<profile>/cert9.db`). Its `pkcs11.txt` loads only
  NSS's internal module, so it doesn't see the system store either. Other installs keep
  profiles in `~/.mozilla/firefox` (deb, tarball) or `~/.var/app/org.mozilla.firefox`
  (Flatpak). Chromium as a snap uses `~/snap/chromium/current/.pki/nssdb`.
- NSS databases are changed with `certutil` from `libnss3-tools`, which isn't installed by
  default (Ubuntu, Fedora Workstation and Debian alike).
- The desktop's Secret Service (`org.freedesktop.secrets` on the user's session bus: GNOME
  Keyring, KWallet 6, KeePassXC) keeps secrets encrypted with the user's login password,
  which is the Linux counterpart of DPAPI.

## Decision

### Root key: in the Secret Service

1. On Linux, the root's private key is stored as a **Secret Service item** in the user's
   default collection, labelled `Kiln local CA root key`, with attributes
   `application = kiln`, `kiln-item = ca-root-key`, `kiln-id = <random 128-bit id>`.
   - `ca/root.key.protected` then holds only a reference
     (`kiln-secret-service/1 <id>`), never key material.
   - The key is read into memory only to sign an intermediate (as on Windows).
   - If the collection is locked, the desktop shows its own unlock dialog; dismissing it is
     an error, and nothing falls back silently.
2. **No Secret Service** (servers, sessions without a desktop): the key is stored with mode
   `0600` as before, and `kiln ca init` says so. The blob format tells the two apart, so a
   key created either way keeps working.
3. `kiln uninstall` deletes the Secret Service item together with Kiln's folders.
4. The Secret Service is spoken over D-Bus with `zbus` 5 (MIT; its blocking API; no C
   libraries), a Linux-only dependency of `kiln-pki`. Items use the `plain` session: the
   session bus is private to the user, and that is what libsecret does with GNOME Keyring
   too.

### Trust: NSS databases per user, the system store on request

5. `kiln ca trust` on Linux trusts the root **for the current user** in every NSS database
   it finds, without root rights:
   - `~/.pki/nssdb` (Chrome, Chromium, Edge, Brave), and Chromium's snap equivalent;
   - each Firefox profile with a `cert9.db`, in the deb, snap and Flatpak locations.

   Each database is one journaled operation, `trust.ca.nss`: `certutil -A -t "C,,"` (trusted
   to issue server certificates only), under a nickname naming the root's SHA-256.
   - `prepare` records whether the root was already there; `revert` removes it only if Kiln
     added it.
   - If `~/.pki/nssdb` doesn't exist but a Chromium-family browser is installed, Kiln
     creates it as Chrome would (`certutil -N --empty-password`), records that, and on undo
     removes the database again if the root was the only thing in it.
   - Browsers that are running pick the change up after a restart; Kiln says so.
6. Without `certutil`, `kiln ca trust` changes nothing and says what to install
   (`libnss3-tools` on Debian/Ubuntu, `nss-tools` on Fedora/openSUSE, `nss` on Arch). Kiln
   doesn't install packages itself.
7. `kiln ca trust --system` also adds the root to the **system store**, for command-line
   tools: curl, PHP (Composer, Guzzle), Node, git. That needs root, so it goes through the
   helper ([ADR 0020](0020-linux-dns-and-privilege.md)) as new fixed operations (protocol v3):
   - `TrustSystemCa { pem }`: the helper parses the certificate and accepts it only if it is
     a CA whose **critical name constraints permit only DNS names under one TLD** that Kiln
     accepts (public TLDs are refused), and whose subject is a Kiln root. It writes
     `kiln-<first 16 hex of SHA-256>.crt` into the distribution's anchor folder and runs its
     update tool, both chosen by the helper from fixed paths:
     - Debian/Ubuntu: `/usr/local/share/ca-certificates`, `update-ca-certificates`;
     - Fedora/RHEL: `/etc/pki/ca-trust/source/anchors`, `update-ca-trust`;
     - Arch: `/etc/ca-certificates/trust-source/anchors`, `trust extract-compat`;
     - openSUSE: `/etc/pki/trust/anchors`, `update-ca-certificates`.
   - `UntrustSystemCa { sha256 }`: removes only that file name, and reruns the update tool.
   - The journal kind is `trust.ca.system`, undone like any other operation.

   System trust affects every user of the machine, which is why it is opt-in, as
   [ADR 0010](0010-local-certificate-authority.md) §7 planned. The name constraints keep it
   limited to development names.
8. `kiln doctor` on Linux reports, when a site uses HTTPS:
   - where the root key is kept (Secret Service or file);
   - each NSS database found, and whether it trusts the root;
   - whether `certutil` is installed;
   - whether the system store trusts the root (info: it only matters for command-line tools).

## Consequences

- Chrome and Firefox trust Kiln's sites after one `kiln ca trust`, with no password prompt.
  `--system` adds one polkit prompt for command-line tools.
- The root key is encrypted with the user's login password on desktops, like DPAPI.
- Kiln depends on `certutil` for browser trust; without it, `kiln ca trust` explains
  instead of guessing. Writing NSS's SQLite databases directly was rejected below.
- An NSS database Kiln didn't find (a browser installed later, a new Firefox profile) needs
  `kiln ca trust` again; `kiln doctor` lists the ones that don't trust the root yet.

## Alternatives considered

- **System store only:** Chrome and Firefox don't read it (measured above).
- **Load `p11-kit-trust` into each Firefox profile's `pkcs11.txt`:** makes Firefox read the
  system store, but needs root for the system store anyway, and a snap may not be allowed to
  load the host's library.
- **Firefox enterprise policies (`/etc/firefox/policies`, `Certificates.Install`):**
  machine-wide, needs root, and edits a file administrators and other tools own.
- **Writing `cert9.db` with SQLite directly:** possible (Kiln has SQLite), but NSS's object
  format is internal, and a running browser holds the database.
- **The `secret-service` crate:** about 240 dependency lines (async runtime, big-integer and
  AES code for the DH session), against about 60 for `zbus` alone.
- **A hand-written D-Bus client:** fewer dependencies, but its marshalling would be new,
  hard-to-test code guarding a secret.
- **An AEAD-encrypted key file with only the wrapping key in the Secret Service:** the same
  protection with more moving parts; the Secret Service already encrypts at rest.
