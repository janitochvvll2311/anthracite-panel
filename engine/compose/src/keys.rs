//! The keyring: trusted third-party signers under `state/keys` (§9.1).
//!
//! v0.5.0 has no distribution key (D-Distribution-Key), so this is the whole
//! trust model: a signed bundle is accepted when one of these keys signs the
//! canonical message. The keyring can extend trust, never privilege.

use std::path::{Path, PathBuf};

use anth_contract::{Error, TrustedKey};

use crate::{signature, store};

#[derive(Debug, Clone)]
pub struct Keyring {
    dir: PathBuf,
}

impl Keyring {
    pub fn new(state: &Path) -> Self {
        Self {
            dir: state.join("keys"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Trusts `key` and returns its fingerprint. Adding the same key twice is
    /// idempotent: the file is the fingerprint.
    pub fn add(&self, key: &TrustedKey) -> Result<String, Error> {
        key.validate()?;
        let fingerprint = signature::fingerprint(&key.public_key)?;
        store::write_atomic(
            &self.dir,
            &format!("{fingerprint}.toml"),
            key.to_toml()?.as_bytes(),
        )?;
        Ok(fingerprint)
    }

    pub fn load(&self, fingerprint: &str) -> Result<Option<TrustedKey>, Error> {
        check_fingerprint(fingerprint)?;
        if !store::check_dir(&self.dir)? {
            return Ok(None);
        }
        let path = self.dir.join(format!("{fingerprint}.toml"));
        let Some(text) = store::read(&path)? else {
            return Ok(None);
        };
        let key = TrustedKey::parse(&text)
            .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
        key.validate()
            .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
        let actual = signature::fingerprint(&key.public_key)?;
        if actual != fingerprint {
            return Err(Error::invalid(format!(
                "{} holds a key for fingerprint `{actual}`, not `{fingerprint}`",
                path.display()
            )));
        }
        Ok(Some(key))
    }

    pub fn remove(&self, fingerprint: &str) -> Result<bool, Error> {
        check_fingerprint(fingerprint)?;
        if !store::check_dir(&self.dir)? {
            return Ok(false);
        }
        store::remove(&self.dir.join(format!("{fingerprint}.toml")))
    }

    /// Every trusted key with its fingerprint, sorted by fingerprint.
    pub fn all(&self) -> Result<Vec<(String, TrustedKey)>, Error> {
        let mut keys = Vec::new();
        for path in store::toml_paths(&self.dir)? {
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            check_fingerprint(stem)?;
            if let Some(key) = self.load(stem)? {
                keys.push((stem.to_string(), key));
            }
        }
        Ok(keys)
    }

    /// Verifies `signature` over `message` against every trusted key and returns
    /// the fingerprint that signed.
    ///
    /// `manifest.sig` carries only the signature, not the signer: with a small
    /// keyring, trying each key is simpler than inventing a second artifact and
    /// keeps the bundle to one signature file.
    pub fn verify(&self, message: &[u8], signature_b64: &str) -> Result<String, Error> {
        for (fingerprint, key) in self.all()? {
            if signature::verify(message, signature_b64, &key.public_key).is_ok() {
                return Ok(fingerprint);
            }
        }
        Err(Error::denied("no trusted key signs this bundle"))
    }
}

fn check_fingerprint(fingerprint: &str) -> Result<(), Error> {
    if signature::is_fingerprint(fingerprint) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "`{fingerprint}` is not a fingerprint; it is 64 lowercase hex"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{KeyAlgorithm, KEY_VERSION};
    use ed25519_dalek::{Signer, SigningKey};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-keys-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn public_hex() -> String {
        signing_key()
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn trusted_key() -> TrustedKey {
        TrustedKey {
            version: KEY_VERSION,
            algorithm: KeyAlgorithm::Ed25519,
            public_key: public_hex(),
            label: Some("test signer".to_string()),
            added_by: "operator-3f2a".to_string(),
            added_at: "2026-11-03T20:00:00Z".to_string(),
        }
    }

    #[test]
    fn a_key_round_trips_through_the_keyring() {
        let state = scratch("round-trip");
        let keyring = Keyring::new(&state);
        let fingerprint = keyring.add(&trusted_key()).expect("add");
        assert_eq!(
            keyring.load(&fingerprint).expect("load"),
            Some(trusted_key())
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_file_is_named_after_the_fingerprint() {
        let state = scratch("name");
        let keyring = Keyring::new(&state);
        let fingerprint = keyring.add(&trusted_key()).expect("add");
        assert_eq!(fingerprint.len(), 64);
        assert!(keyring.dir().join(format!("{fingerprint}.toml")).is_file());
        let mode = fs::metadata(keyring.dir().join(format!("{fingerprint}.toml")))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn adding_the_same_key_twice_is_idempotent() {
        let state = scratch("twice");
        let keyring = Keyring::new(&state);
        let first = keyring.add(&trusted_key()).expect("add");
        let second = keyring.add(&trusted_key()).expect("add");
        assert_eq!(first, second);
        assert_eq!(keyring.all().expect("all").len(), 1);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn all_returns_every_key_in_order() {
        let state = scratch("all");
        let keyring = Keyring::new(&state);
        let one = keyring.add(&trusted_key()).expect("add");
        let mut other = signing_key().verifying_key().to_bytes();
        other[0] ^= 0xff;
        let other_hex: String = other.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut other_key = trusted_key();
        other_key.public_key = other_hex;
        let two = keyring.add(&other_key).expect("add");
        let fingerprints: Vec<String> = keyring
            .all()
            .expect("all")
            .into_iter()
            .map(|(fingerprint, _)| fingerprint)
            .collect();
        let mut expected = vec![one, two];
        expected.sort();
        assert_eq!(fingerprints, expected);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn remove_reports_whether_there_was_a_key() {
        let state = scratch("remove");
        let keyring = Keyring::new(&state);
        let fingerprint = keyring.add(&trusted_key()).expect("add");
        assert!(keyring.remove(&fingerprint).expect("remove"));
        assert!(!keyring.remove(&fingerprint).expect("nothing"));
        assert_eq!(keyring.load(&fingerprint).expect("load"), None);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_key_file_under_another_fingerprint_is_refused() {
        let state = scratch("mismatch");
        let keyring = Keyring::new(&state);
        let fingerprint = keyring.add(&trusted_key()).expect("add");
        let other = "b".repeat(64);
        fs::copy(
            keyring.dir().join(format!("{fingerprint}.toml")),
            keyring.dir().join(format!("{other}.toml")),
        )
        .expect("copy");
        let error = keyring.load(&other).expect_err("mismatch");
        assert!(error.message().contains("fingerprint"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_path_that_is_not_a_fingerprint_is_refused() {
        let state = scratch("traversal");
        let keyring = Keyring::new(&state);
        for fingerprint in ["../keys/x", "ABCD", &"a".repeat(63)] {
            assert!(keyring.load(fingerprint).is_err(), "`{fingerprint}`");
            assert!(keyring.remove(fingerprint).is_err(), "`{fingerprint}`");
        }
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_trusted_signer_verifies() {
        let state = scratch("verify");
        let keyring = Keyring::new(&state);
        let fingerprint = keyring.add(&trusted_key()).expect("add");
        let message = b"anth-manifest-v2\0payload";
        let signature = signing_key().sign(message);
        let encoded = signature::signature_base64(&signature.to_bytes());
        assert_eq!(
            keyring.verify(message, &encoded).expect("verify"),
            fingerprint
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_untrusted_signer_is_denied() {
        let state = scratch("untrusted");
        let keyring = Keyring::new(&state);
        // A key exists, but it is not the one that signed.
        let mut other = trusted_key();
        other.public_key = "0".repeat(64);
        keyring.add(&other).expect("add");
        let signature = signing_key().sign(b"x");
        let encoded = signature::signature_base64(&signature.to_bytes());
        let error = keyring.verify(b"x", &encoded).expect_err("untrusted");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_empty_keyring_denies_everything() {
        let state = scratch("empty");
        let keyring = Keyring::new(&state);
        let signature = signing_key().sign(b"x");
        let encoded = signature::signature_base64(&signature.to_bytes());
        assert!(keyring.verify(b"x", &encoded).is_err());
        fs::remove_dir_all(&state).expect("cleanup");
    }
}
