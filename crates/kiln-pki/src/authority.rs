//! Certificate generation (ADR 0010 §1–3).

use kiln_core::Tld;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, GeneralSubtree, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, NameConstraints, PKCS_ECDSA_P256_SHA256,
};
use sha1::Digest as _;
use time::{Duration, OffsetDateTime};

/// Every Kiln root's subject starts with this; trust code refuses anything else.
pub const ROOT_MARKER: &str = "Kiln Local Development CA";
/// Root validity (3 years).
pub const ROOT_DAYS: i64 = 3 * 365;
/// Intermediate validity; Kiln re-issues before expiry.
pub const INTERMEDIATE_DAYS: i64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum PkiError {
    #[error("certificate generation failed: {0}")]
    Generate(#[from] rcgen::Error),
    #[error("cannot parse certificate: {0}")]
    Parse(String),
    #[error("not a Kiln root CA: {0}")]
    NotKilnRoot(&'static str),
}

/// A freshly generated root. `key_der` is secret: callers must protect it before storing.
pub struct Root {
    pub cert_der: Vec<u8>,
    pub cert_pem: String,
    pub key_der: Vec<u8>,
}

impl std::fmt::Debug for Root {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Root")
            .field("cert_der_len", &self.cert_der.len())
            .finish_non_exhaustive() // never print the key
    }
}

/// An intermediate for the web server (its key is meant to be written for Caddy).
#[derive(Debug, Clone)]
pub struct Intermediate {
    pub cert_der: Vec<u8>,
    pub cert_pem: String,
    pub key_pem: String,
    pub not_after: OffsetDateTime,
}

/// Lowercase hex SHA-1 thumbprint, as Windows identifies certificates.
#[must_use]
pub fn sha1_thumbprint(der: &[u8]) -> [u8; 20] {
    sha1::Sha1::digest(der).into()
}

/// Colon-separated uppercase SHA-256 fingerprint, for display.
#[must_use]
pub fn sha256_fingerprint(der: &[u8]) -> String {
    sha2::Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn constraints(tld: &Tld) -> NameConstraints {
    NameConstraints {
        permitted_subtrees: vec![GeneralSubtree::DnsName(tld.as_str().to_owned())],
        excluded_subtrees: vec![],
    }
}

/// Days are whole and small; an out-of-range date is a bug, not an input error.
fn days(n: i64) -> Duration {
    Duration::days(n)
}

/// Generates a root valid for [`ROOT_DAYS`] from `now` whose Name Constraints permit only
/// names under `tld`. `owner` (e.g. `user@host`) makes the subject unique and recognisable.
pub fn generate_root(tld: &Tld, owner: &str, now: OffsetDateTime) -> Result<Root, PkiError> {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(
        DnType::CommonName,
        format!("{ROOT_MARKER} ({owner}, {})", now.date()),
    );
    dn.push(DnType::OrganizationName, "Kiln");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(1));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.name_constraints = Some(constraints(tld));
    params.not_before = now - days(1); // tolerate small clock differences
    params.not_after = now + days(ROOT_DAYS);
    let cert = params.self_signed(&key)?;
    Ok(Root {
        cert_der: cert.der().to_vec(),
        cert_pem: cert.pem(),
        key_der: key.serialize_der(),
    })
}

/// Issues an intermediate valid for [`INTERMEDIATE_DAYS`] from `now`, with the same name
/// constraints and `pathlen:0`, signed by the root (`root_cert_der` + `root_key_der`).
pub fn issue_intermediate(
    root_cert_der: &[u8],
    root_key_der: &[u8],
    tld: &Tld,
    now: OffsetDateTime,
) -> Result<Intermediate, PkiError> {
    let root_key = KeyPair::try_from(root_key_der)?;
    let issuer = Issuer::from_ca_cert_der(&root_cert_der.into(), root_key)?;

    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(
        DnType::CommonName,
        format!("Kiln Intermediate CA ({})", now.date()),
    );
    dn.push(DnType::OrganizationName, "Kiln");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.name_constraints = Some(constraints(tld));
    params.not_before = now - days(1);
    params.not_after = now + days(INTERMEDIATE_DAYS);
    params.use_authority_key_identifier_extension = true;
    let cert = params.signed_by(&key, &issuer)?;
    Ok(Intermediate {
        cert_der: cert.der().to_vec(),
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
        not_after: params.not_after,
    })
}

/// Subject and expiry of a DER certificate, for display and checks.
pub fn describe(der: &[u8]) -> Result<(String, OffsetDateTime), PkiError> {
    let (_, cert) =
        x509_parser::parse_x509_certificate(der).map_err(|e| PkiError::Parse(e.to_string()))?;
    Ok((
        cert.subject().to_string(),
        cert.validity().not_after.to_datetime(),
    ))
}

/// The one TLD a Kiln root certificate (DER) may vouch for, after checking it is a Kiln
/// root: a CA named as one, whose **critical** name constraints permit DNS names under
/// exactly one domain and nothing else. Used before trusting a certificate machine-wide
/// ([ADR 0021] §7); the caller still checks the TLD itself (public TLDs are refused).
///
/// [ADR 0021]: ../../../docs/adr/0021-linux-ca-key-and-trust.md
pub fn kiln_root_tld(der: &[u8]) -> Result<String, PkiError> {
    use x509_parser::extensions::GeneralName;
    let (_, cert) =
        x509_parser::parse_x509_certificate(der).map_err(|e| PkiError::Parse(e.to_string()))?;
    if !cert.subject().to_string().contains(ROOT_MARKER) {
        return Err(PkiError::NotKilnRoot(
            "its subject doesn't name a Kiln root",
        ));
    }
    let is_ca = cert
        .basic_constraints()
        .map_err(|e| PkiError::Parse(e.to_string()))?
        .is_some_and(|bc| bc.value.ca);
    if !is_ca {
        return Err(PkiError::NotKilnRoot("it isn't a CA certificate"));
    }
    let constraints = cert
        .name_constraints()
        .map_err(|e| PkiError::Parse(e.to_string()))?
        .ok_or(PkiError::NotKilnRoot("it has no name constraints"))?;
    if !constraints.critical {
        return Err(PkiError::NotKilnRoot(
            "its name constraints aren't critical",
        ));
    }
    match constraints.value.permitted_subtrees.as_deref() {
        Some([subtree]) => match &subtree.base {
            GeneralName::DNSName(domain) if !domain.is_empty() => Ok((*domain).to_owned()),
            _ => Err(PkiError::NotKilnRoot(
                "it permits something other than DNS names",
            )),
        },
        _ => Err(PkiError::NotKilnRoot(
            "its name constraints don't permit exactly one domain",
        )),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rcgen::{ExtendedKeyUsagePurpose, SanType};
    use rustls_pki_types::{CertificateDer, ServerName, UnixTime};

    pub(crate) fn now() -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }

    /// A leaf for `name`, signed by the intermediate: what Caddy will issue in P1-06.
    pub(crate) fn leaf(inter: &Intermediate, name: &str) -> Vec<u8> {
        let issuer_key = KeyPair::from_pem(&inter.key_pem).unwrap();
        let issuer =
            Issuer::from_ca_cert_der(&inter.cert_der.as_slice().into(), issuer_key).unwrap();
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = CertificateParams::default();
        params.subject_alt_names = vec![SanType::DnsName(name.try_into().unwrap())];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.not_before = now() - days(1);
        params.not_after = now() + days(7);
        params.signed_by(&key, &issuer).unwrap().der().to_vec()
    }

    /// Independent verification with rustls-webpki (enforces name constraints).
    pub(crate) fn verifies(
        root_der: &[u8],
        inter: &Intermediate,
        leaf_der: &[u8],
        name: &str,
    ) -> bool {
        let root = CertificateDer::from(root_der);
        let anchors = [webpki::anchor_from_trusted_cert(&root).unwrap()];
        let leaf = CertificateDer::from(leaf_der);
        let Ok(ee) = webpki::EndEntityCert::try_from(&leaf) else {
            return false;
        };
        let intermediates = [CertificateDer::from(inter.cert_der.as_slice())];
        let time = UnixTime::since_unix_epoch(std::time::Duration::from_secs(
            u64::try_from(now().unix_timestamp()).unwrap(),
        ));
        let chain_ok = ee
            .verify_for_usage(
                webpki::ALL_VERIFICATION_ALGS,
                &anchors,
                &intermediates,
                time,
                webpki::KeyUsage::server_auth(),
                None,
                None,
            )
            .is_ok();
        chain_ok
            && ee
                .verify_is_valid_for_subject_name(&ServerName::try_from(name).unwrap())
                .is_ok()
    }

    #[test]
    fn the_chain_vouches_for_the_tld_and_nothing_else() {
        let tld = Tld::default();
        let root = generate_root(&tld, "dev@box", now()).unwrap();
        let inter = issue_intermediate(&root.cert_der, &root.key_der, &tld, now()).unwrap();

        for name in ["blog.test", "api.blog.test"] {
            assert!(
                verifies(&root.cert_der, &inter, &leaf(&inter, name), name),
                "{name}"
            );
        }
        // Name constraints: the CA cannot vouch for real domains even if its keys leak.
        for name in ["example.com", "test.example.com", "bank.test.com"] {
            assert!(
                !verifies(&root.cert_der, &inter, &leaf(&inter, name), name),
                "{name}"
            );
        }
    }

    #[test]
    fn only_a_constrained_kiln_root_passes_the_system_trust_check() {
        let root = generate_root(&Tld::new("wip").unwrap(), "dev@box", now()).unwrap();
        assert_eq!(kiln_root_tld(&root.cert_der).unwrap(), "wip");

        // An unconstrained CA calling itself a Kiln root.
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, format!("{ROOT_MARKER} (impostor)"));
        params.distinguished_name = dn;
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let impostor = params.self_signed(&key).unwrap();
        assert!(matches!(
            kiln_root_tld(impostor.der()),
            Err(PkiError::NotKilnRoot(_))
        ));

        // A constrained CA that isn't Kiln's, and a Kiln-named leaf that isn't a CA.
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.name_constraints = Some(constraints(&Tld::default()));
        let other = params.self_signed(&key).unwrap();
        assert!(kiln_root_tld(other.der()).is_err());
        let inter =
            issue_intermediate(&root.cert_der, &root.key_der, &Tld::default(), now()).unwrap();
        assert!(kiln_root_tld(&leaf(&inter, "blog.test")).is_err());
        assert!(kiln_root_tld(b"not a certificate").is_err());
    }

    #[test]
    fn roots_and_intermediates_have_the_documented_shape() {
        let tld = Tld::new("wip").unwrap();
        let t = now();
        let root = generate_root(&tld, "dev@box", t).unwrap();
        let (subject, not_after) = describe(&root.cert_der).unwrap();
        assert!(
            subject.contains(ROOT_MARKER) && subject.contains("dev@box"),
            "{subject}"
        );
        assert!((not_after - t - days(ROOT_DAYS)).abs() < Duration::minutes(1));

        let inter = issue_intermediate(&root.cert_der, &root.key_der, &tld, t).unwrap();
        assert!((inter.not_after - t - days(INTERMEDIATE_DAYS)).abs() < Duration::minutes(1));
        assert!(inter.key_pem.contains("PRIVATE KEY"));

        let (_, cert) = x509_parser::parse_x509_certificate(&root.cert_der).unwrap();
        let nc = cert
            .extensions()
            .iter()
            .find(|e| e.oid == x509_parser::oid_registry::OID_X509_EXT_NAME_CONSTRAINTS)
            .unwrap();
        assert!(nc.critical, "name constraints must be critical");
        let bc = cert.basic_constraints().unwrap().unwrap().value;
        assert!(bc.ca && bc.path_len_constraint == Some(1));
    }

    #[test]
    fn thumbprints_are_stable_and_distinct() {
        let tld = Tld::default();
        let a = generate_root(&tld, "a", now()).unwrap();
        let b = generate_root(&tld, "b", now()).unwrap();
        assert_eq!(sha1_thumbprint(&a.cert_der), sha1_thumbprint(&a.cert_der));
        assert_ne!(sha1_thumbprint(&a.cert_der), sha1_thumbprint(&b.cert_der));
        assert_eq!(sha256_fingerprint(&a.cert_der).len(), 32 * 3 - 1);
        assert!(
            !format!("{a:?}").contains("key_der: ["),
            "Debug never prints the key"
        );
    }
}
