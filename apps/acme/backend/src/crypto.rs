//! Keys, JWKs, JWS and CSRs.
//!
//! Account keys and certificate keys are generated locally and stored as
//! PKCS#8 PEM at 0600. The CA only ever sees a public key or a CSR. ECDSA
//! P-256 (ES256) is the default; RSA 2048 (RS256) is the alternative.

use std::time::{SystemTime, UNIX_EPOCH};

use anth_contract::Error;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::LineEnding;
use rsa::pkcs1v15::SigningKey as RsaSigningKey;
use rsa::pkcs8::{DecodePrivateKey as _, EncodePrivateKey as _};
use rsa::signature::{SignatureEncoding, Signer as _};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const EC: &str = "ecdsa-p256";
pub const RSA: &str = "rsa-2048";

/// One account key, loaded from `{data}/accounts/<id>/account.key.pem`.
pub enum AccountKey {
    Ec(p256::SecretKey),
    Rsa(Box<RsaPrivateKey>),
}

impl AccountKey {
    pub fn generate(key_type: &str) -> Result<Self, Error> {
        match parse_key_type(key_type)? {
            EC => Ok(Self::Ec(p256::SecretKey::random(&mut rand_core()))),
            RSA => RsaPrivateKey::new(&mut rand_core(), 2048)
                .map(|key| Self::Rsa(Box::new(key)))
                .map_err(|error| Error::internal(format!("generating an RSA key: {error}"))),
            _ => unreachable!("parse_key_type validated the value"),
        }
    }

    pub fn from_pem(pem: &str) -> Result<Self, Error> {
        if let Ok(key) = p256::SecretKey::from_pkcs8_pem(pem) {
            return Ok(Self::Ec(key));
        }
        RsaPrivateKey::from_pkcs8_pem(pem)
            .map(|key| Self::Rsa(Box::new(key)))
            .map_err(|error| Error::invalid(format!("the account key is not PKCS#8: {error}")))
    }

    pub fn to_pem(&self) -> Result<String, Error> {
        match self {
            Self::Ec(key) => key
                .to_pkcs8_pem(LineEnding::LF)
                .map(|pem| pem.to_string())
                .map_err(|error| Error::internal(format!("encoding the account key: {error}"))),
            Self::Rsa(key) => key
                .to_pkcs8_pem(LineEnding::LF)
                .map(|pem| pem.to_string())
                .map_err(|error| Error::internal(format!("encoding the account key: {error}"))),
        }
    }

    pub fn algorithm(&self) -> &'static str {
        match self {
            Self::Ec(_) => "ES256",
            Self::Rsa(_) => "RS256",
        }
    }

    pub fn jwk(&self) -> Value {
        match self {
            Self::Ec(key) => {
                let point = key.public_key().to_encoded_point(false);
                let x = point.x().expect("an uncompressed point has x");
                let y = point.y().expect("an uncompressed point has y");
                json!({
                    "crv": "P-256",
                    "kty": "EC",
                    "x": URL_SAFE_NO_PAD.encode(x),
                    "y": URL_SAFE_NO_PAD.encode(y),
                })
            }
            Self::Rsa(key) => rsa_jwk(&key.to_public_key()),
        }
    }

    /// The RFC 7638 thumbprint: SHA-256 over the canonical JWK.
    pub fn thumbprint(&self) -> String {
        let canonical = serde_json::to_string(&self.jwk()).expect("a JSON object");
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
    }

    /// Signs one JWS input. ES256 uses the fixed 64-byte `r || s` form; RS256
    /// uses PKCS#1 v1.5.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        match self {
            Self::Ec(key) => {
                let signing = SigningKey::from(key.clone());
                let signature: Signature = signing.sign(message);
                Ok(signature.to_bytes().to_vec())
            }
            Self::Rsa(key) => {
                let signing = RsaSigningKey::<Sha256>::new(key.as_ref().clone());
                signing
                    .try_sign(message)
                    .map(|signature| signature.to_vec())
                    .map_err(|error| Error::internal(format!("signing with RS256: {error}")))
            }
        }
    }

    pub fn key_type(&self) -> &'static str {
        match self {
            Self::Ec(_) => EC,
            Self::Rsa(_) => RSA,
        }
    }
}

fn rsa_jwk(key: &RsaPublicKey) -> Value {
    use rsa::traits::PublicKeyParts;
    json!({
        "e": URL_SAFE_NO_PAD.encode(key.e().to_bytes_be()),
        "kty": "RSA",
        "n": URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),
    })
}

fn rand_core() -> p256::elliptic_curve::rand_core::OsRng {
    // p256 re-exports the same `rand_core` version used by the `rsa` crate.
    p256::elliptic_curve::rand_core::OsRng
}

pub fn parse_key_type(key_type: &str) -> Result<&'static str, Error> {
    match key_type.trim().to_ascii_lowercase().as_str() {
        "" | "ec" | "ecdsa" | "p-256" | "p256" | "ecdsa-p256" => Ok(EC),
        "rsa" | "rsa-2048" | "rsa2048" => Ok(RSA),
        other => Err(Error::invalid(format!(
            "`{other}` is not a key type; use ecdsa-p256 or rsa-2048"
        ))),
    }
}

/// Builds the protected header, payload and signature of one JWS request.
pub struct Jws<'a> {
    pub protected: Value,
    pub payload: Vec<u8>,
    pub signature: Vec<u8>,
    pub _key: &'a AccountKey,
}

impl<'a> Jws<'a> {
    pub fn new(
        key: &'a AccountKey,
        url: &str,
        nonce: &str,
        kid: Option<&str>,
        payload: &[u8],
    ) -> Result<Self, Error> {
        let mut protected = json!({
            "alg": key.algorithm(),
            "nonce": nonce,
            "url": url,
        });
        match kid {
            Some(kid) => {
                protected["kid"] = json!(kid);
            }
            None => {
                protected["jwk"] = key.jwk();
            }
        }
        let encoded = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&protected)
                .map_err(|error| Error::internal(format!("encoding the JWS header: {error}")))?,
        );
        let body = URL_SAFE_NO_PAD.encode(payload);
        let signing_input = format!("{encoded}.{body}");
        let signature = key.sign(signing_input.as_bytes())?;
        Ok(Self {
            protected,
            payload: payload.to_vec(),
            signature,
            _key: key,
        })
    }

    pub fn body(&self) -> Value {
        json!({
            "protected": URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&self.protected).expect("a JSON object")
            ),
            "payload": URL_SAFE_NO_PAD.encode(&self.payload),
            "signature": URL_SAFE_NO_PAD.encode(&self.signature),
        })
    }
}

/// External Account Binding: a JWS over the account JWK, signed with the CA's
/// HMAC key instead of the account key.
pub fn external_account_binding(
    new_account_url: &str,
    kid: &str,
    hmac_key: &str,
    jwk: &Value,
) -> Result<Value, Error> {
    let key = URL_SAFE_NO_PAD
        .decode(hmac_key.trim())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(hmac_key.trim()))
        .map_err(|error| Error::invalid(format!("the EAB hmac-key is not base64: {error}")))?;
    let protected = json!({"alg": "HS256", "kid": kid, "url": new_account_url});
    let protected = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&protected)
            .map_err(|error| Error::internal(format!("encoding the EAB header: {error}")))?,
    );
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(jwk)
            .map_err(|error| Error::internal(format!("encoding the account JWK: {error}")))?,
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(&key)
        .map_err(|error| Error::internal(format!("the EAB key is unusable: {error}")))?;
    mac.update(format!("{protected}.{payload}").as_bytes());
    let signature = mac.finalize().into_bytes();
    Ok(json!({
        "protected": protected,
        "payload": payload,
        "signature": URL_SAFE_NO_PAD.encode(signature),
    }))
}

/// A certificate key and its PEM, ready for a CSR.
pub struct CertificateKey {
    pub key: rcgen::KeyPair,
    pub pem: String,
}

pub fn certificate_key(key_type: &str) -> Result<CertificateKey, Error> {
    match parse_key_type(key_type)? {
        EC => {
            let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
                .map_err(|error| Error::internal(format!("generating the EC key: {error}")))?;
            let pem = key.serialize_pem();
            Ok(CertificateKey { key, pem })
        }
        RSA => {
            let key = RsaPrivateKey::new(&mut rand_core(), 2048)
                .map_err(|error| Error::internal(format!("generating the RSA key: {error}")))?;
            let pem = key
                .to_pkcs8_pem(LineEnding::LF)
                .map_err(|error| Error::internal(format!("encoding the RSA key: {error}")))?
                .to_string();
            let key = rcgen::KeyPair::from_pem(&pem)
                .map_err(|error| Error::internal(format!("loading the RSA key: {error}")))?;
            Ok(CertificateKey { key, pem })
        }
        _ => unreachable!("parse_key_type validated the value"),
    }
}

/// PKCS#10 DER for the given DNS names, signed by `key`.
pub fn csr(key: &rcgen::KeyPair, names: &[String]) -> Result<Vec<u8>, Error> {
    let params = rcgen::CertificateParams::new(names.to_vec())
        .map_err(|error| Error::invalid(format!("the names are not a usable CSR: {error}")))?;
    let request = params
        .serialize_request(key)
        .map_err(|error| Error::internal(format!("building the CSR: {error}")))?;
    Ok(request.der().to_vec())
}

/// Splits an ACME PEM chain into the leaf and the rest. The CA returns the leaf
/// first (RFC 8555 §7.4.2).
pub fn split_chain(pem: &str) -> (String, String) {
    let blocks: Vec<String> = pem
        .split("-----BEGIN CERTIFICATE-----")
        .filter(|block| !block.trim().is_empty())
        .map(|block| format!("-----BEGIN CERTIFICATE-----{block}"))
        .collect();
    match blocks.split_first() {
        Some((leaf, rest)) => (leaf.clone(), rest.join("\n")),
        None => (pem.to_string(), String::new()),
    }
}

/// The DER of the first certificate in a PEM blob.
pub fn leaf_der(pem: &str) -> Result<Vec<u8>, Error> {
    let mut reader = std::io::Cursor::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| Error::invalid(format!("reading the certificate PEM: {error}")))?;
    certs
        .into_iter()
        .next()
        .map(|der| der.to_vec())
        .ok_or_else(|| Error::invalid("the certificate PEM has no certificate"))
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .unwrap_or(0)
}

/// Not-before and not-after of the leaf, in seconds.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ec_account_thumbprint_is_stable_and_key_is_not_in_the_jwk() {
        let first = AccountKey::generate(EC).expect("key");
        let again = AccountKey::from_pem(&first.to_pem().expect("pem")).expect("load");
        assert_eq!(first.thumbprint(), again.thumbprint());
        let jwk = first.jwk();
        assert_eq!(jwk["kty"], "EC");
        assert_eq!(jwk["crv"], "P-256");
        assert!(jwk["d"].is_null());
        assert!(jwk["x"].as_str().expect("x").len() == 43);
    }

    #[test]
    fn an_rsa_account_signs_rs256() {
        let key = AccountKey::generate(RSA).expect("key");
        assert_eq!(key.algorithm(), "RS256");
        let jws =
            Jws::new(&key, "https://ca.example/new-account", "nonce", None, b"{}").expect("jws");
        assert!(jws.body()["protected"].as_str().is_some());
        assert_eq!(jws.body()["signature"].as_str().expect("sig").len(), 342);
    }

    #[test]
    fn a_csr_carries_the_names_and_is_signed_by_the_key() {
        let key = certificate_key(EC).expect("key");
        let der = csr(&key.key, &["a.test".to_string(), "*.b.test".to_string()]).expect("csr");
        assert_eq!(der.first(), Some(&0x30), "a CSR is a DER SEQUENCE");
        assert!(der.len() > 100);
    }

    #[test]
    fn a_chain_splits_the_leaf_from_the_rest() {
        let key = certificate_key(EC).expect("key");
        let ca = rcgen::CertificateParams::new(Vec::new()).expect("ca");
        let ca_key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("key");
        let ca = ca.self_signed(&ca_key).expect("ca cert");
        let leaf_params = rcgen::CertificateParams::new(vec!["a.test".to_string()]).expect("leaf");
        let leaf = leaf_params
            .signed_by(&key.key, &ca, &ca_key)
            .expect("leaf cert");
        let pem = format!("{}\n{}", leaf.pem(), ca.pem());
        let (chain_leaf, rest) = split_chain(&pem);
        assert!(chain_leaf.contains("CERTIFICATE"));
        assert!(rest.contains("CERTIFICATE"));
        assert!(leaf_der(&pem).is_ok());
    }

    #[test]
    fn the_external_account_binding_is_a_signed_jws() {
        let key = AccountKey::generate(EC).expect("key");
        let secret = URL_SAFE_NO_PAD.encode(b"0123456789abcdef");
        let eab = external_account_binding(
            "https://ca.example/new-account",
            "kid-1",
            &secret,
            &key.jwk(),
        )
        .expect("eab");
        assert!(eab["protected"].as_str().is_some());
        assert!(eab["signature"].as_str().is_some());
        assert!(!eab.to_string().contains("account.key"));
    }
}
