//! Ed25519 verification for plugin bundles.
//!
//! A bundle is signed by its author with a private key; the host only stores the
//! public key in `[plugins] public_key`. The signature covers the raw bytes of
//! `plugin.wasm`, which is the code that actually runs. The manifest's own
//! `sha256` is still checked, but it is not the trust anchor: an attacker who
//! rewrites both files can keep the pair self-consistent, while a signature
//! cannot be recomputed without the private key.
//!
//! The signature is a detached `plugin.sig` file holding 128 hex characters.
//! Keeping it out of the manifest means packaging signs the module it built,
//! not a manifest that had to be edited first.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Length of an Ed25519 public key, in bytes.
pub const PUBLIC_KEY_BYTES: usize = 32;

/// Length of an Ed25519 signature, in bytes.
pub const SIGNATURE_BYTES: usize = 64;

/// Confirms that `signature_hex` is a valid Ed25519 signature of `message`
/// under `public_key_hex`.
///
/// Both keys use the hex spelling the rest of the plugin config already uses
/// for digests. A malformed key or signature is reported separately from a
/// signature that simply does not match, because they mean different things to
/// the operator: one is a configuration or packaging mistake, the other is a
/// bundle that must not be installed.
pub fn verify(public_key_hex: &str, signature_hex: &str, message: &[u8]) -> Result<(), String> {
    let key_bytes: [u8; PUBLIC_KEY_BYTES] = decode_hex(public_key_hex.trim())
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            format!(
                "`plugins.public_key` must be {} hex characters",
                PUBLIC_KEY_BYTES * 2
            )
        })?;
    let signature_bytes: [u8; SIGNATURE_BYTES] = decode_hex(signature_hex.trim())
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| {
            format!(
                "`plugin.sig` must be {} hex characters",
                SIGNATURE_BYTES * 2
            )
        })?;

    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| "`plugins.public_key` is not a valid Ed25519 public key".to_string())?;
    let signature = Signature::from_bytes(&signature_bytes);
    key.verify(message, &signature)
        .map_err(|_| "the bundle signature does not match `plugin.wasm`".to_string())
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn a_signature_over_the_module_verifies() {
        let key = key();
        let message = b"\0asm\x01\0\0\0";
        let signature = key.sign(message);
        verify(
            &hex(key.verifying_key().as_bytes()),
            &hex(&signature.to_bytes()),
            message,
        )
        .unwrap();
    }

    #[test]
    fn a_changed_module_is_refused() {
        let key = key();
        let signature = key.sign(b"one");
        let error = verify(
            &hex(key.verifying_key().as_bytes()),
            &hex(&signature.to_bytes()),
            b"two",
        )
        .unwrap_err();
        assert!(error.contains("does not match"), "{error}");
    }

    #[test]
    fn a_malformed_key_is_reported_before_verification() {
        let error = verify("nope", "00", b"x").unwrap_err();
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
