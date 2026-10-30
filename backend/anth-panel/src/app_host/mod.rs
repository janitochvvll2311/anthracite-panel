//! Extensions: signed bundles, frozen grants and the catalogue the panel shows.
//!
//! The bundle is content, the grant is policy and the daemon is the only thing
//! that turns one into the other. An extension cannot widen its own grant; a
//! changed manifest is refused until the operator approves it again.
//!
//! This module owns the management surface. The wasm runtime, HTTP anchors and
//! the iframe bridge build on [`AppService::load`] and the grant stored here.

pub mod bin;
pub mod bundle;
pub mod bundle_v3;
pub mod exec;
pub mod runtime;
pub mod web;

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_host::exec::ExecRunner;
use anth_wire::WireError;
use serde::{Deserialize, Serialize};

use crate::apps::capability::ManifestV3;
use crate::apps::grant_v3::{self, ApprovalFlags, GrantV3};
use crate::apps::grants_v3::GrantStoreV3;
use crate::apps::manifest::MANIFEST_FILE;
use crate::config::AppsConfig;
use crate::log::Logger;

pub use crate::apps::{
    Anchor, AppGrant, AppManifest as Manifest, Capability, GrantStore, UiSlot, validate_id,
};
pub use bundle::{BundleInstall, LoadedBundle};
pub use bundle_v3::InstalledV3;

/// The result of installing a bundle, whichever manifest version it speaks.
#[derive(Debug, Clone)]
pub enum Installed {
    V2(BundleInstall),
    V3(InstalledV3),
}

/// One entry in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppView {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    /// `enabled`, `disabled`, `error` or `unapproved`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub capabilities: Vec<String>,
    pub api: Vec<Anchor>,
    pub ui: Vec<UiSlot>,
}

/// The whole catalogue plus the two policy switches that shape the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppsView {
    pub enabled: bool,
    pub install_from_web: bool,
    pub apps: Vec<AppView>,
}

/// What an app answered, already typed for the HTTP proxy.
#[derive(Debug, Clone, PartialEq)]
pub struct AppReply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl AppReply {
    /// Wraps a wasm guest's JSON value in the same shape a bin app returns.
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self {
                status,
                content_type: "text/plain; charset=utf-8".to_string(),
                body: Vec::new(),
            },
            serde_json::Value::String(text) => Self {
                status,
                content_type: "text/plain; charset=utf-8".to_string(),
                body: text.into_bytes(),
            },
            other => Self {
                status,
                content_type: "application/json".to_string(),
                body: other.to_string().into_bytes(),
            },
        }
    }

    /// The decoded value, for the CLI and tests.
    pub fn value(&self) -> serde_json::Value {
        if self.content_type.starts_with("application/json") {
            serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
        } else if self.body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(String::from_utf8_lossy(&self.body).into_owned())
        }
    }
}

/// One anchor call as the HTTP surface received it.
#[derive(Debug, Clone)]
pub struct AnchorRequest {
    pub method: String,
    pub path: String,
    /// Decoded query pairs, for the wasm `anth_http` envelope.
    pub query: BTreeMap<String, String>,
    /// The raw query, so a bin app sees exactly what the caller sent.
    pub raw_query: Option<String>,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppService {
    config: AppsConfig,
    root: PathBuf,
    grants: GrantStore,
    grants_v3: GrantStoreV3,
    exec: Arc<dyn ExecRunner>,
    /// Sockets the proxy may never reach, starting with the panel's own.
    denied_sockets: Vec<PathBuf>,
}

impl AppService {
    pub fn new(
        config: AppsConfig,
        root: PathBuf,
        grants_dir: PathBuf,
        exec: Arc<dyn ExecRunner>,
    ) -> Self {
        let grants_v3 = GrantStoreV3::new(grants_dir.join("v3"));
        Self {
            config,
            root,
            grants: GrantStore::new(grants_dir),
            grants_v3,
            exec,
            denied_sockets: Vec::new(),
        }
    }

    /// Refuses to proxy to this socket. The panel adds its own socket so a
    /// bundle cannot use the local peer-credential path to call the API.
    pub fn deny_socket(mut self, path: impl Into<PathBuf>) -> Self {
        self.denied_sockets.push(path.into());
        self
    }

    fn is_denied_socket(&self, socket: &Path) -> bool {
        let target = std::fs::canonicalize(socket).unwrap_or_else(|_| socket.to_path_buf());
        self.denied_sockets.iter().any(|denied| {
            let denied = std::fs::canonicalize(denied).unwrap_or_else(|_| denied.clone());
            denied == target
        })
    }

    pub fn config(&self) -> &AppsConfig {
        &self.config
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn grants(&self) -> &GrantStore {
        &self.grants
    }

    pub fn grants_v3(&self) -> &GrantStoreV3 {
        &self.grants_v3
    }

    /// Lists every installed bundle, including disabled and broken ones so the
    /// operator sees why an entry is not serving.
    pub fn snapshot(&self) -> Result<AppsView, WireError> {
        let entries = bundle::installed(&self.root, &self.grants)?;
        let mut by_id: BTreeMap<String, Option<AppGrant>> = entries.into_iter().collect();
        let mut apps = Vec::new();

        match fs::read_dir(&self.root) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|error| {
                        WireError::internal(format!("reading {}: {error}", self.root.display()))
                    })?;
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                        continue;
                    };
                    if name.starts_with('.') {
                        continue;
                    }
                    match self.v3_manifest(name) {
                        Ok(Some(manifest)) => {
                            by_id.remove(name);
                            apps.push(self.v3_view(name, &manifest));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            by_id.remove(name);
                            apps.push(AppView {
                                id: name.to_string(),
                                alias: None,
                                name: name.to_string(),
                                version: String::new(),
                                enabled: false,
                                state: "error".to_string(),
                                error: Some(error.message),
                                capabilities: Vec::new(),
                                api: Vec::new(),
                                ui: Vec::new(),
                            });
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(WireError::internal(format!(
                    "reading {}: {error}",
                    self.root.display()
                )));
            }
        }

        for (id, grant) in by_id {
            apps.push(self.view(&id, grant.as_ref()));
        }
        apps.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(AppsView {
            enabled: self.config.enabled,
            install_from_web: self.config.install_from_web,
            apps,
        })
    }

    /// Reads one installed v3 manifest, or `None` when the directory is v2.
    fn v3_manifest(&self, id: &str) -> Result<Option<ManifestV3>, WireError> {
        validate_id(id)?;
        let path = self.root.join(id).join(MANIFEST_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(WireError::internal(format!(
                    "reading {}: {error}",
                    path.display()
                )));
            }
        };
        let value: toml::Value = toml::from_str(&text)
            .map_err(|error| WireError::invalid(format!("{}: {error}", path.display())))?;
        if value
            .get("manifest_version")
            .and_then(toml::Value::as_integer)
            != Some(i64::from(crate::apps::capability::MANIFEST_VERSION))
        {
            return Ok(None);
        }
        ManifestV3::parse(&text)
            .map(Some)
            .map_err(WireError::invalid)
    }

    /// The hash the grant freezes for a v3 manifest set.
    fn v3_manifest_hash(&self, id: &str) -> Result<String, WireError> {
        let files = bundle::read_manifest_files(&self.root.join(id))?;
        Ok(bundle::sha256(&crate::apps::manifest::manifest_message(
            &files,
        )))
    }

    fn v3_view(&self, id: &str, manifest: &ManifestV3) -> AppView {
        let grant = self.grants_v3.read(id).ok().flatten();
        let hash_ok = grant
            .as_ref()
            .map(|grant| {
                self.v3_manifest_hash(id)
                    .map(|hash| hash == grant.manifest_sha256)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let (enabled, state, error) = match grant {
            None => (
                false,
                "unapproved".to_string(),
                Some("the app is installed but not approved".to_string()),
            ),
            Some(_) if !hash_ok => (
                false,
                "unapproved".to_string(),
                Some("the manifest changed; approve it again".to_string()),
            ),
            Some(grant) if !grant.enabled => (
                false,
                "disabled".to_string(),
                Some("the operator switched this app off".to_string()),
            ),
            Some(_) => (true, "enabled".to_string(), None),
        };
        AppView {
            id: id.to_string(),
            alias: manifest.app.alias.clone(),
            name: manifest.app.name.clone(),
            version: manifest.app.version.clone(),
            enabled,
            state,
            error,
            capabilities: manifest
                .export
                .iter()
                .map(|export| export.id.clone())
                .collect(),
            api: manifest
                .api
                .iter()
                .map(|route| Anchor {
                    method: route.method.clone(),
                    path: route.path.clone(),
                })
                .collect(),
            ui: manifest.ui.clone(),
        }
    }

    fn view(&self, id: &str, grant: Option<&AppGrant>) -> AppView {
        let Some(grant) = grant else {
            return AppView {
                id: id.to_string(),
                alias: None,
                name: id.to_string(),
                version: String::new(),
                enabled: false,
                state: "unapproved".to_string(),
                error: Some(
                    "the bundle has no grant; run `extension approve <id> --accept-capabilities`"
                        .to_string(),
                ),
                capabilities: Vec::new(),
                api: Vec::new(),
                ui: Vec::new(),
            };
        };
        let loaded = bundle::load(
            &self.root,
            grant,
            self.config.require_signature,
            &self.config.public_key,
        );
        let (state, error) = match (grant.enabled, loaded) {
            (false, _) => (
                "disabled".to_string(),
                Some("the operator switched this extension off".to_string()),
            ),
            (true, Ok(_)) => ("enabled".to_string(), None),
            (true, Err(error)) => ("error".to_string(), Some(error.message)),
        };
        AppView {
            id: grant.id.clone(),
            alias: grant.alias.clone(),
            name: grant.name.clone(),
            version: grant.app_version.clone(),
            enabled: grant.enabled,
            state,
            error,
            capabilities: grant
                .capability
                .iter()
                .map(|capability| capability.id.clone())
                .collect(),
            api: grant.api.clone(),
            ui: grant.ui.clone(),
        }
    }

    /// Installs one zip from bytes. The caller decides whether web install is
    /// allowed; this service enforces the master switch and the bundle rules.
    pub fn install_bytes(
        &self,
        bytes: &[u8],
        accept_capabilities: bool,
    ) -> Result<BundleInstall, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
            WireError::invalid(format!("the upload is not a zip bundle: {error}"))
        })?;
        bundle::install_reader(
            &mut archive,
            &self.root,
            &self.grants,
            self.config.require_signature,
            &self.config.public_key,
            accept_capabilities,
        )
    }

    pub fn install_file(
        &self,
        bundle: &Path,
        accept_capabilities: bool,
    ) -> Result<BundleInstall, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        bundle::install_file(
            bundle,
            &self.root,
            &self.grants,
            self.config.require_signature,
            &self.config.public_key,
            accept_capabilities,
        )
    }

    /// Installs whichever manifest version the bundle carries.
    pub fn install_any_bytes(
        &self,
        bytes: &[u8],
        accept_capabilities: bool,
    ) -> Result<Installed, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        match bundle_v3::manifest_version(bytes)? {
            3 => Ok(Installed::V3(self.install_v3_bytes(bytes)?)),
            2 => Ok(Installed::V2(
                self.install_bytes(bytes, accept_capabilities)?,
            )),
            other => Err(WireError::new(
                anth_wire::ErrorKind::Unsupported,
                "manifest_version",
                format!("manifest version {other} is not supported"),
            )),
        }
    }

    pub fn install_any_file(
        &self,
        bundle: &Path,
        accept_capabilities: bool,
    ) -> Result<Installed, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        match bundle_v3::manifest_version_file(bundle)? {
            3 => Ok(Installed::V3(self.install_v3_file(bundle)?)),
            2 => Ok(Installed::V2(
                self.install_file(bundle, accept_capabilities)?,
            )),
            other => Err(WireError::new(
                anth_wire::ErrorKind::Unsupported,
                "manifest_version",
                format!("manifest version {other} is not supported"),
            )),
        }
    }

    pub fn install_v3_bytes(&self, bytes: &[u8]) -> Result<InstalledV3, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        bundle_v3::install_bytes(
            bytes,
            &self.root,
            self.config.require_signature,
            &self.config.public_key,
        )
    }

    pub fn install_v3_file(&self, bundle: &Path) -> Result<InstalledV3, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        bundle_v3::install_file(
            bundle,
            &self.root,
            self.config.require_signature,
            &self.config.public_key,
        )
    }

    pub fn is_v3(&self, id: &str) -> Result<bool, WireError> {
        Ok(self.v3_manifest(id)?.is_some())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), WireError> {
        if self.v3_manifest(id)?.is_some() {
            let mut grant = self.grants_v3.read(id)?.ok_or_else(|| {
                WireError::forbidden(format!(
                    "app `{id}` is installed but not approved; approve it first"
                ))
            })?;
            grant.enabled = enabled;
            self.grants_v3.write(&grant)?;
            return Ok(());
        }
        validate_id(id)?;
        let mut grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("extension `{id}` is not installed")))?;
        grant.enabled = enabled;
        self.grants.write(&grant)?;
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<(), WireError> {
        if self.v3_manifest(id)?.is_some() {
            self.grants_v3.remove(id)?;
            let dir = self.root.join(id);
            match fs::remove_dir_all(&dir) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(WireError::internal(format!(
                        "removing {}: {error}",
                        dir.display()
                    )));
                }
            }
            return Ok(());
        }
        bundle::remove(&self.root, &self.grants, id)
    }

    /// Rewrites the grant from the manifest currently on disk.
    pub fn approve(&self, id: &str, accept_capabilities: bool) -> Result<AppGrant, WireError> {
        if !accept_capabilities {
            return Err(WireError::invalid(
                "pass `--accept-capabilities` to approve the extension's authority",
            ));
        }
        bundle::approve(
            &self.root,
            &self.grants,
            id,
            self.config.require_signature,
            &self.config.public_key,
        )
    }

    /// Freezes a v3 grant after checking every approval flag the manifest needs.
    pub fn approve_v3(&self, id: &str, flags: ApprovalFlags) -> Result<GrantV3, WireError> {
        let manifest = self
            .v3_manifest(id)?
            .ok_or_else(|| WireError::not_found(format!("no v3 app `{id}` is installed")))?;
        let required = grant_v3::approvals(&manifest).map_err(WireError::invalid)?;
        let missing = flags.missing(&required);
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|level| level.as_str()).collect();
            return Err(WireError::invalid(format!(
                "missing approval: {}",
                names.join(", ")
            )));
        }
        let hash = self.v3_manifest_hash(id)?;
        let signature = fs::read_to_string(self.root.join(id).join("manifest.sig"))
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        let grant =
            GrantV3::from_manifest(&manifest, hash, signature, "local".to_string(), now_ms())
                .map_err(WireError::invalid)?;
        self.grants_v3.write(&grant)?;
        Ok(grant)
    }

    /// Loads one approved, enabled v3 extension.
    pub fn load_v3(&self, id: &str) -> Result<runtime::LoadedV3, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        let manifest = self
            .v3_manifest(id)?
            .ok_or_else(|| WireError::not_found(format!("no v3 app `{id}` is installed")))?;
        let grant = self.grants_v3.read(id)?.ok_or_else(|| {
            WireError::forbidden(format!("app `{id}` is not approved; run `app approve`"))
        })?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!("app `{id}` is disabled")));
        }
        if self.v3_manifest_hash(id)? != grant.manifest_sha256 {
            return Err(WireError::forbidden(format!(
                "app `{id}` changed since approval; approve it again"
            )));
        }
        let wasm = bundle::verify_hash(
            &self.root.join(id).join(&manifest.wasm.path),
            &manifest.wasm.sha256,
            "module",
        )?;
        runtime::LoadedV3::load(manifest, &wasm)
    }

    /// Loads one enabled extension, re-verifying its grant and every file hash.
    pub fn load(&self, id: &str) -> Result<LoadedBundle, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        validate_id(id)?;
        let grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("extension `{id}` is not installed")))?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!(
                "extension `{id}` is disabled"
            )));
        }
        bundle::load(
            &self.root,
            &grant,
            self.config.require_signature,
            &self.config.public_key,
        )
    }

    /// Reads one declared UI module. The grant, not the manifest, decides what
    /// may be served.
    pub fn asset(&self, id: &str, module: &str) -> Result<Vec<u8>, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "extensions are disabled; set `[extensions] enabled = true`",
            ));
        }
        validate_id(id)?;
        if let Some(manifest) = self.v3_manifest(id)? {
            return self.v3_asset(id, &manifest, module);
        }
        let grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("extension `{id}` is not installed")))?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!(
                "extension `{id}` is disabled"
            )));
        }
        web::read_asset(&self.root, &grant, module)
    }

    fn v3_asset(
        &self,
        id: &str,
        manifest: &ManifestV3,
        module: &str,
    ) -> Result<Vec<u8>, WireError> {
        let grant = self.grants_v3.read(id)?.ok_or_else(|| {
            WireError::forbidden(format!("app `{id}` is not approved; run `app approve`"))
        })?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!("app `{id}` is disabled")));
        }
        if self.v3_manifest_hash(id)? != grant.manifest_sha256 {
            return Err(WireError::forbidden(format!(
                "app `{id}` changed since approval; approve it again"
            )));
        }
        let slot = manifest
            .ui
            .iter()
            .find(|slot| slot.module == module)
            .ok_or_else(|| {
                WireError::not_found(format!("app `{id}` declares no UI module `{module}`"))
            })?;
        let dir = self.root.join(id);
        let base = dir
            .canonicalize()
            .map_err(|error| WireError::not_found(format!("reading app `{id}`: {error}")))?;
        let path = dir.join(module).canonicalize().map_err(|error| {
            WireError::not_found(format!("reading `{module}` from `{id}`: {error}"))
        })?;
        if !path.starts_with(&base) || !path.is_file() {
            return Err(WireError::forbidden(format!(
                "`{module}` is not a regular file inside the app"
            )));
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| WireError::internal(format!("reading {module}: {error}")))?;
        if metadata.len() > bundle::MAX_ASSET_BYTES {
            return Err(WireError::too_large(format!(
                "`{module}` is larger than {} bytes",
                bundle::MAX_ASSET_BYTES
            )));
        }
        bundle::verify_hash(&path, &slot.sha256, "UI module")
    }

    /// Runs one HTTP anchor, through the guest for a wasm app or over the
    /// grant's Unix socket for a bin app.
    ///
    /// The grant decides both the route and the runtime; the caller only
    /// supplies the request.
    pub async fn call_anchor(
        &self,
        id: &str,
        request: AnchorRequest,
        log: std::sync::Arc<Logger>,
    ) -> Result<AppReply, WireError> {
        if let Some(manifest) = self.v3_manifest(id)? {
            return self.call_anchor_v3(id, manifest, request, log).await;
        }
        let grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("extension `{id}` is not installed")))?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!(
                "extension `{id}` is disabled"
            )));
        }
        let method = request.method.to_uppercase();
        if !grant.api.iter().any(|anchor| {
            anchor.method.eq_ignore_ascii_case(&method) && anchor.path == request.path
        }) {
            return Err(WireError::not_found(format!(
                "extension `{id}` declares no anchor {method} {}",
                request.path
            )));
        }

        let LoadedBundle {
            manifest,
            wasm,
            dir: _,
        } = self.load(id)?;
        if let Some(wasm) = wasm {
            let exec = Arc::clone(&self.exec);
            let fuel = self.config.fuel;
            let body = if request.body.is_empty() {
                None
            } else {
                Some(String::from_utf8_lossy(&request.body).into_owned())
            };
            let envelope = serde_json::json!({
                "method": method,
                "path": request.path,
                "query": request.query,
                "body": body,
            })
            .to_string();
            return tokio::task::spawn_blocking(move || {
                let runtime = runtime::Loaded::load(manifest, &wasm, exec)?;
                let reply = runtime.call("anth_http", &envelope, log, fuel)?;
                Ok(AppReply::json(reply.status, reply.value))
            })
            .await
            .map_err(|error| WireError::internal(format!("the app task failed: {error}")))?;
        }

        let socket = grant.bin_socket.ok_or_else(|| {
            WireError::invalid(format!(
                "extension `{id}` has no socket, so its anchors cannot be served"
            ))
        })?;
        if self.is_denied_socket(Path::new(&socket)) {
            return Err(WireError::forbidden(
                "an app may not proxy the panel's own socket",
            ));
        }
        bin::call(
            Path::new(&socket),
            &method,
            &request.path,
            request.raw_query.as_deref(),
            request.body,
            request.content_type.as_deref(),
        )
        .await
    }

    async fn call_anchor_v3(
        &self,
        id: &str,
        manifest: ManifestV3,
        request: AnchorRequest,
        log: std::sync::Arc<Logger>,
    ) -> Result<AppReply, WireError> {
        let grant = self.grants_v3.read(id)?.ok_or_else(|| {
            WireError::forbidden(format!("app `{id}` is not approved; run `app approve`"))
        })?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!("app `{id}` is disabled")));
        }
        if self.v3_manifest_hash(id)? != grant.manifest_sha256 {
            return Err(WireError::forbidden(format!(
                "app `{id}` changed since approval; approve it again"
            )));
        }
        let method = request.method.to_uppercase();
        if !manifest
            .api
            .iter()
            .any(|route| route.method.eq_ignore_ascii_case(&method) && route.path == request.path)
        {
            return Err(WireError::not_found(format!(
                "app `{id}` declares no anchor {method} {}",
                request.path
            )));
        }
        let body = if request.body.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&request.body).into_owned())
        };
        let envelope = serde_json::json!({
            "method": method,
            "path": request.path,
            "query": request.query,
            "body": body,
        })
        .to_string();
        let loaded = self.load_v3(id)?;
        let fuel = self.config.fuel;
        tokio::task::spawn_blocking(move || {
            let reply = loaded.call("anth_http", &envelope, log, fuel)?;
            Ok(AppReply::json(reply.status, reply.value))
        })
        .await
        .map_err(|error| WireError::internal(format!("the app task failed: {error}")))?
    }

    /// Runs one CLI request through the guest's `anth_cli` entry.
    pub fn call_cli(
        &self,
        id: &str,
        command: &str,
        log: std::sync::Arc<Logger>,
    ) -> Result<AppReply, WireError> {
        let envelope = serde_json::to_string(command)
            .map_err(|error| WireError::invalid(format!("the command is not JSON: {error}")))?;
        self.call(id, "anth_cli", &envelope, log)
    }

    /// Runs one direct request through the guest's `anth_direct` entry.
    pub fn call_direct(
        &self,
        id: &str,
        request: &str,
        log: std::sync::Arc<Logger>,
    ) -> Result<AppReply, WireError> {
        self.call(id, "anth_direct", request, log)
    }

    fn call(
        &self,
        id: &str,
        entry: &str,
        payload: &str,
        log: std::sync::Arc<Logger>,
    ) -> Result<AppReply, WireError> {
        if self.v3_manifest(id)?.is_some() {
            return Err(WireError::new(
                anth_wire::ErrorKind::Unsupported,
                "v3_cli_pending",
                "v3 CLI dispatch is not wired yet; use the app's HTTP anchors",
            ));
        }
        let loaded = self.load(id)?;
        let wasm = loaded.wasm.as_deref().ok_or_else(|| {
            WireError::new(
                anth_wire::ErrorKind::Unsupported,
                "bin_app_has_no_cli",
                "bin apps are served over HTTP anchors; anth_cli/anth_direct belong to wasm plugins",
            )
        })?;
        let runtime = runtime::Loaded::load(loaded.manifest, wasm, Arc::clone(&self.exec))?;
        let reply = runtime.call(entry, payload, log, self.config.fuel)?;
        Ok(AppReply::json(reply.status, reply.value))
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_host::exec::DemoExecRunner;

    #[test]
    fn the_panel_socket_is_denied_even_behind_a_symlink() {
        let root = std::env::temp_dir().join(format!(
            "anth-denied-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("scratch");
        let panel = root.join("panel.sock");
        std::fs::write(&panel, b"").expect("panel socket stand-in");
        let alias = root.join("alias.sock");
        std::os::unix::fs::symlink(&panel, &alias).expect("symlink");

        let service = AppService::new(
            AppsConfig::default(),
            root.join("apps"),
            root.join("grants"),
            Arc::new(DemoExecRunner::new()),
        )
        .deny_socket(&panel);

        assert!(service.is_denied_socket(&panel));
        assert!(service.is_denied_socket(&alias));
        assert!(!service.is_denied_socket(&root.join("other.sock")));
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
