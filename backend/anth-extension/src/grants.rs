//! Frozen grants: one file per installed extension under `extensions.d/`.
//!
//! The bundle is content; the grant is policy. It records the manifest hash and
//! signature, the module and asset hashes, and the normalized capability, anchor
//! and UI surface the operator approved. A rewritten bundle cannot widen its own
//! grant: a changed surface is refused until `extension approve` is run.
//!
//! The file name is the extension id, and the id is validated before it becomes a
//! path. A symlink is refused rather than followed, and writes are atomic and
//! `0600`, like every other policy file.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::WireError;
use serde::{Deserialize, Serialize};

use crate::manifest::{
    Anchor, Capability, ExtensionManifest, ExtensionSurface, RuntimeKind, UiSlot, validate_id,
};

pub const GRANT_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionGrant {
    pub version: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub name: String,
    pub extension_version: String,
    pub runtime: RuntimeKind,
    pub enabled: bool,
    /// SHA-256 of the canonical manifest-set message the operator approved.
    pub manifest_sha256: String,
    /// The detached signature over that message, when the bundle had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin_socket: Option<String>,
    pub capability: Vec<Capability>,
    pub api: Vec<Anchor>,
    pub ui: Vec<UiSlot>,
    pub installed_at_ms: u64,
}

impl ExtensionGrant {
    pub fn from_manifest(
        manifest: &ExtensionManifest,
        manifest_sha256: String,
        manifest_signature: Option<String>,
    ) -> Self {
        Self {
            version: GRANT_VERSION,
            id: manifest.extension.id.clone(),
            alias: manifest.extension.alias.clone(),
            name: manifest.extension.name.clone(),
            extension_version: manifest.extension.version.clone(),
            runtime: manifest.runtime.kind,
            enabled: true,
            manifest_sha256,
            manifest_signature,
            wasm_sha256: manifest.wasm.as_ref().map(|wasm| wasm.sha256.clone()),
            bin_socket: manifest.bin.as_ref().map(|bin| bin.socket.clone()),
            capability: manifest.capability.clone(),
            api: manifest.api.clone(),
            ui: manifest.ui.clone(),
            installed_at_ms: now_ms(),
        }
    }

    pub fn surface(&self) -> ExtensionSurface {
        ExtensionSurface {
            runtime: self.runtime,
            bin_socket: self.bin_socket.clone(),
            capability: self.capability.clone(),
            api: self.api.clone(),
            ui: self.ui.clone(),
        }
    }

    /// Whether a new manifest asks for exactly the authority already approved.
    pub fn approves(&self, manifest: &ExtensionManifest) -> bool {
        self.surface() == manifest.surface()
    }

    pub fn allowed_ids(&self) -> Vec<&str> {
        self.capability
            .iter()
            .map(|capability| capability.id.as_str())
            .collect()
    }
}

/// The directory that holds one `<id>.toml` per extension.
#[derive(Debug, Clone)]
pub struct GrantStore {
    dir: PathBuf,
}

impl GrantStore {
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

    pub fn read(&self, id: &str) -> Result<Option<ExtensionGrant>, WireError> {
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
        let grant: ExtensionGrant = toml::from_str(&text).map_err(|error| {
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
                "{} uses grant version {}; this host speaks {GRANT_VERSION}",
                path.display(),
                grant.version
            )));
        }
        Ok(Some(grant))
    }

    pub fn load_all(&self) -> Result<Vec<ExtensionGrant>, WireError> {
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
            let grant = self.read(stem)?;
            if let Some(grant) = grant {
                grants.push(grant);
            }
        }
        grants.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(grants)
    }

    pub fn write(&self, grant: &ExtensionGrant) -> Result<(), WireError> {
        let path = self.path(&grant.id)?;
        let text = toml::to_string_pretty(grant)
            .map_err(|error| WireError::internal(format!("could not render the grant: {error}")))?;
        crate::state::write_atomic(&path, text.as_bytes(), 0o600).map_err(|error| {
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ExtensionManifest;

    fn manifest() -> ExtensionManifest {
        ExtensionManifest::parse(&format!(
            r#"
manifest_version = 2
[extension]
id = "org.anth.hello"
version = "0.3.0"
name = "Hello"
[runtime]
kind = "wasm"

[wasm]
path = "wasm/extension.wasm"
abi = 3
sha256 = "{}"
"#,
            "a".repeat(64)
        ))
        .expect("manifest")
    }

    fn scratch(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).expect("scratch");
        root
    }

    #[test]
    fn a_grant_round_trips_and_is_0600() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("grant");
        let store = GrantStore::new(&root);
        let grant = ExtensionGrant::from_manifest(&manifest(), "b".repeat(64), None);
        store.write(&grant).expect("write");

        let loaded = store.read("org.anth.hello").expect("read").expect("grant");
        assert_eq!(loaded, grant);
        let mode = fs::metadata(root.join("org.anth.hello.toml"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_manifest_with_a_new_surface_needs_approval() {
        let grant = ExtensionGrant::from_manifest(&manifest(), "b".repeat(64), None);
        assert!(grant.approves(&manifest()));

        let changed = ExtensionManifest::parse(&format!(
            r#"
manifest_version = 2
[extension]
id = "org.anth.hello"
version = "0.3.0"
name = "Hello"
[runtime]
kind = "wasm"
[wasm]
path = "wasm/extension.wasm"
abi = 3
sha256 = "{}"
[[capability]]
id = "host.uname"
program = "uname"
"#,
            "a".repeat(64)
        ))
        .expect("manifest");
        assert!(!grant.approves(&changed));
    }

    #[test]
    fn a_symlink_or_a_mismatched_file_name_is_refused() {
        let root = scratch("grant-bad");
        let store = GrantStore::new(&root);
        let grant = ExtensionGrant::from_manifest(&manifest(), "b".repeat(64), None);
        store.write(&grant).expect("write");

        let text = fs::read_to_string(root.join("org.anth.hello.toml")).expect("read");
        fs::write(root.join("org.anth.other.toml"), text).expect("copy");
        assert!(store.read("org.anth.other").is_err());

        fs::remove_file(root.join("org.anth.hello.toml")).expect("remove");
        std::os::unix::fs::symlink(
            root.join("org.anth.other.toml"),
            root.join("org.anth.hello.toml"),
        )
        .expect("symlink");
        assert!(store.read("org.anth.hello").is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
