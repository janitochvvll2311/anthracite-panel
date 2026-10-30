//! Grant v3 storage: one `<id>.toml` per installed v3 app.
//!
//! It lives under `apps.d/v3` so the v2 store keeps its own directory and its
//! parser never sees a version it does not understand. The file rules are the
//! same: the id is validated before it becomes a path, a symlink is refused, and
//! writes are atomic and `0600`.

use std::fs;
use std::path::{Path, PathBuf};

use anth_wire::WireError;

use crate::apps::grant_v3::{GRANT_VERSION, GrantV3};
use crate::apps::manifest::validate_id;

#[derive(Debug, Clone)]
pub struct GrantStoreV3 {
    dir: PathBuf,
}

impl GrantStoreV3 {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: &str) -> Result<PathBuf, WireError> {
        validate_id(id)?;
        Ok(self.dir.join(format!("{id}.toml")))
    }

    pub fn read(&self, id: &str) -> Result<Option<GrantV3>, WireError> {
        let path = self.path(id)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(WireError::internal(format!(
                    "could not inspect {}: {error}",
                    path.display()
                )));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(WireError::invalid(format!(
                "{} is a symlink; grant files must be regular files",
                path.display()
            )));
        }
        let text = fs::read_to_string(&path).map_err(|error| {
            WireError::internal(format!("could not read {}: {error}", path.display()))
        })?;
        let grant: GrantV3 = toml::from_str(&text).map_err(|error| {
            WireError::invalid(format!("could not parse {}: {error}", path.display()))
        })?;
        if grant.id != id {
            return Err(WireError::invalid(format!(
                "{} declares id `{}`; the file name must match",
                path.display(),
                grant.id
            )));
        }
        if grant.version != GRANT_VERSION {
            return Err(WireError::invalid(format!(
                "{} uses grant version {}; this store speaks {GRANT_VERSION}",
                path.display(),
                grant.version
            )));
        }
        grant
            .validate()
            .map_err(|error| WireError::invalid(format!("{}: {error}", path.display())))?;
        Ok(Some(grant))
    }

    pub fn load_all(&self) -> Result<Vec<GrantV3>, WireError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(WireError::internal(format!(
                    "could not read {}: {error}",
                    self.dir.display()
                )));
            }
        };
        let mut grants = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                WireError::internal(format!("could not read {}: {error}", self.dir.display()))
            })?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    WireError::invalid(format!("{} has no usable file name", path.display()))
                })?;
            if let Some(grant) = self.read(stem)? {
                grants.push(grant);
            }
        }
        grants.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(grants)
    }

    pub fn write(&self, grant: &GrantV3) -> Result<(), WireError> {
        grant.validate().map_err(WireError::invalid)?;
        let path = self.path(&grant.id)?;
        let text = toml::to_string_pretty(grant)
            .map_err(|error| WireError::internal(format!("could not render the grant: {error}")))?;
        crate::apps::state::write_atomic(&path, text.as_bytes(), 0o600).map_err(|error| {
            WireError::internal(format!("could not write {}: {error}", path.display()))
        })
    }

    pub fn remove(&self, id: &str) -> Result<bool, WireError> {
        let path = self.path(id)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(WireError::internal(format!(
                "could not remove {}: {error}",
                path.display()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::grant_v3::{ApprovedCapability, Risk};

    fn grant(id: &str) -> GrantV3 {
        GrantV3 {
            version: GRANT_VERSION,
            id: id.to_string(),
            alias: None,
            name: "Example".to_string(),
            app_version: "0.1.0".to_string(),
            enabled: true,
            manifest_sha256: "a".repeat(64),
            manifest_signature: None,
            wasm_sha256: "b".repeat(64),
            risk: Risk::Read,
            approved_at_ms: 1,
            approved_by: "uid:0".to_string(),
            capability: vec![ApprovedCapability {
                id: format!("{id}.action"),
                version: 1,
                kind: "wasm".to_string(),
                implementation_sha256: Some("c".repeat(64)),
                sandbox: None,
                resource: Vec::new(),
                limits: None,
                risk: Risk::Read,
            }],
            import: Vec::new(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "anth-grant-v3-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_grant_round_trips_through_the_store() {
        let root = scratch("roundtrip");
        let store = GrantStoreV3::new(root.join("v3"));
        let grant = grant("org.example.app");
        store.write(&grant).expect("write");
        assert_eq!(store.read(&grant.id).expect("read"), Some(grant.clone()));
        assert_eq!(store.load_all().expect("list"), vec![grant.clone()]);
        assert!(store.remove(&grant.id).expect("remove"));
        assert!(!store.remove(&grant.id).expect("remove twice"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_symlink_is_refused() {
        let root = scratch("symlink");
        let store = GrantStoreV3::new(root.join("v3"));
        std::fs::create_dir_all(store.dir()).unwrap();
        let target = root.join("outside.toml");
        std::fs::write(&target, b"").unwrap();
        std::os::unix::fs::symlink(&target, store.dir().join("org.example.app.toml")).unwrap();
        let error = store.read("org.example.app").expect_err("symlink");
        assert!(error.message.contains("symlink"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
