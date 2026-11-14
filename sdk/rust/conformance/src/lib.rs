//! The offline conformance kit: validate a bundle with the same checks the
//! engine applies at install, without a daemon or a state tree.
//!
//! The kit is deliberately thin: it calls the engine's bundle verifier with a
//! temporary state directory and reports what the manifest claims. It does not
//! grant authority and it does not run the component.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anth_compose::bundle::{self, Options};
use anth_compose::keys::{Distribution, Keyring};
use anth_contract::{Error, TrustedKey, KEY_VERSION};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// What the caller permits while validating.
#[derive(Debug, Clone)]
pub struct Request {
    /// A bundle without `manifest.sig` is refused.
    pub require_signature: bool,
    /// A third-party service bundle is refused unless this is set.
    pub allow_third_party_service: bool,
    /// Provider packages the bundle may import besides the native set.
    pub known_packages: Vec<String>,
    /// Ed25519 public keys in lowercase hex; they are added to a temporary keyring.
    pub keys: Vec<String>,
    /// The compressed bundle cap.
    pub max_bundle_bytes: u64,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            require_signature: false,
            allow_third_party_service: false,
            known_packages: Vec::new(),
            keys: Vec::new(),
            max_bundle_bytes: 32 * 1024 * 1024,
        }
    }
}

/// What a verified bundle claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub id: String,
    pub version: String,
    pub package: String,
    pub signer: Option<String>,
    /// The tier the signature earns on this build: `system` only when the kit
    /// was built with the distribution key and that key signed the bundle.
    pub tier: String,
    pub exports: Vec<String>,
    pub imports: Vec<String>,
    pub service: Option<String>,
}

impl Report {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "version": self.version,
            "package": self.package,
            "signer": self.signer,
            "tier": self.tier,
            "exports": self.exports,
            "imports": self.imports,
            "service": self.service,
        })
    }
}

/// Verify `bundle` and return its claims. The temporary state lives only for
/// the call; the staged tree is dropped before this returns.
pub fn validate(bundle: &[u8], request: &Request) -> Result<Report, Error> {
    let state = scratch();
    let result = (|| {
        let keyring = Keyring::new(&state);
        for key in &request.keys {
            keyring.add(&TrustedKey {
                version: KEY_VERSION,
                algorithm: anth_contract::KeyAlgorithm::Ed25519,
                public_key: key.clone(),
                label: Some("conformance".to_string()),
                added_by: "conformance".to_string(),
                added_at: "1970-01-01T00:00:00Z".to_string(),
            })?;
        }
        let options = Options {
            max_bundle_bytes: request.max_bundle_bytes,
            require_signature: request.require_signature,
            allow_third_party_service: request.allow_third_party_service,
            known_packages: request.known_packages.clone(),
        };
        let staged = bundle::extract(
            bundle,
            &state,
            &options,
            &keyring,
            &Distribution::embedded(),
        )?;
        let manifest = staged.manifest();
        Ok(Report {
            id: manifest.app.id.clone(),
            version: manifest.app.version.clone(),
            package: manifest.app.namespace.clone(),
            signer: staged.signer().map(ToString::to_string),
            tier: match staged.tier() {
                anth_contract::Tier::System => "system".to_string(),
                anth_contract::Tier::ThirdParty => "third-party".to_string(),
            },
            exports: manifest
                .export
                .iter()
                .map(|export| export.interface.clone())
                .collect(),
            imports: manifest
                .import
                .iter()
                .map(|import| import.interface.clone())
                .collect(),
            service: manifest
                .service
                .as_ref()
                .map(|service| service.class.as_str().to_string()),
        })
    })();
    let _ = fs::remove_dir_all(&state);
    result
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
        "conformance-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create the conformance scratch directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .expect("chmod the conformance scratch directory");
    dir
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::os::unix::fs::PermissionsExt;

    use base64::Engine as _;
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use zip::write::SimpleFileOptions;

    use super::*;

    const WASM: &[u8] = b"\0asm a fake component";
    const WIT: &str = "package example:notes@1.0.0;\n\ninterface notes {\n  all: func() -> list<string>;\n}\n\nworld app {\n  export notes;\n}\n";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn manifest(wasm: &[u8]) -> String {
        format!(
            "manifest_version = 2\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n\n[wasm]\npath = \"wasm/app.wasm\"\nsha256 = \"{}\"\n\n[[export]]\ninterface = \"example:notes/notes@1.0.0\"\n",
            hex(&Sha256::digest(wasm))
        )
    }

    fn manifest_with_import(wasm: &[u8], import: &str) -> String {
        format!("{}\n[[import]]\ninterface = \"{import}\"\n", manifest(wasm))
    }

    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("start");
            writer.write_all(bytes).expect("write");
        }
        writer.finish().expect("finish").into_inner()
    }

    fn bundle(wit: &str) -> Vec<u8> {
        let manifest = manifest(WASM);
        zip(&[
            ("manifest.toml", manifest.as_bytes()),
            ("wit/notes.wit", wit.as_bytes()),
            ("wasm/app.wasm", WASM),
        ])
    }

    #[test]
    fn a_minimal_bundle_passes_and_reports_its_claims() {
        let report = validate(&bundle(WIT), &Request::default()).expect("valid");
        assert_eq!(report.id, "org.example.notes");
        assert_eq!(report.version, "0.1.0");
        assert_eq!(report.package, "example:notes");
        assert_eq!(report.exports, ["example:notes/notes@1.0.0"]);
        assert!(report.imports.is_empty());
        assert!(report.signer.is_none());
    }

    #[test]
    fn a_tampered_component_is_refused() {
        let manifest = manifest(WASM);
        let tampered = zip(&[
            ("manifest.toml", manifest.as_bytes()),
            ("wit/notes.wit", WIT.as_bytes()),
            ("wasm/app.wasm", b"another component"),
        ]);
        let error = validate(&tampered, &Request::default()).expect_err("tampered");
        assert!(error.message().contains("manifest hash"), "{error}");
    }

    #[test]
    fn an_unknown_import_is_refused_unless_known() {
        let wit = "package example:notes@1.0.0;\n\ninterface notes {\n  use example:other/x@1.0.0.{thing};\n  all: func() -> thing;\n}\n\nworld app {\n  import example:other/x@1.0.0;\n  export notes;\n}\n";
        let manifest = manifest_with_import(WASM, "example:other/x@1.0.0");
        let bundle = zip(&[
            ("manifest.toml", manifest.as_bytes()),
            ("wit/notes.wit", wit.as_bytes()),
            ("wasm/app.wasm", WASM),
        ]);
        let error = validate(&bundle, &Request::default()).expect_err("unknown import");
        assert!(
            error.message().contains("neither native nor installed"),
            "{error}"
        );

        let request = Request {
            known_packages: vec!["example:other".to_string()],
            ..Request::default()
        };
        let report = validate(&bundle, &request).expect("known import");
        assert_eq!(report.imports, ["example:other/x@1.0.0"]);
    }

    #[test]
    fn a_signed_bundle_needs_its_key_and_verifies() {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let public = hex(signing.verifying_key().as_bytes());
        let manifest = manifest(WASM);
        let signed_files = vec![
            ("manifest.toml".to_string(), manifest.as_bytes().to_vec()),
            ("wit/notes.wit".to_string(), WIT.as_bytes().to_vec()),
        ];
        let message = anth_contract::signature::message(&signed_files).expect("message");
        let signature =
            base64::engine::general_purpose::STANDARD.encode(signing.sign(&message).to_bytes());
        let bundle = zip(&[
            ("manifest.toml", manifest.as_bytes()),
            ("wit/notes.wit", WIT.as_bytes()),
            ("wasm/app.wasm", WASM),
            ("manifest.sig", signature.as_bytes()),
        ]);

        let error = validate(
            &bundle,
            &Request {
                require_signature: true,
                ..Request::default()
            },
        )
        .expect_err("untrusted signer");
        assert!(
            error.message().to_lowercase().contains("sign")
                || error.message().to_lowercase().contains("trust"),
            "{error}"
        );

        let report = validate(
            &bundle,
            &Request {
                require_signature: true,
                keys: vec![public],
                ..Request::default()
            },
        )
        .expect("trusted signature");
        assert!(report.signer.is_some());
    }

    #[test]
    fn the_scratch_directory_is_private_and_removed() {
        let state = scratch();
        assert_eq!(
            fs::metadata(&state).expect("metadata").permissions().mode() & 0o777,
            0o700
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }
}
