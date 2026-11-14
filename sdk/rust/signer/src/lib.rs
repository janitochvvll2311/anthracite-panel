//! Signing an app bundle (§9.1, D-Signature-Algorithm).
//!
//! The signing half of the format lives in the SDK, not in the engine: the
//! engine only verifies, so a binary that ships to an operator carries no
//! private key. `anth-sign` signs a bundle directory — `manifest.toml` plus
//! `wit/**` — and writes `manifest.sig`, the one artifact the engine reads.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anth_compose::bundle;
use anth_compose::signature;
use anth_contract::Error;
use ed25519_dalek::{Signer, SigningKey};

/// An Ed25519 private key is a 32-byte seed.
pub const PRIVATE_KEY_BYTES: usize = 32;

/// The public half of a signing key: what an operator trusts in the keyring and
/// what a build compiles in as the distribution key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    /// The 32 bytes of the verifying key, lowercase hex.
    pub key: String,
    /// `sha256(public key)`, lowercase hex: the name the engine uses.
    pub fingerprint: String,
}

/// Writes `manifest.sig` for `dir`, over exactly the set the engine verifies.
///
/// The message is computed by the engine's own `signed_message`, so the signer
/// cannot drift from the verifier. Signing twice is idempotent: Ed25519 is
/// deterministic and the message is canonical.
pub fn sign(dir: &Path, key_path: &Path) -> Result<PublicKey, Error> {
    let key = read_key(key_path)?;
    let message = bundle::signed_message(dir)?;
    let signature = signature::signature_base64(&key.sign(&message).to_bytes());
    fs::write(dir.join("manifest.sig"), format!("{signature}\n")).map_err(|error| {
        Error::internal(format!(
            "writing {}: {error}",
            dir.join("manifest.sig").display()
        ))
    })?;
    public_key(&key)
}

/// Generates a key pair, writes the private half with mode 0600 and refuses to
/// overwrite an existing file.
pub fn generate_key(out: &Path) -> Result<PublicKey, Error> {
    let mut bytes = [0u8; PRIVATE_KEY_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|error| Error::internal(format!("no operating-system randomness: {error}")))?;
    let key = SigningKey::from_bytes(&bytes);
    let text = format!("{}\n", signature::hex_encode(&bytes));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(out)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => Error::conflict(format!(
                "{} already exists; refusing to overwrite a key",
                out.display()
            )),
            _ => Error::internal(format!("writing {}: {error}", out.display())),
        })?;
    file.write_all(text.as_bytes())
        .map_err(|error| Error::internal(format!("writing {}: {error}", out.display())))?;
    public_key(&key)
}

/// Reads a private key: one line of 32 bytes in lowercase hex, from a file only
/// its owner can read.
///
/// A key anybody can read is a key anybody can sign with, so the mode is part
/// of the contract instead of an advice in the docs.
pub fn read_key(path: &Path) -> Result<SigningKey, Error> {
    let metadata = fs::metadata(path)
        .map_err(|error| Error::not_found(format!("reading {}: {error}", path.display())))?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(Error::denied(format!(
            "{} is accessible by others (mode {mode:o}); chmod 600 it",
            path.display()
        )));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| Error::internal(format!("reading {}: {error}", path.display())))?;
    let bytes = signature::hex_decode(text.trim())?;
    let bytes: [u8; PRIVATE_KEY_BYTES] = bytes
        .try_into()
        .map_err(|_| Error::invalid("an Ed25519 private key is 32 bytes of lowercase hex"))?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// Reads a private key and returns its public half, without signing anything.
///
/// The release script uses it to prove that the key that signs the bundles is
/// the key the binaries were compiled with; nothing secret is printed.
pub fn key_info(key_path: &Path) -> Result<PublicKey, Error> {
    public_key(&read_key(key_path)?)
}

fn public_key(key: &SigningKey) -> Result<PublicKey, Error> {
    let key = signature::hex_encode(&key.verifying_key().to_bytes());
    let fingerprint = signature::fingerprint(&key)?;
    Ok(PublicKey { key, fingerprint })
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::path::PathBuf;

    use anth_compose::bundle::Options;
    use anth_compose::keys::{Distribution, Keyring};
    use anth_contract::Tier;
    use sha2::{Digest, Sha256};
    use zip::write::SimpleFileOptions;

    use super::*;

    const WASM: &[u8] = b"\0asm a fake component";
    const WIT: &str = "package example:notes@1.0.0;\n\ninterface notes {\n  all: func() -> list<string>;\n}\n\nworld app {\n  export notes;\n}\n";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-signer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// A bundle directory: manifest, WIT, component and the manifest's hash.
    fn app_dir(name: &str) -> PathBuf {
        let dir = scratch(name);
        let manifest = format!(
            "manifest_version = 2\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n\n[wasm]\npath = \"wasm/app.wasm\"\nsha256 = \"{}\"\n\n[[export]]\ninterface = \"example:notes/notes@1.0.0\"\n",
            hex(&Sha256::digest(WASM))
        );
        fs::write(dir.join("manifest.toml"), manifest).expect("manifest");
        fs::create_dir_all(dir.join("wit")).expect("wit dir");
        fs::create_dir_all(dir.join("wasm")).expect("wasm dir");
        fs::write(dir.join("wit/notes.wit"), WIT).expect("wit");
        fs::write(dir.join("wasm/app.wasm"), WASM).expect("wasm");
        dir
    }

    /// Packs a directory into the zip an engine installs.
    fn zip_dir(dir: &Path) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for entry in [
            "manifest.toml",
            "manifest.sig",
            "wit/notes.wit",
            "wasm/app.wasm",
        ] {
            let bytes = fs::read(dir.join(entry)).expect("read entry");
            writer
                .start_file(entry, SimpleFileOptions::default())
                .expect("start");
            writer.write_all(&bytes).expect("write");
        }
        writer.finish().expect("finish").into_inner()
    }

    fn options() -> Options {
        Options {
            max_bundle_bytes: 1 << 20,
            require_signature: true,
            allow_third_party_service: false,
            known_packages: Vec::new(),
        }
    }

    #[test]
    fn a_signed_bundle_installs_as_system() {
        let dir = app_dir("round-trip");
        let key = dir.join("key");
        let generated = generate_key(&key).expect("keygen");
        let signed = sign(&dir, &key).expect("sign");
        assert_eq!(signed, generated);

        let distribution = Distribution::from_public_key(Some(&generated.key));
        let staged = bundle::extract(
            &zip_dir(&dir),
            &dir.join("staging"),
            &options(),
            &Keyring::new(&dir),
            &distribution,
        )
        .expect("extract");
        assert_eq!(staged.tier(), Tier::System);
        assert_eq!(staged.signer(), Some(generated.fingerprint.as_str()));
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn signing_is_deterministic_and_rewrites_the_signature() {
        let dir = app_dir("deterministic");
        let key = dir.join("key");
        generate_key(&key).expect("keygen");
        sign(&dir, &key).expect("first");
        let first = fs::read_to_string(dir.join("manifest.sig")).expect("sig");
        sign(&dir, &key).expect("second");
        let second = fs::read_to_string(dir.join("manifest.sig")).expect("sig");
        assert_eq!(first, second);
        assert!(first.ends_with('\n'));
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_generated_key_is_private_to_its_owner() {
        let dir = scratch("key-mode");
        let key = dir.join("key");
        generate_key(&key).expect("keygen");
        let mode = fs::metadata(&key).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn key_info_matches_the_key_that_signs() {
        let dir = app_dir("key-info");
        let key = dir.join("key");
        let generated = generate_key(&key).expect("keygen");
        assert_eq!(key_info(&key).expect("info"), generated);
        let signed = sign(&dir, &key).expect("sign");
        assert_eq!(key_info(&key).expect("info"), signed);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_key_others_can_read_is_refused() {
        let dir = scratch("world-readable");
        let key = dir.join("key");
        generate_key(&key).expect("keygen");
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).expect("chmod");
        let error = read_key(&key).expect_err("world-readable");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(error.message().contains("chmod 600"), "{error}");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn keygen_never_overwrites_a_key() {
        let dir = scratch("no-overwrite");
        let key = dir.join("key");
        generate_key(&key).expect("keygen");
        let before = fs::read_to_string(&key).expect("key");
        let error = generate_key(&key).expect_err("existing key");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        assert_eq!(fs::read_to_string(&key).expect("key"), before);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_directory_without_a_manifest_is_refused() {
        let dir = scratch("no-manifest");
        let key = dir.join("key");
        generate_key(&key).expect("keygen");
        let error = sign(&dir, &key).expect_err("no manifest");
        assert!(error.message().contains("manifest.toml"), "{error}");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_signed_wit_set_cannot_be_edited_afterwards() {
        let dir = app_dir("tampered");
        let key = dir.join("key");
        let generated = generate_key(&key).expect("keygen");
        sign(&dir, &key).expect("sign");
        // A comment is valid WIT, so only the signature can catch the edit.
        let edited = format!("{WIT}// edited after signing\n");
        fs::write(dir.join("wit/notes.wit"), edited).expect("edit");
        let distribution = Distribution::from_public_key(Some(&generated.key));
        let error = bundle::extract(
            &zip_dir(&dir),
            &dir.join("staging"),
            &options(),
            &Keyring::new(&dir),
            &distribution,
        )
        .expect_err("tampered WIT");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
