# 0010. Local certificate authority: name-constrained root, DPAPI-protected key, per-user trust

- **Status:** accepted; the choice of TLD (and which names are allowed) is set by [ADR 0033](0033-configurable-tld.md)
- **Date:** 2026-10-21
- **Task:** P1-05 · Builds on [ADR 0004](0004-reversible-operation-engine.md), [0005](0005-web-server.md), [0008](0008-unsafe-confined-to-kiln-sys.md)

## Context

Every `https://<site>.test` page needs a certificate that browsers trust. The usual approach
for local dev tools is a self-signed root CA added to the machine's trust store. Done
carelessly, that is a standing risk:

- the root's private key usually sits **unencrypted** in the user profile;
- the root can sign certificates for **any** domain, so anyone who steals the key can
  intercept the user's banking traffic;
- it is often trusted **machine-wide** for decades and never removed on uninstall.

Spike 01 showed Kiln can avoid all three: a root with a *Name Constraints* extension limited to
`.test` is honoured by OpenSSL and by Windows Schannel, and an `example.com` leaf signed by it is
rejected. Spike 01 also showed Caddy can issue site certificates itself from a CA we supply
([ADR 0005](0005-web-server.md)).

## Decision

### Hierarchy

1. **Root CA**, created once per user by Kiln:
   - ECDSA P-256. Subject `CN=Kiln Local Development CA (<user>@<host>, <date>)`,
     `O=Kiln`, so every root is unique and recognisable.
   - Basic constraints `CA:TRUE, pathlen:1`; key usage `keyCertSign, cRLSign`.
   - **Name Constraints (critical):** permitted DNS subtree = the configured development TLD
     (default `test`), nothing else. No IP or e-mail names.
   - Valid **3 years**. `kiln ca rotate` (later) issues a new one and swaps trust.
2. **Intermediate CA**, issued by Kiln from the root for Caddy's PKI app:
   - Same name constraints (they also inherit from the root), `pathlen:0`.
   - Valid **30 days**, re-issued by Kiln before expiry.
   - Its key is the only CA key Caddy ever sees. It is stored with a user-only ACL.
3. **Leaf certificates** are issued by Caddy from the intermediate (P1-06). Kiln doesn't
   write leaves itself.

A stolen intermediate key mints `.test` certificates for at most 30 days. A stolen root
certificate is harmless without its key.

### Key protection

4. The **root private key is never stored in plaintext**. On Windows it is encrypted with
   **DPAPI** (`CryptProtectData`, current-user scope, with a Kiln-specific entropy value), so
   only the same Windows user on the same machine can decrypt it. It is decrypted only in
   memory, to sign an intermediate, and never handed to Caddy.
5. On Linux (P1-04 follow-up) the equivalent is the Secret Service (libsecret). Until that
   exists, the non-Windows backend stores the key with mode `0600` and the CLI warns that it is
   not encrypted at rest.
6. Layout under the state directory: `ca/root.crt` (PEM), `ca/root.key.protected` (DPAPI
   blob), `ca/intermediate.crt` and `ca/intermediate.key` (PEM, user-only).

### Trust: per-user, reversible, consented

7. Trusting the root is a `kiln_ops::Operation` (`trust.ca.user`), so it is previewed,
   journaled and undoable like everything else. It is added to **`CurrentUser\Root`**:
   - no administrator rights are needed;
   - Windows shows its own security dialog naming the certificate, which is the user's
     consent;
   - other users on the machine are unaffected.

   Machine-wide trust (`LocalMachine\Root`) is a later, explicit opt-in.
8. The operation identifies the certificate by its SHA-1 thumbprint, as Windows does.
   - `prepare` records whether it was already trusted.
   - `apply` adds it only if missing.
   - `revert` removes it only if Kiln added it.

   The `kiln-sys` wrapper refuses to add any certificate whose subject is not a Kiln root, so
   the code path can't be abused to trust arbitrary certificates.
9. Tests use a private, per-test certificate store, which shows no dialog. Only the real
   `Root` store is a manual check, like I-42.

### Firefox and other NSS users (plan)

10. **Windows:** Firefox can trust Windows' certificate stores ("enterprise roots"). Kiln
    `doctor` will detect Firefox profiles and, on request, add
    `security.enterprise_roots.enabled = true` to `user.js` through a journaled operation.
    No NSS database writes on Windows.
11. **Linux:** import the root into `~/.pki/nssdb` (Chromium) and each Firefox profile's
    `cert9.db` with `certutil` from `libnss3-tools`. Each import is a journaled operation,
    removed again on undo. Detection and a missing-`certutil` hint live in `kiln doctor`.
    Implemented with P1-04.
12. **macOS (Phase 3):** login keychain via `security add-trusted-cert`, plus the same NSS
    approach for Firefox.

## Consequences

- Kiln's CA can only ever vouch for development names, even if fully compromised. That is a
  material improvement over unconstrained local CAs.
- The root key never reaches the web server, and needs the user's Windows login to decrypt.
- Schannel clients hard-fail revocation checks against any local CA without CRL/OCSP
  ([I-12](../issues.md#i-12)). That is unchanged and documented in `kiln doctor`.
- Trust needs one click on a Windows dialog per root, and again after rotation.

## Alternatives considered

- **Unconstrained root, machine-wide trust:** the common default elsewhere. Rejected for the
  risks above.
- **Give Caddy the root key:** simpler, but puts the long-lived key on disk in plaintext.
  Rejected.
- **`certutil` / PowerShell for trust:** spawns processes and parses localized output;
  `crypt32` via `kiln-sys` is direct and testable.
