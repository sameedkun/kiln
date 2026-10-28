//! Just enough OpenPGP ([RFC 4880], [RFC 9580]) to check Node's release signatures ([ADR 0028]
//! §2): v4 RSA and Ed25519 ("EdDSA") public keys and subkeys, and v4 signatures over a binary
//! document with SHA-256/384/512. Anything else is refused. The arithmetic is `ring`'s.
//!
//! [RFC 4880]: https://www.rfc-editor.org/rfc/rfc4880
//! [RFC 9580]: https://www.rfc-editor.org/rfc/rfc9580
//! [ADR 0028]: ../../../docs/adr/0028-node-runtimes-and-shims.md

use std::fmt::Write as _;

use base64::Engine as _;
use ring::signature;
use sha1::Sha1;
use sha2::{Digest as _, Sha256, Sha384, Sha512};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PgpError {
    #[error("malformed OpenPGP data: {0}")]
    Malformed(&'static str),
    #[error("unsupported OpenPGP signature: {0}")]
    Unsupported(String),
    #[error(
        "signed by key {0}, which this Kiln doesn't know; if Node added a releaser, update Kiln"
    )]
    UnknownKey(String),
    #[error("the signature doesn't match (made by key {0})")]
    Bad(String),
}

/// A public key or subkey that can check signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The v4 fingerprint (SHA-1 of the key packet).
    pub fingerprint: [u8; 20],
    material: Material,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Material {
    Rsa { n: Vec<u8>, e: Vec<u8> },
    Ed25519([u8; 32]),
}

/// Upper-case hex, as OpenPGP tools print fingerprints and key IDs.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02X}");
        out
    })
}

const TAG_SIGNATURE: u8 = 2;
const TAG_PUBLIC_KEY: u8 = 6;
const TAG_PUBLIC_SUBKEY: u8 = 14;
const ALGO_RSA: u8 = 1;
const ALGO_RSA_SIGN_ONLY: u8 = 3;
const ALGO_EDDSA: u8 = 22;
/// 1.3.6.1.4.1.11591.15.1, Ed25519 in the v4 "EdDSA" form.
const ED25519_OID: &[u8] = &[0x2B, 0x06, 0x01, 0x04, 0x01, 0xDA, 0x47, 0x0F, 0x01];
const SIGTYPE_BINARY: u8 = 0x00;
const SUBPACKET_CREATED: u8 = 2;
const SUBPACKET_EXPIRES: u8 = 3;
const SUBPACKET_ISSUER: u8 = 16;
const SUBPACKET_ISSUER_FPR: u8 = 33;

#[derive(Clone, Copy)]
enum Hash {
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    fn from_id(id: u8) -> Result<Self, PgpError> {
        match id {
            8 => Ok(Self::Sha256),
            9 => Ok(Self::Sha384),
            10 => Ok(Self::Sha512),
            other => Err(PgpError::Unsupported(format!("hash algorithm {other}"))),
        }
    }

    fn digest(self, parts: &[&[u8]]) -> Vec<u8> {
        fn run<D: sha2::Digest>(parts: &[&[u8]]) -> Vec<u8> {
            let mut hasher = D::new();
            for part in parts {
                hasher.update(part);
            }
            hasher.finalize().to_vec()
        }
        match self {
            Self::Sha256 => run::<Sha256>(parts),
            Self::Sha384 => run::<Sha384>(parts),
            Self::Sha512 => run::<Sha512>(parts),
        }
    }

    fn rsa_params(self) -> &'static signature::RsaParameters {
        match self {
            Self::Sha256 => &signature::RSA_PKCS1_2048_8192_SHA256,
            Self::Sha384 => &signature::RSA_PKCS1_2048_8192_SHA384,
            Self::Sha512 => &signature::RSA_PKCS1_2048_8192_SHA512,
        }
    }
}

/// Reads bytes off the front of a slice, failing instead of panicking.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], PgpError> {
        if self.0.len() < n {
            return Err(PgpError::Malformed("truncated"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn byte(&mut self) -> Result<u8, PgpError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<usize, PgpError> {
        let b = self.take(2)?;
        Ok(usize::from(b[0]) << 8 | usize::from(b[1]))
    }

    fn u32(&mut self) -> Result<usize, PgpError> {
        let b = self.take(4)?;
        usize::try_from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .map_err(|_| PgpError::Malformed("length"))
    }

    /// A multiprecision integer: a bit count, then the big-endian bytes.
    fn mpi(&mut self) -> Result<&'a [u8], PgpError> {
        let bits = self.u16()?;
        self.take(bits.div_ceil(8))
    }
}

/// The packets in `data`: (tag, body).
fn packets(data: &[u8]) -> Result<Vec<(u8, &[u8])>, PgpError> {
    let mut reader = Reader(data);
    let mut out = Vec::new();
    while !reader.0.is_empty() {
        let header = reader.byte()?;
        if header & 0x80 == 0 {
            return Err(PgpError::Malformed("not a packet header"));
        }
        let (tag, len) = if header & 0x40 == 0 {
            // Old format: the tag in bits 5–2, the length type in bits 1–0.
            let len = match header & 0x03 {
                0 => usize::from(reader.byte()?),
                1 => reader.u16()?,
                2 => reader.u32()?,
                _ => return Err(PgpError::Malformed("indeterminate length")),
            };
            ((header >> 2) & 0x0F, len)
        } else {
            let first = usize::from(reader.byte()?);
            let len = match first {
                0..192 => first,
                192..224 => ((first - 192) << 8) + usize::from(reader.byte()?) + 192,
                255 => reader.u32()?,
                _ => return Err(PgpError::Malformed("partial body length")),
            };
            (header & 0x3F, len)
        };
        out.push((tag, reader.take(len)?));
    }
    Ok(out)
}

/// The key in a v4 public key or subkey packet, if it is one Kiln can check signatures with.
fn parse_key(body: &[u8]) -> Result<Option<Key>, PgpError> {
    let mut reader = Reader(body);
    if reader.byte()? != 4 {
        return Ok(None);
    }
    reader.take(4)?; // creation time
    let material = match reader.byte()? {
        ALGO_RSA | ALGO_RSA_SIGN_ONLY => {
            let n = reader.mpi()?.to_vec();
            let e = reader.mpi()?.to_vec();
            Material::Rsa { n, e }
        }
        ALGO_EDDSA => {
            let oid_len = usize::from(reader.byte()?);
            if reader.take(oid_len)? != ED25519_OID {
                return Ok(None);
            }
            // A native point: 0x40, then the 32-byte key.
            match reader.mpi()? {
                [0x40, key @ ..] if key.len() == 32 => {
                    Material::Ed25519(key.try_into().map_err(|_| PgpError::Malformed("key"))?)
                }
                _ => return Err(PgpError::Malformed("Ed25519 key")),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(Key {
        fingerprint: fingerprint(body)?,
        material,
    }))
}

/// A v4 key's fingerprint: SHA-1 over its packet, with a fixed header.
fn fingerprint(body: &[u8]) -> Result<[u8; 20], PgpError> {
    let len = u16::try_from(body.len()).map_err(|_| PgpError::Malformed("key length"))?;
    let mut sha1 = Sha1::new();
    sha1.update([0x99]);
    sha1.update(len.to_be_bytes());
    sha1.update(body);
    Ok(sha1.finalize().into())
}

/// The usable keys and subkeys in binary key packets.
pub fn keys_from_binary(data: &[u8]) -> Result<Vec<Key>, PgpError> {
    let mut keys = Vec::new();
    for (tag, body) in packets(data)? {
        if (tag == TAG_PUBLIC_KEY || tag == TAG_PUBLIC_SUBKEY)
            && let Some(key) = parse_key(body)?
        {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// The usable keys and subkeys in an ASCII-armored public key block.
pub fn keys_from_armored(text: &str) -> Result<Vec<Key>, PgpError> {
    keys_from_binary(&dearmor(text)?)
}

/// The fingerprint of the primary key in an armored block, whatever its algorithm (to check a
/// key file against its name).
pub fn primary_fingerprint(text: &str) -> Result<[u8; 20], PgpError> {
    let data = dearmor(text)?;
    let (_, body) = packets(&data)?
        .into_iter()
        .find(|(tag, _)| *tag == TAG_PUBLIC_KEY)
        .ok_or(PgpError::Malformed("no public key packet"))?;
    fingerprint(body)
}

/// The bytes of the first armored block in `text` (its CRC line isn't checked: the key packets'
/// fingerprints are).
fn dearmor(text: &str) -> Result<Vec<u8>, PgpError> {
    let mut lines = text.lines().map(str::trim);
    if !lines.any(|l| l.starts_with("-----BEGIN PGP ")) {
        return Err(PgpError::Malformed("no armor header"));
    }
    let mut body = String::new();
    let mut in_headers = true;
    for line in lines {
        if line.starts_with("-----END PGP ") {
            return base64::engine::general_purpose::STANDARD
                .decode(body)
                .map_err(|_| PgpError::Malformed("armor"));
        }
        if in_headers {
            in_headers = false;
            if line.contains(": ") {
                in_headers = true;
                continue;
            }
            if line.is_empty() {
                continue;
            }
        }
        if line.starts_with('=') {
            continue; // the checksum
        }
        body.push_str(line);
    }
    Err(PgpError::Malformed("no armor footer"))
}

/// Checks a detached binary signature (`.sig`) over `data` with one of `keys`. Returns the
/// fingerprint of the key that made it.
pub fn verify_detached(keys: &[Key], data: &[u8], sig: &[u8]) -> Result<[u8; 20], PgpError> {
    let packets = packets(sig)?;
    let mut signatures = packets.iter().filter(|(tag, _)| *tag == TAG_SIGNATURE);
    let Some((_, body)) = signatures.next() else {
        return Err(PgpError::Malformed("no signature packet"));
    };
    if signatures.next().is_some() {
        return Err(PgpError::Unsupported("more than one signature".into()));
    }
    verify_packet(keys, data, body)
}

fn verify_packet(keys: &[Key], data: &[u8], body: &[u8]) -> Result<[u8; 20], PgpError> {
    let mut reader = Reader(body);
    let version = reader.byte()?;
    if version != 4 {
        return Err(PgpError::Unsupported(format!("version {version}")));
    }
    let sig_type = reader.byte()?;
    if sig_type != SIGTYPE_BINARY {
        return Err(PgpError::Unsupported(format!(
            "signature type {sig_type:#04x}"
        )));
    }
    let algo = reader.byte()?;
    let hash = Hash::from_id(reader.byte()?)?;
    let hashed_len = reader.u16()?;
    let hashed = reader.take(hashed_len)?;
    let unhashed_len = reader.u16()?;
    let unhashed = reader.take(unhashed_len)?;
    reader.take(2)?; // the digest's first two bytes, a quick check only

    // Who signed: the issuer fingerprint, else the issuer key ID (unhashed is allowed: a wrong
    // one only makes the check fail).
    let mut fingerprint = None;
    let mut key_id = None;
    for (area, is_hashed) in [(hashed, true), (unhashed, false)] {
        for (kind, critical, value) in subpackets(area)? {
            match kind {
                SUBPACKET_ISSUER_FPR if value.len() == 21 && value[0] == 4 => {
                    fingerprint
                        .get_or_insert(<[u8; 20]>::try_from(&value[1..]).unwrap_or_default());
                }
                SUBPACKET_ISSUER if value.len() == 8 => {
                    key_id.get_or_insert(<[u8; 8]>::try_from(value).unwrap_or_default());
                }
                SUBPACKET_CREATED | SUBPACKET_EXPIRES | SUBPACKET_ISSUER | SUBPACKET_ISSUER_FPR => {
                }
                other if critical && is_hashed => {
                    return Err(PgpError::Unsupported(format!("critical subpacket {other}")));
                }
                _ => {}
            }
        }
    }
    let key = match (fingerprint, key_id) {
        (Some(fpr), _) => keys
            .iter()
            .find(|k| k.fingerprint == fpr)
            .ok_or_else(|| PgpError::UnknownKey(hex(&fpr)))?,
        (None, Some(id)) => keys
            .iter()
            .find(|k| k.fingerprint[12..] == id)
            .ok_or_else(|| PgpError::UnknownKey(hex(&id)))?,
        (None, None) => return Err(PgpError::Malformed("no issuer")),
    };

    // What the signature covers: the data, the signature's hashed part, and a trailer.
    let signed_part = &body[..6 + hashed_len];
    let signed_len = u32::try_from(signed_part.len()).map_err(|_| PgpError::Malformed("length"))?;
    let mut trailer = [0x04, 0xFF, 0, 0, 0, 0];
    trailer[2..].copy_from_slice(&signed_len.to_be_bytes());
    let bad = || PgpError::Bad(hex(&key.fingerprint));
    match (&key.material, algo) {
        (Material::Rsa { n, e }, ALGO_RSA | ALGO_RSA_SIGN_ONLY) => {
            let s = left_pad(reader.mpi()?, n.len()).ok_or_else(bad)?;
            let message = [data, signed_part, &trailer].concat();
            signature::RsaPublicKeyComponents { n, e }
                .verify(hash.rsa_params(), &message, &s)
                .map_err(|_| bad())?;
        }
        (Material::Ed25519(public), ALGO_EDDSA) => {
            let r = left_pad(reader.mpi()?, 32).ok_or_else(bad)?;
            let s = left_pad(reader.mpi()?, 32).ok_or_else(bad)?;
            // EdDSA in OpenPGP signs the digest, not the data.
            let digest = hash.digest(&[data, signed_part, &trailer]);
            signature::UnparsedPublicKey::new(&signature::ED25519, public)
                .verify(&digest, &[r, s].concat())
                .map_err(|_| bad())?;
        }
        _ => {
            return Err(PgpError::Unsupported(format!(
                "algorithm {algo} for key {}",
                hex(&key.fingerprint)
            )));
        }
    }
    if !reader.0.is_empty() {
        return Err(PgpError::Malformed("trailing data in signature"));
    }
    Ok(key.fingerprint)
}

/// A signature subpacket: (type, critical, value).
type Subpacket<'a> = (u8, bool, &'a [u8]);

/// The subpackets in an area.
fn subpackets(area: &[u8]) -> Result<Vec<Subpacket<'_>>, PgpError> {
    let mut reader = Reader(area);
    let mut out = Vec::new();
    while !reader.0.is_empty() {
        let first = usize::from(reader.byte()?);
        let len = match first {
            0..192 => first,
            192..255 => ((first - 192) << 8) + usize::from(reader.byte()?) + 192,
            _ => reader.u32()?,
        };
        let packet = reader.take(len)?;
        let Some((&kind, value)) = packet.split_first() else {
            return Err(PgpError::Malformed("empty subpacket"));
        };
        out.push((kind & 0x7F, kind & 0x80 != 0, value));
    }
    Ok(out)
}

/// `bytes` as exactly `len` bytes, with leading zeros; `None` if it is longer.
fn left_pad(bytes: &[u8], len: usize) -> Option<Vec<u8>> {
    let pad = len.checked_sub(bytes.len())?;
    let mut out = vec![0; pad];
    out.extend_from_slice(bytes);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures")
                .join(name),
        )
        .unwrap()
    }

    fn keys() -> Vec<Key> {
        crate::keys::release_keys().unwrap()
    }

    #[test]
    fn every_release_key_file_parses_to_its_own_fingerprint() {
        let mut usable = 0;
        for (name, text) in crate::keys::FILES {
            assert_eq!(hex(&primary_fingerprint(text).unwrap()), *name);
            let keys = keys_from_armored(text).unwrap();
            assert!(!keys.is_empty(), "{name} has a key Kiln can check with");
            usable += keys.len();
        }
        // A DSA primary from 2016 is skipped; its RSA subkey isn't.
        assert!(usable >= crate::keys::FILES.len());
    }

    #[test]
    fn checks_real_release_signatures() {
        let keys = keys();
        for (version, signer) in [
            // RSA 4096, the primary key (Marco Ippolito).
            ("v20.20.2", "CC68F5A3106FF448322E48ED27F5E38D5B0A215F"),
            // Ed25519, the primary key (Antoine du Hamel).
            ("v22.23.4", "5BE8A3F6C8A5C01D106C0AD820B1A390B168D356"),
            // Ed25519 signing subkey of an RSA primary (Michaël Zasso).
            ("v24.12.0", "86C8D74642E67846F8E120284DAA80D1E737BC9F"),
        ] {
            let data = fixture(&format!("SHASUMS256-{version}.txt"));
            let sig = fixture(&format!("SHASUMS256-{version}.txt.sig"));
            assert_eq!(
                hex(&verify_detached(&keys, &data, &sig).unwrap()),
                signer,
                "{version}"
            );
        }
    }

    #[test]
    fn a_changed_document_or_signature_fails() {
        let keys = keys();
        for version in ["v20.20.2", "v22.23.4"] {
            let data = fixture(&format!("SHASUMS256-{version}.txt"));
            let sig = fixture(&format!("SHASUMS256-{version}.txt.sig"));
            let mut changed = data.clone();
            changed[10] ^= 1;
            assert!(matches!(
                verify_detached(&keys, &changed, &sig),
                Err(PgpError::Bad(_))
            ));
            let mut extra = data.clone();
            extra.push(b'\n');
            assert!(verify_detached(&keys, &extra, &sig).is_err());
            // The last byte is part of the signature value.
            let mut bad_sig = sig.clone();
            *bad_sig.last_mut().unwrap() ^= 1;
            assert!(
                verify_detached(&keys, &data, &bad_sig).is_err(),
                "{version}"
            );
        }
    }

    #[test]
    fn an_unknown_signer_is_named() {
        let data = fixture("SHASUMS256-v22.23.4.txt");
        let sig = fixture("SHASUMS256-v22.23.4.txt.sig");
        let others: Vec<Key> = keys()
            .into_iter()
            .filter(|k| hex(&k.fingerprint) != "5BE8A3F6C8A5C01D106C0AD820B1A390B168D356")
            .collect();
        let err = verify_detached(&others, &data, &sig).unwrap_err();
        assert_eq!(
            err,
            PgpError::UnknownKey("5BE8A3F6C8A5C01D106C0AD820B1A390B168D356".into())
        );
        assert!(err.to_string().contains("update Kiln"));
    }

    #[test]
    fn garbage_is_refused_without_panicking() {
        let keys = keys();
        let data = b"x";
        for sig in [
            &b""[..],
            b"\x00",
            b"\x88",
            b"\x88\xff",
            b"\xc2\xff\xff\xff\xff\xff",
            b"\x88\x05\x04\x00\x16\x08\x00",
            b"\x89\x00\x08\x04\x00\x01\x08\xff\xff\x00\x00",
        ] {
            assert!(verify_detached(&keys, data, sig).is_err(), "{sig:?}");
        }
        assert!(keys_from_armored("not armor").is_err());
        assert!(keys_from_binary(b"\x99\x00\x10\x04").is_err());
    }
}
