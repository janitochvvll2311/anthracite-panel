//! Minimal HS256 JSON Web Tokens.
//!
//! Only HS256 is accepted. `alg: none`, RS256 and every other algorithm are
//! rejected by the header check rather than by trusting the caller.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// The claims this panel issues.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
    pub sid: String,
    pub kind: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JwtError {
    Malformed,
    UnsupportedAlgorithm,
    BadSignature,
    Expired,
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            JwtError::Malformed => "malformed token",
            JwtError::UnsupportedAlgorithm => "unsupported signing algorithm",
            JwtError::BadSignature => "signature does not match",
            JwtError::Expired => "token expired",
        })
    }
}

impl std::error::Error for JwtError {}

/// Seconds since the Unix epoch.
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

/// Signs claims with `key` and returns the compact token.
pub fn sign(claims: &Claims, key: &[u8]) -> Result<String, String> {
    let header = Header {
        alg: "HS256".to_string(),
        typ: "JWT".to_string(),
    };
    let header = serde_json::to_vec(&header).map_err(|error| format!("header: {error}"))?;
    let payload = serde_json::to_vec(claims).map_err(|error| format!("claims: {error}"))?;
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

/// Verifies a token against `key` and returns its claims.
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
    if claims.sub.is_empty() || claims.sid.is_empty() || claims.kind.is_empty() {
        return Err(JwtError::Malformed);
    }
    if now >= claims.exp {
        return Err(JwtError::Expired);
    }
    Ok(claims)
}

fn signature_for(input: &[u8], key: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(input);
    mac.finalize().into_bytes().to_vec()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(now: u64) -> Claims {
        Claims {
            sub: "admin".to_string(),
            iss: "anth-panel".to_string(),
            iat: now,
            exp: now + 60,
            sid: "session".to_string(),
            kind: "access".to_string(),
        }
    }

    #[test]
    fn a_token_round_trips() {
        let key = b"secret";
        let token = sign(&claims(100), key).expect("sign");
        let parsed = verify(&token, key, 100).expect("verify");
        assert_eq!(parsed.sub, "admin");
    }

    #[test]
    fn a_wrong_key_or_expired_token_is_refused() {
        let token = sign(&claims(100), b"secret").expect("sign");
        assert_eq!(
            verify(&token, b"other", 100).expect_err("wrong key"),
            JwtError::BadSignature
        );
        assert_eq!(
            verify(&token, b"secret", 200).expect_err("expired"),
            JwtError::Expired
        );
    }

    #[test]
    fn alg_none_is_refused() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD
            .encode(br#"{"sub":"admin","iss":"x","iat":0,"exp":9,"sid":"s","kind":"access"}"#);
        let token = format!("{header}.{payload}.");
        assert_eq!(
            verify(&token, b"secret", 0).expect_err("alg"),
            JwtError::UnsupportedAlgorithm
        );
    }
}
