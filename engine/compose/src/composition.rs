//! The composition document on disk (§8.1, §6.8).
//!
//! The daemon creates the file on first boot; this store is what the HTTP
//! surface uses to replace it. Writing is atomic and refuses a symlink, so a
//! reader sees the old document or the new one.

use std::path::{Path, PathBuf};

use anth_contract::{Composition, Error};
use sha2::{Digest, Sha256};

use crate::store;

const FILE: &str = "composition.toml";

#[derive(Debug, Clone)]
pub struct CompositionStore {
    state: PathBuf,
}

impl CompositionStore {
    pub fn new(state: &Path) -> Self {
        Self {
            state: state.to_path_buf(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.state.join(FILE)
    }

    /// Replaces the document. `composition` is validated before anything is
    /// written, so a refused document leaves the old one intact.
    pub fn save(&self, composition: &Composition) -> Result<(), Error> {
        store::write_atomic(&self.state, FILE, composition.to_toml()?.as_bytes())
    }

    /// The strong validator of the document: quoted lowercase hex of its
    /// canonical TOML bytes, ready for `ETag` and `If-Match` (§6.4).
    pub fn etag(&self, composition: &Composition) -> Result<String, Error> {
        let text = composition.to_toml()?;
        let digest = Sha256::digest(text.as_bytes());
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(format!("\"{hex}\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-composition-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("chmod");
        dir
    }

    fn composed(apps: &[&str]) -> Composition {
        Composition {
            version: 1,
            profile: vec![anth_contract::Profile {
                id: "default".to_string(),
                apps: apps.iter().map(|app| app.to_string()).collect(),
            }],
        }
    }

    #[test]
    fn a_saved_document_is_read_back() {
        let dir = scratch("round-trip");
        let store = CompositionStore::new(&dir);
        let composition = composed(&["org.example.notes"]);
        store.save(&composition).expect("save");
        let text = fs::read_to_string(dir.join("composition.toml")).expect("read");
        assert_eq!(Composition::parse(&text).expect("parse"), composition);
        let mode = fs::metadata(dir.join("composition.toml"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_etag_tracks_the_document() {
        let store = CompositionStore::new(Path::new("/nonexistent"));
        let first = store.etag(&composed(&[])).expect("etag");
        let second = store.etag(&composed(&[])).expect("etag");
        assert_eq!(first, second);
        assert!(first.starts_with('"') && first.ends_with('"'));
        assert_ne!(
            first,
            store.etag(&composed(&["org.example.notes"])).expect("etag")
        );
    }

    #[test]
    fn an_invalid_document_is_not_written() {
        let dir = scratch("invalid");
        let store = CompositionStore::new(&dir);
        let invalid = Composition {
            version: 9,
            profile: Vec::new(),
        };
        assert!(store.save(&invalid).is_err());
        assert!(!dir.join("composition.toml").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
