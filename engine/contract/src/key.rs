//! A trusted third-party signer (§9.1): one document per key under
//! `state/keys/<fingerprint>.toml`.
//!
//! The fingerprint is `sha256(public key)` and names the file; the document
//! carries the key and who trusted it. v0.5.0 has no distribution key, so every
//! signed bundle is `third-party` and this is the whole trust model
//! (D-Distribution-Key).

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::id::is_sha256;

pub const KEY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyAlgorithm {
    Ed25519,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedKey {
    pub version: u32,
    pub algorithm: KeyAlgorithm,
    /// 32 bytes, lowercase hex. Its sha256 is the fingerprint.
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The operator who added the key.
    pub added_by: String,
    /// RFC 3339, UTC.
    pub added_at: String,
}

impl TrustedKey {
    #[cfg(feature = "parse")]
    pub fn parse(text: &str) -> Result<Self, Error> {
        toml::from_str(text)
            .map_err(|error| Error::invalid(format!("the key is not valid TOML: {error}")))
    }

    #[cfg(feature = "parse")]
    pub fn to_toml(&self) -> Result<String, Error> {
        toml::to_string_pretty(self)
            .map_err(|error| Error::internal(format!("rendering the key: {error}")))
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.version != KEY_VERSION {
            return Err(Error::invalid(format!(
                "key version {} is not supported; this engine speaks {KEY_VERSION}",
                self.version
            )));
        }
        if !is_sha256(&self.public_key) {
            return Err(Error::invalid(
                "`public_key` is not 64 lowercase hex; an Ed25519 key is 32 bytes",
            ));
        }
        if let Some(label) = &self.label {
            if label.trim().is_empty() {
                return Err(Error::invalid("`label` cannot be empty when present"));
            }
        }
        if self.added_by.trim().is_empty() {
            return Err(Error::invalid("`added_by` cannot be empty"));
        }
        humantime::parse_rfc3339(&self.added_at)
            .map_err(|error| Error::invalid(format!("`added_at` is not RFC 3339: {error}")))?;
        Ok(())
    }
}

#[cfg(all(test, feature = "parse"))]
mod tests {
    use super::*;

    fn sample() -> TrustedKey {
        TrustedKey {
            version: KEY_VERSION,
            algorithm: KeyAlgorithm::Ed25519,
            public_key: "a".repeat(64),
            label: Some("example signer".to_string()),
            added_by: "operator-3f2a".to_string(),
            added_at: "2026-11-03T20:00:00Z".to_string(),
        }
    }

    #[test]
    fn a_trusted_key_validates() {
        sample().validate().expect("valid");
    }

    #[test]
    fn the_algorithm_is_pinned() {
        let text = sample()
            .to_toml()
            .expect("render")
            .replace("ed25519", "rsa");
        assert!(TrustedKey::parse(&text).is_err());
    }

    #[test]
    fn a_key_that_is_not_32_bytes_of_hex_is_refused() {
        for public_key in ["a".repeat(63), "A".repeat(64), "zz".repeat(32)] {
            let mut key = sample();
            key.public_key = public_key;
            assert!(key.validate().is_err());
        }
    }

    #[test]
    fn an_empty_approver_or_label_is_refused() {
        let mut key = sample();
        key.added_by = "  ".to_string();
        assert!(key.validate().is_err());

        let mut key = sample();
        key.label = Some("  ".to_string());
        assert!(key.validate().is_err());
    }

    #[test]
    fn a_bad_timestamp_is_refused() {
        let mut key = sample();
        key.added_at = "yesterday".to_string();
        assert!(key.validate().is_err());
    }

    #[test]
    fn a_bad_version_is_refused() {
        let mut key = sample();
        key.version = 2;
        assert!(key.validate().is_err());
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let text = sample().to_toml().expect("render");
        let text = text.replace("added_by", "added-by");
        assert!(TrustedKey::parse(&text).is_err());
    }

    #[test]
    fn the_document_round_trips() {
        let key = sample();
        assert_eq!(
            TrustedKey::parse(&key.to_toml().expect("render")).expect("parse"),
            key
        );
    }
}
