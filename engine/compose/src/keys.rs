//! The keyring: trusted third-party signers under `state/keys` (§9.1), and the
//! distribution key compiled into the binary.
//!
//! The keyring can extend trust, never privilege: anything it trusts is
//! `third-party`. The `system` tier comes from the distribution key, whose
//! public half is a build input and whose private half never enters the build.

use std::path::{Path, PathBuf};

use anth_contract::id::is_sha256;
use anth_contract::{Error, ErrorKind, Tier, TrustedKey};

use crate::{signature, store};

/// The distribution key of this build (§9.1, D-Distribution-Key).
///
/// A release build compiles the public key in
/// (`ANTHRACITE_DISTRIBUTION_KEY`, 32 bytes of lowercase hex); a debug build
/// carries none and can install nothing as `system`. A bundle this key signs is
/// `system` and skips the third-party service switch.
#[derive(Debug, Clone, Default)]
pub struct Distribution {
    public_key: Option<String>,
    fingerprint: Option<String>,
    problem: Option<String>,
}

impl Distribution {
    /// No key: every signed bundle is `third-party`.
    pub fn none() -> Self {
        Self::default()
    }

    /// The key this binary was built with. Read at compile time, so a release
    /// binary carries it and no configuration file can replace it.
    pub fn embedded() -> Self {
        Self::from_public_key(option_env!("ANTHRACITE_DISTRIBUTION_KEY"))
    }

    /// Parses a public key. A key that is present but malformed is a build
    /// mistake, not an absent key: `check` refuses installs instead of silently
    /// demoting system bundles to third-party.
    pub fn from_public_key(public_key: Option<&str>) -> Self {
        let Some(text) = public_key.map(str::trim).filter(|text| !text.is_empty()) else {
            return Self::default();
        };
        let text = text.to_ascii_lowercase();
        // A fingerprint only proves the bytes are 32 hex bytes; a key that does
        // not decompress would report itself healthy and then fail every
        // verification, so the key is checked as an Ed25519 point as well.
        match signature::check_public_key(&text).and_then(|()| signature::fingerprint(&text)) {
            Ok(fingerprint) => Self {
                public_key: Some(text),
                fingerprint: Some(fingerprint),
                problem: None,
            },
            Err(error) => Self {
                public_key: None,
                fingerprint: None,
                problem: Some(error.message().to_string()),
            },
        }
    }

    pub fn public_key(&self) -> Option<&str> {
        self.public_key.as_deref()
    }

    pub fn fingerprint(&self) -> Option<&str> {
        self.fingerprint.as_deref()
    }

    /// The tier a signer earns. Only the distribution fingerprint is `system`.
    pub fn tier(&self, signer: Option<&str>) -> Tier {
        match (self.fingerprint(), signer) {
            (Some(key), Some(signer)) if key == signer => Tier::System,
            _ => Tier::ThirdParty,
        }
    }

    /// Verifies `signature` over `message` with the compiled key.
    ///
    /// `Ok(None)` means "not this key", so the keyring still gets its turn: a
    /// distribution signature that does not verify is not an error, it is
    /// someone else's signature.
    pub fn verify(&self, message: &[u8], signature_b64: &str) -> Result<Option<String>, Error> {
        let (Some(key), Some(fingerprint)) = (self.public_key(), self.fingerprint()) else {
            return Ok(None);
        };
        // A malformed signature is an invalid artifact; only a well-formed one
        // that does not match is "not this key".
        let parsed = signature::parse_signature(signature_b64)?;
        match signature::verify_bytes(message, &parsed, key) {
            Ok(()) => Ok(Some(fingerprint.to_string())),
            // A well-formed signature by another key is not an error; a key
            // that cannot be used at all is, and must not be read as "someone
            // else's signature".
            Err(error) if error.kind() == ErrorKind::Denied => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Refuses the operation when a compiled key cannot be used. A build that
    /// names a key it cannot parse must not pretend it has none.
    pub fn check(&self) -> Result<(), Error> {
        match &self.problem {
            Some(problem) => Err(Error::internal(format!(
                "this engine was built with an invalid distribution key: {problem}"
            ))),
            None => Ok(()),
        }
    }

    /// Why the compiled key cannot be used, if it cannot. Reported so an
    /// operator sees a broken build instead of a missing tier.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

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
        // Shape is not enough: a key that does not decompress can never verify,
        // and trusting it would only produce confusing denials later.
        signature::check_public_key(&key.public_key)?;
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
        signature::check_public_key(&key.public_key)
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
        // Parse once: a signature that is not base64 or not 64 bytes is an
        // invalid artifact, not a permission denial.
        let signature = signature::parse_signature(signature_b64)?;
        for (fingerprint, key) in self.all()? {
            if signature::verify_bytes(message, &signature, &key.public_key).is_ok() {
                return Ok(fingerprint);
            }
        }
        Err(Error::denied("no trusted key signs this bundle"))
    }
}

fn check_fingerprint(fingerprint: &str) -> Result<(), Error> {
    if is_sha256(fingerprint) {
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

    fn other_public_hex() -> String {
        SigningKey::from_bytes(&[9u8; 32])
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
        let mut other = trusted_key();
        other.public_key = other_public_hex();
        let two = keyring.add(&other).expect("add");
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
        other.public_key = other_public_hex();
        keyring.add(&other).expect("add");
        let signature = signing_key().sign(b"x");
        let encoded = signature::signature_base64(&signature.to_bytes());
        let error = keyring.verify(b"x", &encoded).expect_err("untrusted");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_stored_key_that_cannot_sign_is_refused() {
        // `add` refuses an unusable key; a file that appears in the tree some
        // other way must be refused on read too.
        let state = scratch("unusable-file");
        let keyring = Keyring::new(&state);
        let mut key = trusted_key();
        key.public_key = "02".to_string() + &"00".repeat(31);
        let fingerprint = signature::fingerprint(&key.public_key).expect("fingerprint");
        store::ensure_dir(keyring.dir()).expect("dir");
        let path = keyring.dir().join(format!("{fingerprint}.toml"));
        fs::write(&path, key.to_toml().expect("render")).expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
        let error = keyring.load(&fingerprint).expect_err("unusable key");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_corrupt_signature_is_invalid_not_denied() {
        let state = scratch("corrupt");
        let keyring = Keyring::new(&state);
        keyring.add(&trusted_key()).expect("add");
        let error = keyring.verify(b"x", "not base64!").expect_err("corrupt");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_key_that_can_never_sign_is_refused() {
        let state = scratch("unusable");
        let keyring = Keyring::new(&state);
        for public_key in [
            // Does not decompress to an Edwards point (y = 2).
            "02".to_string() + &"00".repeat(31),
            // The identity point: weak (small order).
            "01".to_string() + &"00".repeat(31),
        ] {
            let mut key = trusted_key();
            key.public_key = public_key;
            let error = keyring.add(&key).expect_err("unusable key");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        }
        assert!(keyring.all().expect("all").is_empty());
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

    #[test]
    fn a_distribution_key_that_cannot_verify_is_a_broken_build() {
        for public_key in [
            "not-hex".to_string(),
            // Does not decompress to an Edwards point (y = 2).
            "02".to_string() + &"00".repeat(31),
            // The identity point: weak (small order).
            "01".to_string() + &"00".repeat(31),
        ] {
            let distribution = Distribution::from_public_key(Some(&public_key));
            assert!(distribution.fingerprint().is_none(), "{public_key}");
            assert!(distribution.public_key().is_none(), "{public_key}");
            assert!(distribution.problem().is_some(), "{public_key}");
            let error = distribution.check().expect_err("unusable key");
            assert_eq!(error.kind(), ErrorKind::Internal);
            assert!(
                error.message().contains("invalid distribution key"),
                "{error}"
            );
        }
    }

    #[test]
    fn an_absent_or_blank_distribution_key_is_none_not_broken() {
        for text in [None, Some(""), Some("   ")] {
            let distribution = Distribution::from_public_key(text);
            assert!(distribution.fingerprint().is_none());
            assert!(distribution.problem().is_none());
            distribution.check().expect("no key is not a problem");
            assert_eq!(distribution.tier(Some("someone")), Tier::ThirdParty);
        }
    }

    #[test]
    fn only_the_distribution_fingerprint_earns_the_system_tier() {
        let distribution = Distribution::from_public_key(Some(&public_hex()));
        let fingerprint = distribution.fingerprint().expect("fingerprint");
        assert_eq!(distribution.tier(Some(fingerprint)), Tier::System);
        assert_eq!(
            distribution.tier(Some(&other_public_hex())),
            Tier::ThirdParty
        );
        assert_eq!(distribution.tier(None), Tier::ThirdParty);
    }

    #[test]
    fn a_foreign_signature_is_not_this_key_and_a_corrupt_one_is_an_error() {
        let distribution = Distribution::from_public_key(Some(&public_hex()));
        let message = b"manifest";
        let own = signature::signature_base64(&signing_key().sign(message).to_bytes());
        assert_eq!(
            distribution.verify(message, &own).expect("own"),
            distribution.fingerprint().map(str::to_string)
        );
        let foreign = signature::signature_base64(
            &SigningKey::from_bytes(&[9u8; 32]).sign(message).to_bytes(),
        );
        assert!(distribution
            .verify(message, &foreign)
            .expect("foreign")
            .is_none());
        let error = distribution
            .verify(message, "not base64!")
            .expect_err("corrupt");
        assert_eq!(error.kind(), ErrorKind::Invalid);
    }
}
