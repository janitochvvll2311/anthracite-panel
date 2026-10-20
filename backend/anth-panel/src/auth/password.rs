//! Password hashing with PBKDF2-HMAC-SHA256.
//!
//! The hash is stored as `pbkdf2-sha256$<iterations>$<salt>$<digest>`, all
//! base64url without padding. The parameters travel with the digest so a later
//! build can raise the iteration count without invalidating existing hashes.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Chosen to stay near 200 ms on a small VPS while remaining far above the
/// minimum OWASP recommends for PBKDF2-HMAC-SHA256.
///
/// The count is only a default: `[auth] iterations` overrides it, and a debug
/// build drops to [`DEV_ITERATIONS`] because an unoptimised PBKDF2 is roughly
/// thirty times slower and would make every login wait for several seconds.
pub const DEFAULT_ITERATIONS: u32 = 210_000;

/// Iteration count for `[auth]` in a debug build.
pub const DEV_ITERATIONS: u32 = 1_000;

/// The cost a new config records: the real one in release, the cheap one in a
/// debug build, where an unoptimised PBKDF2 would make every login wait.
pub fn default_iterations() -> u32 {
    if cfg!(debug_assertions) {
        DEV_ITERATIONS
    } else {
        DEFAULT_ITERATIONS
    }
}
const SALT_LEN: usize = 16;
const DIGEST_LEN: usize = 32;

/// Hashes `password` with a fresh random salt and the default cost.
pub fn hash_password(password: &str) -> Result<String, String> {
    hash_password_with_iterations(password, DEFAULT_ITERATIONS)
}

/// Hashes `password` with a fresh random salt and an explicit cost.
pub fn hash_password_with_iterations(password: &str, iterations: u32) -> Result<String, String> {
    let mut salt = [0u8; SALT_LEN];
    rand::fill(&mut salt);
    hash_with_salt(password, &salt, iterations)
}

/// Hashes `password` with an explicit salt and iteration count, as an encoded
/// string. Separate from [`hash_password`] so tests are deterministic.
pub fn hash_with_salt(password: &str, salt: &[u8], iterations: u32) -> Result<String, String> {
    if iterations == 0 {
        return Err("PBKDF2 iteration count must be positive".to_string());
    }
    if salt.is_empty() {
        return Err("PBKDF2 salt must not be empty".to_string());
    }

    let digest = pbkdf2(password.as_bytes(), salt, iterations, DIGEST_LEN);
    Ok(format!(
        "pbkdf2-sha256${iterations}${}${}",
        URL_SAFE_NO_PAD.encode(salt),
        URL_SAFE_NO_PAD.encode(digest)
    ))
}

/// Checks `password` against an encoded hash.
///
/// A malformed or empty hash is a mismatch, never an error: a corrupt config
/// must lock the panel rather than open it.
pub fn verify_password(password: &str, encoded: &str) -> bool {
    let Some((algorithm, iterations, salt, digest)) = split_encoded(encoded) else {
        return false;
    };
    if algorithm != "pbkdf2-sha256" || iterations == 0 {
        return false;
    }
    let Ok(salt) = URL_SAFE_NO_PAD.decode(salt) else {
        return false;
    };
    let Ok(expected) = URL_SAFE_NO_PAD.decode(digest) else {
        return false;
    };
    if salt.is_empty() || expected.is_empty() {
        return false;
    }

    let actual = pbkdf2(password.as_bytes(), &salt, iterations, expected.len());
    // Constant-time compare: a length mismatch already leaks nothing useful,
    // but the loop must not stop at the first differing byte.
    if actual.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in actual.iter().zip(expected.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn split_encoded(encoded: &str) -> Option<(&str, u32, &str, &str)> {
    let mut parts = encoded.split('$');
    let algorithm = parts.next()?;
    let iterations = parts.next()?.parse().ok()?;
    let salt = parts.next()?;
    let digest = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((algorithm, iterations, salt, digest))
}

/// PBKDF2 with HMAC-SHA256, one 32-byte block wide.
fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32, length: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(length);
    let mut block: u32 = 1;

    while output.len() < length {
        let mut mac = HmacSha256::new_from_slice(password).expect("HMAC accepts any key length");
        mac.update(salt);
        mac.update(&block.to_be_bytes());
        let mut current = mac.finalize().into_bytes();

        let mut accumulated = current;
        for _ in 1..iterations {
            let mut mac =
                HmacSha256::new_from_slice(password).expect("HMAC accepts any key length");
            mac.update(&current);
            current = mac.finalize().into_bytes();
            for (target, byte) in accumulated.iter_mut().zip(current.iter()) {
                *target ^= byte;
            }
        }

        output.extend_from_slice(&accumulated);
        block += 1;
    }

    output.truncate(length);
    output
}

/// A random password that is easy to read aloud: no `0`/`O` or `1`/`l`.
pub fn generate_password(length: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    (0..length)
        .map(|_| {
            let index = rand::random_range(0..ALPHABET.len());
            ALPHABET[index] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_verifies_its_own_password() {
        let encoded =
            hash_password_with_iterations("correct horse battery staple", DEV_ITERATIONS).unwrap();
        assert!(verify_password("correct horse battery staple", &encoded));
        assert!(!verify_password("correct horse battery stapl", &encoded));
    }

    #[test]
    fn the_same_password_hashes_differently_each_time() {
        let first = hash_password_with_iterations("repeatable", DEV_ITERATIONS).unwrap();
        let second = hash_password_with_iterations("repeatable", DEV_ITERATIONS).unwrap();
        assert_ne!(first, second, "a re-used salt would leak equality");
    }

    #[test]
    fn a_known_vector_matches_rfc_6070() {
        // PBKDF2-HMAC-SHA256, "password", "salt", 1 iteration.
        let digest = pbkdf2(b"password", b"salt", 1, 32);
        assert_eq!(
            URL_SAFE_NO_PAD.encode(digest),
            "Eg-2z_z4syxD5yJSVsT4N6hlSMkszDVICAWYfLcL4Xs"
        );
    }
}
