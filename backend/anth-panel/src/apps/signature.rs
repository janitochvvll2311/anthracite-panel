//! Ed25519 verification for app manifests.
//!
//! The author signs the canonical manifest-set message: every `manifest.toml`
//! present, in fixed order with explicit lengths. The manifest in turn names the
//! sha256 of the module and of every UI asset, so one signature approves the
//! whole bundle.
//!
//! The host stores only `[apps] public_key`. A signature that is present is
//! always checked, even when `require_signature = false`; the switch allows an
//! unsigned bundle, not a signature that does not match.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

pub const PUBLIC_KEY_BYTES: usize = 32;
pub const SIGNATURE_BYTES: usize = 64;

/// Confirms that `signature_hex` is an Ed25519 signature of `message` under
/// `public_key_hex`. Both keys use lowercase or uppercase hex.
pub fn verify(public_key_hex: &str, signature_hex: &str, message: &[u8]) -> Result<(), String> {
    let key_bytes: [u8; PUBLIC_KEY_BYTES] = decode_hex(public_key_hex.trim())
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            format!(
                "`apps.public_key` must be {} hex characters",
                PUBLIC_KEY_BYTES * 2
            )
        })?;
    let signature_bytes: [u8; SIGNATURE_BYTES] = decode_hex(signature_hex.trim())
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            format!(
                "`manifest.sig` must be {} hex characters",
                SIGNATURE_BYTES * 2
            )
        })?;

    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| "`apps.public_key` is not a valid Ed25519 public key".to_string())?;
    let signature = Signature::from_bytes(&signature_bytes);
    key.verify(message, &signature)
        .map_err(|_| "the bundle signature does not match the manifest set".to_string())
}

pub fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect()
}

pub fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn a_signature_over_the_manifest_verifies() {
        let key = key();
        let message = b"manifest_version = 1\n";
        let signature = key.sign(message);
        verify(
            &encode_hex(key.verifying_key().as_bytes()),
            &encode_hex(&signature.to_bytes()),
            message,
        )
        .expect("signature");
    }

    #[test]
    fn a_changed_manifest_is_refused() {
        let key = key();
        let signature = key.sign(b"one");
        let error = verify(
            &encode_hex(key.verifying_key().as_bytes()),
            &encode_hex(&signature.to_bytes()),
            b"two",
        )
        .expect_err("mismatch");
        assert!(error.contains("does not match"), "{error}");
    }

    #[test]
    fn a_malformed_key_is_reported_before_verification() {
        let error = verify("nope", "00", b"x").expect_err("key");
        assert!(error.contains("public_key"), "{error}");
    }

    #[test]
    fn hex_must_be_even_and_valid() {
        assert_eq!(decode_hex("abcd"), Some(vec![0xab, 0xcd]));
        assert_eq!(decode_hex("abc"), None);
        assert_eq!(decode_hex("zz"), None);
        assert_eq!(decode_hex(""), None);
    }
}
