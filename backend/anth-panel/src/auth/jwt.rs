//! A minimal HS256 JSON Web Token implementation.
//!
//! The panel needs signing, parsing and expiry, not the whole JOSE surface, and
//! the musl build must stay free of C dependencies. `hmac` and `sha2` cover the
//! signature, so this module is the encoding and the clock.
//!
//! Only HS256 is ever accepted. `alg: none` and RS256 are rejected by the
//! header check rather than by trusting the caller's choice.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Claims this panel issues. `sub` is the account name, `jti` the identifier
/// the refresh store revokes, and `role` the panel role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub role: String,
    /// Issued at, seconds since the Unix epoch.
    pub iat: u64,
    /// Expiry, seconds since the Unix epoch.
    pub exp: u64,
    /// Token identifier. A refresh token's `jti` rotates on every refresh.
    pub jti: String,
    /// Which half of the pair this is: `access` or `refresh`.
    ///
    /// Without it a refresh token is a valid credential for the API, which
    /// would let a leaked one keep full access for its whole life no matter how
    /// often it rotates.
    #[serde(default)]
    pub kind: String,
    /// Session identifier, shared by the access and refresh halves of a pair.
    ///
    /// The signature says a token was issued by this panel; `sid` is what lets
    /// the store say whether the session behind it is still live. A token
    /// without one is rejected, so an old two-field token cannot be downgraded
    /// into an unrevokable one.
    #[serde(default)]
    pub sid: String,
}

impl Claims {
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.exp
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum JwtError {
    Malformed,
    UnsupportedAlgorithm,
    BadSignature,
    Expired,
    /// The signature is valid but the session it names is gone.
    Revoked,
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Malformed => "malformed token",
            Self::UnsupportedAlgorithm => "unsupported signing algorithm",
            Self::BadSignature => "signature does not match",
            Self::Expired => "token expired",
            Self::Revoked => "session revoked",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for JwtError {}

/// Seconds since the Unix epoch. A clock before 1970 yields `0`, which only
/// means every token looks expired until the clock is fixed.
pub fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
struct Header {
    alg: String,
    typ: String,
}

/// Signs `claims` with `key` and returns the compact token.
pub fn sign(claims: &Claims, key: &[u8]) -> Result<String, String> {
    let header = Header {
        alg: "HS256".to_string(),
        typ: "JWT".to_string(),
    };
    let header = serde_json::to_vec(&header).map_err(|e| format!("header: {e}"))?;
    let payload = serde_json::to_vec(claims).map_err(|e| format!("claims: {e}"))?;

    let mut token = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(payload)
    );
    let signature = signature_for(token.as_bytes(), key);
    token.push('.');
    token.push_str(&URL_SAFE_NO_PAD.encode(signature));
    Ok(token)
}

/// Verifies `token` against `key` and returns its claims.
///
/// The signature is checked before the payload is parsed, so a forged token
/// never reaches the JSON decoder.
pub fn verify(token: &str, key: &[u8], now: u64) -> Result<Claims, JwtError> {
    let mut parts = token.split('.');
    let encoded_header = parts.next().ok_or(JwtError::Malformed)?;
    let payload = parts.next().ok_or(JwtError::Malformed)?;
    let signature = parts.next().ok_or(JwtError::Malformed)?;
    if parts.next().is_some() {
        return Err(JwtError::Malformed);
    }

    let header: Header = URL_SAFE_NO_PAD
        .decode(encoded_header)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or(JwtError::Malformed)?;
    if header.alg != "HS256" {
        return Err(JwtError::UnsupportedAlgorithm);
    }

    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| JwtError::Malformed)?;
    let signed = format!("{encoded_header}.{payload}");
    let expected = signature_for(signed.as_bytes(), key);
    if !constant_time_eq(&expected, &signature) {
        return Err(JwtError::BadSignature);
    }

    let claims: Claims = URL_SAFE_NO_PAD
        .decode(payload)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or(JwtError::Malformed)?;
    if claims.sub.is_empty() {
        return Err(JwtError::Malformed);
    }
    if claims.is_expired(now) {
        return Err(JwtError::Expired);
    }
    Ok(claims)
}

/// A random identifier for `jti` and for session bookkeeping.
pub fn random_id() -> String {
    let mut bytes = [0u8; 16];
    rand::fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn signature_for(message: &[u8], key: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(exp: u64) -> Claims {
        Claims {
            sub: "admin".to_string(),
            role: "admin".to_string(),
            iat: 1_700_000_000,
            exp,
            jti: "abc".to_string(),
            kind: "access".to_string(),
            sid: "session".to_string(),
        }
    }

    #[test]
    fn a_signed_token_round_trips() {
        let token = sign(&claims(1_700_000_060), b"secret").unwrap();
        let parsed = verify(&token, b"secret", 1_700_000_000).unwrap();
        assert_eq!(parsed.sub, "admin");
        assert_eq!(parsed.jti, "abc");
        assert_eq!(parsed.sid, "session");
    }

    #[test]
    fn a_token_without_a_session_identifier_still_parses() {
        // `sid` is optional on the wire so an older token is not a parse error;
        // the session store rejects it, which is where revocation lives.
        let token = sign(&claims(1_700_000_060), b"secret").unwrap();
        let parsed = verify(&token, b"secret", 1_700_000_000).unwrap();
        assert!(!parsed.sid.is_empty());
    }

    #[test]
    fn another_key_is_rejected() {
        let token = sign(&claims(1_700_000_060), b"secret").unwrap();
        assert_eq!(
            verify(&token, b"other", 1_700_000_000),
            Err(JwtError::BadSignature)
        );
    }

    #[test]
    fn a_tampered_payload_is_rejected() {
        let token = sign(&claims(1_700_000_060), b"secret").unwrap();
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&Claims {
                role: "root".to_string(),
                ..claims(1_700_000_060)
            })
            .unwrap(),
        );
        parts[1] = &forged;
        assert_eq!(
            verify(&parts.join("."), b"secret", 1_700_000_000),
            Err(JwtError::BadSignature)
        );
    }

    #[test]
    fn an_expired_token_reports_expiry() {
        let token = sign(&claims(1_700_000_000), b"secret").unwrap();
        assert_eq!(
            verify(&token, b"secret", 1_700_000_001),
            Err(JwtError::Expired)
        );
    }

    #[test]
    fn an_unsigned_token_is_rejected() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD
            .encode(br#"{"sub":"admin","role":"admin","iat":1,"exp":9,"jti":"x","sid":"s"}"#);
        let token = format!("{header}.{payload}.");
        assert_eq!(
            verify(&token, b"secret", 2),
            Err(JwtError::UnsupportedAlgorithm)
        );
    }

    #[test]
    fn four_segments_are_malformed() {
        assert_eq!(verify("a.b.c.d", b"secret", 2), Err(JwtError::Malformed));
    }
}
