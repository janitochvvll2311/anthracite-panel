//! PEM parsing and the key/certificate checks the manager owns.
//!
//! The parser is deliberately read-only: it turns bytes into records and never
//! writes a key anywhere. The service decides custody and publication.

use std::io::Cursor;

use anth_contract::Error;
use p256::elliptic_curve::sec1::ToEncodedPoint as _;
use p256::pkcs8::DecodePrivateKey as _;
use rsa::pkcs1::{DecodeRsaPrivateKey as _, EncodeRsaPublicKey as _};
use rustls_pki_types::PrivateKeyDer;
use sha2::{Digest, Sha256};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::*;

/// One parsed certificate, with the fields the inventory exposes.
#[derive(Debug, Clone)]
pub struct CertInfo {
    pub der: Vec<u8>,
    pub names: Vec<String>,
    pub issuer: String,
    pub not_before: u64,
    pub not_after: u64,
    pub key_type: String,
    pub fingerprint: String,
    /// The `subjectPublicKey` bits, used to match a private key.
    pub spki: Vec<u8>,
}

/// A private key this service knows how to match against a certificate.
pub enum KeyPair {
    Rsa(rsa::RsaPrivateKey),
    P256(p256::SecretKey),
    Ed25519(ed25519_dalek::SigningKey),
}

impl KeyPair {
    /// The public key as it appears in a certificate's `subjectPublicKey`.
    pub fn public_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            Self::Rsa(key) => key
                .to_public_key()
                .to_pkcs1_der()
                .map(|der| der.as_bytes().to_vec())
                .map_err(|error| Error::invalid(format!("reading the RSA public key: {error}"))),
            Self::P256(key) => Ok(key.public_key().to_encoded_point(false).as_bytes().to_vec()),
            Self::Ed25519(key) => Ok(key.verifying_key().to_bytes().to_vec()),
        }
    }
}

/// Parses every certificate in a PEM document.
pub fn parse_certs(pem: &str) -> Result<Vec<CertInfo>, Error> {
    let mut reader = Cursor::new(pem.as_bytes());
    let ders = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| Error::invalid(format!("reading the certificate PEM: {error}")))?;
    if ders.is_empty() {
        return Err(Error::invalid(
            "the certificate PEM contains no certificate",
        ));
    }
    let mut certs = Vec::with_capacity(ders.len());
    for der in ders {
        certs.push(parse_cert(&der)?);
    }
    Ok(certs)
}

fn parse_cert(der: &[u8]) -> Result<CertInfo, Error> {
    let (_, cert) = parse_x509_certificate(der)
        .map_err(|error| Error::invalid(format!("parsing an X.509 certificate: {error}")))?;
    let not_before = cert.validity().not_before.timestamp().max(0) as u64;
    let not_after = cert.validity().not_after.timestamp().max(0) as u64;
    let algorithm = cert.public_key().algorithm.algorithm.to_id_string();
    let key_type = match algorithm.as_str() {
        "1.2.840.113549.1.1.1" => "rsa",
        "1.2.840.10045.2.1" => "ecdsa",
        "1.3.101.112" => "ed25519",
        _ => "unknown",
    }
    .to_string();
    let spki = cert.public_key().subject_public_key.data.to_vec();
    let fingerprint = hex(&Sha256::digest(der));
    let issuer = name_text(cert.issuer());
    let mut names = Vec::new();
    if let Some(cn) = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|value| value.as_str().ok())
    {
        names.push(cn.to_string());
    }
    if let Ok(Some(san)) = cert.subject_alternative_name() {
        for name in &san.value.general_names {
            match name {
                GeneralName::DNSName(dns) => names.push(dns.to_string()),
                GeneralName::IPAddress(bytes) => {
                    let address = match bytes.len() {
                        4 => std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                            bytes[0], bytes[1], bytes[2], bytes[3],
                        )),
                        16 => {
                            let mut octets = [0u8; 16];
                            octets.copy_from_slice(bytes);
                            std::net::IpAddr::V6(std::net::Ipv6Addr::from(octets))
                        }
                        _ => continue,
                    };
                    names.push(address.to_string());
                }
                _ => {}
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(CertInfo {
        der: der.to_vec(),
        names,
        issuer,
        not_before,
        not_after,
        key_type,
        fingerprint,
        spki,
    })
}

fn name_text(name: &X509Name<'_>) -> String {
    name.iter_common_name()
        .next()
        .and_then(|value| value.as_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}

/// Parses one private key in PKCS#8, PKCS#1 or SEC1 PEM form.
pub fn parse_key(pem: &str) -> Result<KeyPair, Error> {
    let mut reader = Cursor::new(pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|error| Error::invalid(format!("reading the private key PEM: {error}")))?
        .ok_or_else(|| Error::invalid("the key PEM contains no private key"))?;
    match key {
        PrivateKeyDer::Pkcs8(key) => {
            let der = key.secret_pkcs8_der();
            if let Ok(key) = rsa::RsaPrivateKey::from_pkcs8_der(der) {
                return Ok(KeyPair::Rsa(key));
            }
            if let Ok(key) = p256::SecretKey::from_pkcs8_der(der) {
                return Ok(KeyPair::P256(key));
            }
            if let Ok(key) = ed25519_dalek::SigningKey::from_pkcs8_der(der) {
                return Ok(KeyPair::Ed25519(key));
            }
            Err(Error::invalid(
                "the PKCS#8 key is not RSA, ECDSA P-256 or Ed25519",
            ))
        }
        PrivateKeyDer::Pkcs1(key) => {
            let key = rsa::RsaPrivateKey::from_pkcs1_der(key.secret_pkcs1_der())
                .map_err(|error| Error::invalid(format!("reading the RSA key: {error}")))?;
            Ok(KeyPair::Rsa(key))
        }
        PrivateKeyDer::Sec1(key) => {
            let key = p256::SecretKey::from_sec1_der(key.secret_sec1_der())
                .map_err(|error| Error::invalid(format!("reading the EC key: {error}")))?;
            Ok(KeyPair::P256(key))
        }
        _ => Err(Error::invalid("unsupported private key format")),
    }
}

/// Whether the key is the one the leaf certificate names.
pub fn key_matches(key: &KeyPair, cert: &CertInfo) -> Result<bool, Error> {
    Ok(key.public_bytes()? == cert.spki)
}

/// Whether a chain is linked and signed leaf → intermediates → self-signed root.
/// Name matching alone is not enough: each certificate must verify against the
/// next one's public key, and the last one must verify against itself.
pub fn chain_ok(certs: &[CertInfo]) -> bool {
    if certs.is_empty() {
        return false;
    }
    let parsed: Vec<X509Certificate<'_>> = match certs
        .iter()
        .map(|cert| parse_x509_certificate(&cert.der))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(parsed) => parsed.into_iter().map(|(_, cert)| cert).collect(),
        Err(_) => return false,
    };
    for pair in parsed.windows(2) {
        if pair[0].issuer().as_raw() != pair[1].subject().as_raw() {
            return false;
        }
        if pair[0]
            .verify_signature(Some(pair[1].public_key()))
            .is_err()
        {
            return false;
        }
    }
    let Some(last) = parsed.last() else {
        return false;
    };
    last.issuer().as_raw() == last.subject().as_raw() && last.verify_signature(None).is_ok()
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text
}
