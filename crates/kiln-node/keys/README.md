# Node.js release keys

These files are copied verbatim from
<https://github.com/nodejs/release-keys/tree/main/keys>: one armored public key per person
who has signed Node.js releases, current and former. Kiln checks `SHASUMS256.txt.sig` against
them before it trusts any checksum ([ADR 0028](../../../docs/adr/0028-node-runtimes-and-shims.md) §2).

To update:

1. Copy the new or changed `<FINGERPRINT>.asc` files here, and remove any key that Node.js
   has removed or revoked.
2. Update the list in `src/keys.rs`.
3. `cargo test -p kiln-node` checks that every file parses to the fingerprint in its name.
