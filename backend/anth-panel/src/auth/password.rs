//! Password hashing with Argon2id.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

/// The shortest password accepted. Length is the only composition rule.
pub const MIN_PASSWORD_CHARS: usize = 12;

/// The longest password accepted, so a hash cannot become a denial of service.
pub const MAX_PASSWORD_BYTES: usize = 1024;

/// Hashes a password with a fresh salt and the default Argon2id parameters.
pub fn hash(password: &str) -> Result<String, String> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(format!(
            "the password must be at least {MIN_PASSWORD_CHARS} characters"
        ));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(format!(
            "the password must be at most {MAX_PASSWORD_BYTES} bytes"
        ));
    }

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("hashing the password: {error}"))
}

/// Checks a password against a PHC string.
///
/// A malformed or empty hash is a mismatch, never an error: a corrupt secrets
/// file must lock the panel rather than open it.
pub fn verify(password: &str, encoded: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(encoded) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_password_is_refused() {
        assert!(hash("short").is_err());
    }

    #[test]
    fn a_hash_verifies_and_a_wrong_password_does_not() {
        let encoded = hash("a sufficiently long password").expect("hash");
        assert!(verify("a sufficiently long password", &encoded));
        assert!(!verify("the wrong password", &encoded));
        assert!(!verify("anything", "not a phc string"));
    }
}
