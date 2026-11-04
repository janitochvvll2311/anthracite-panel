//! Ed25519 over the canonical bundle message (D-Signature-Algorithm).
//!
//! This lives in the engine, not in the contract: the CLI links the contract and
//! must not carry curve25519 for a signature it never checks. The registry is
//! the only caller; the formats are pinned here.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use anth_contract::Error;

pub const SIGNATURE_BYTES: usize = 64;
pub const PUBLIC_KEY_BYTES: usize = 32;

/// `manifest.sig`: the 64-byte signature in standard base64, one line.
pub fn parse_signature(text: &str) -> Result<[u8; SIGNATURE_BYTES], Error> {
    let bytes = STANDARD.decode(text.trim()).map_err(|error| {
        Error::invalid(format!("the signature is not standard base64: {error}"))
    })?;
    bytes
        .try_into()
        .map_err(|_| Error::invalid("an Ed25519 signature is 64 bytes"))
}

pub fn signature_base64(signature: &[u8; SIGNATURE_BYTES]) -> String {
    STANDARD.encode(signature)
}

/// A public key is 32 bytes, lowercase hex: the same alphabet as a fingerprint.
pub fn public_key(text: &str) -> Result<[u8; PUBLIC_KEY_BYTES], Error> {
    let bytes = hex_decode(text)?;
    bytes
        .try_into()
        .map_err(|_| Error::invalid("an Ed25519 public key is 32 bytes"))
}

/// `sha256(public key)`, lowercase hex. It names the key file.
pub fn fingerprint(text: &str) -> Result<String, Error> {
    Ok(hex_encode(&Sha256::digest(public_key(text)?)))
}

/// Verifies `signature` over `message` with `public_key_hex`.
///
/// `verify_strict` rejects small-order and non-canonical keys, which is the
/// right default for an artifact that grants authority.
pub fn verify(message: &[u8], signature: &str, public_key_hex: &str) -> Result<(), Error> {
    let signature = parse_signature(signature)?;
    verify_bytes(message, &signature, public_key_hex)
}

/// The same check when the signature was already parsed. A keyring parses once
/// and tries every key, so a corrupt signature is `invalid`, not `denied`.
pub fn verify_bytes(
    message: &[u8],
    signature: &[u8; SIGNATURE_BYTES],
    public_key_hex: &str,
) -> Result<(), Error> {
    let key = verifying_key(public_key_hex)?;
    key.verify_strict(message, &Ed25519Signature::from_bytes(signature))
        .map_err(|_| Error::denied("the signature does not verify for this key"))
}

/// Whether the key can actually verify: it decompresses and is not low-order.
pub fn check_public_key(text: &str) -> Result<(), Error> {
    verifying_key(text).map(drop)
}

fn verifying_key(text: &str) -> Result<VerifyingKey, Error> {
    let key = VerifyingKey::from_bytes(&public_key(text)?).map_err(|error| {
        Error::invalid(format!("`{text}` is not an Ed25519 public key: {error}"))
    })?;
    if key.is_weak() {
        return Err(Error::invalid(format!(
            "`{text}` is a weak (small-order) public key"
        )));
    }
    Ok(key)
}

fn hex_decode(text: &str) -> Result<Vec<u8>, Error> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(Error::invalid("the key is not lowercase hex"));
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        bytes.push((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?);
    }
    Ok(bytes)
}

fn hex_digit(byte: u8) -> Result<u8, Error> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Error::invalid("the key is not lowercase hex")),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signing_key() -> SigningKey {
        // Deterministic key: no random source needed for the test.
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn public_hex() -> String {
        hex_encode(&signing_key().verifying_key().to_bytes())
    }

    #[test]
    fn a_valid_signature_verifies() {
        let message = b"anth-manifest-v2\0payload";
        let signature = signing_key().sign(message);
        let encoded = signature_base64(&signature.to_bytes());
        verify(message, &encoded, &public_hex()).expect("verify");
    }

    #[test]
    fn a_tampered_message_is_denied() {
        let signature = signing_key().sign(b"the real message");
        let encoded = signature_base64(&signature.to_bytes());
        let error = verify(b"another message", &encoded, &public_hex()).expect_err("tampered");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn a_signature_from_another_key_is_denied() {
        let message = b"anth-manifest-v2\0payload";
        let signature = SigningKey::from_bytes(&[9u8; 32]).sign(message);
        let encoded = signature_base64(&signature.to_bytes());
        let error = verify(message, &encoded, &public_hex()).expect_err("wrong key");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn a_signature_that_is_not_base64_is_invalid() {
        let error = verify(b"x", "not base64!", &public_hex()).expect_err("base64");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }

    #[test]
    fn a_signature_of_the_wrong_length_is_invalid() {
        let encoded = STANDARD.encode([0u8; 63]);
        assert!(parse_signature(&encoded).is_err());
    }

    #[test]
    fn a_key_that_is_not_hex_is_invalid() {
        for key in ["zz", "ABC", &public_hex().to_uppercase()] {
            assert!(public_key(key).is_err(), "`{key}` must be refused");
        }
    }

    #[test]
    fn the_fingerprint_is_sha256_of_the_key() {
        assert_eq!(
            fingerprint(&public_hex()).expect("fingerprint"),
            "fe812c12f3ab4ce6ac5db69ac352f906cb1b11ef43fb33e252ef7ff552263889",
            "the fingerprint of the verifying key for seed [7; 32]"
        );
    }

    #[test]
    fn a_small_order_key_is_refused() {
        // The identity point and the order-4 point: both decompress, neither is
        // a legitimate signer. `verify_strict` denies them; the explicit check
        // gives the operator the reason instead of a generic failure.
        for key in [
            "0100000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            let signature = STANDARD.encode([0u8; SIGNATURE_BYTES]);
            let error = verify(b"x", &signature, key).expect_err("weak key");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
            assert!(error.message().contains("weak"), "{error}");
        }
    }

    #[test]
    fn a_signature_round_trips_through_base64() {
        let signature = signing_key().sign(b"x").to_bytes();
        let text = signature_base64(&signature);
        assert_eq!(parse_signature(&text).expect("parse"), signature);
        assert!(!text.contains('\n'), "manifest.sig is one line");
    }
}
