//! Extensions: signed bundles, frozen grants and the catalogue the panel shows.
//!
//! The bundle is content, the grant is policy and the daemon is the only thing
//! that turns one into the other. An extension cannot widen its own grant; a
//! changed manifest is refused until the operator approves it again.
//!
//! This module owns the management surface. The wasm runtime, HTTP anchors and
//! the iframe bridge build on [`ExtensionService::load`] and the grant stored here.

pub mod bundle;
pub mod runtime;
pub mod web;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anth_host::exec::ExecRunner;
use anth_wire::WireError;
use serde::{Deserialize, Serialize};

use crate::config::ExtensionsConfig;
use crate::log::Logger;

pub use anth_extension::{
    Anchor, Capability, ExtensionGrant, ExtensionManifest as Manifest, GrantStore, UiSlot,
    validate_id,
};
pub use bundle::{BundleInstall, LoadedBundle};

/// One entry in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionView {
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
pub struct ExtensionsView {
    pub enabled: bool,
    pub install_from_web: bool,
    pub extensions: Vec<ExtensionView>,
}

/// What a guest answered.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionReply {
    pub status: u16,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ExtensionService {
    config: ExtensionsConfig,
    root: PathBuf,
    grants: GrantStore,
    exec: Arc<dyn ExecRunner>,
}

impl ExtensionService {
    pub fn new(
        config: ExtensionsConfig,
        root: PathBuf,
        grants_dir: PathBuf,
        exec: Arc<dyn ExecRunner>,
    ) -> Self {
        Self {
            config,
            root,
            grants: GrantStore::new(grants_dir),
            exec,
        }
    }

    pub fn config(&self) -> &ExtensionsConfig {
        &self.config
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn grants(&self) -> &GrantStore {
        &self.grants
    }

    /// Lists every installed bundle, including disabled and broken ones so the
    /// operator sees why an entry is not serving.
    pub fn snapshot(&self) -> Result<ExtensionsView, WireError> {
        let entries = bundle::installed(&self.root, &self.grants)?;
        let mut extensions = Vec::with_capacity(entries.len());
        for (id, grant) in entries {
            extensions.push(self.view(&id, grant.as_ref()));
        }
        Ok(ExtensionsView {
            enabled: self.config.enabled,
            install_from_web: self.config.install_from_web,
            extensions,
        })
    }

    fn view(&self, id: &str, grant: Option<&ExtensionGrant>) -> ExtensionView {
        let Some(grant) = grant else {
            return ExtensionView {
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
        ExtensionView {
            id: grant.id.clone(),
            alias: grant.alias.clone(),
            name: grant.name.clone(),
            version: grant.extension_version.clone(),
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

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<ExtensionGrant, WireError> {
        validate_id(id)?;
        let mut grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("extension `{id}` is not installed")))?;
        grant.enabled = enabled;
        self.grants.write(&grant)?;
        Ok(grant)
    }

    pub fn remove(&self, id: &str) -> Result<(), WireError> {
        bundle::remove(&self.root, &self.grants, id)
    }

    /// Rewrites the grant from the manifest currently on disk.
    pub fn approve(
        &self,
        id: &str,
        accept_capabilities: bool,
    ) -> Result<ExtensionGrant, WireError> {
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

    /// Runs one HTTP anchor through the guest's `anth_http` entry.
    pub fn call_anchor(
        &self,
        id: &str,
        method: &str,
        path: &str,
        query: BTreeMap<String, String>,
        body: Option<String>,
        log: std::sync::Arc<Logger>,
    ) -> Result<ExtensionReply, WireError> {
        let grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("extension `{id}` is not installed")))?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!(
                "extension `{id}` is disabled"
            )));
        }
        let method = method.to_uppercase();
        if !grant
            .api
            .iter()
            .any(|anchor| anchor.method.eq_ignore_ascii_case(&method) && anchor.path == path)
        {
            return Err(WireError::not_found(format!(
                "extension `{id}` declares no anchor {method} {path}"
            )));
        }
        let envelope = serde_json::json!({
            "method": method,
            "path": path,
            "query": query,
            "body": body,
        })
        .to_string();
        self.call(id, "anth_http", &envelope, log)
    }

    /// Runs one CLI request through the guest's `anth_cli` entry.
    pub fn call_cli(
        &self,
        id: &str,
        command: &str,
        log: std::sync::Arc<Logger>,
    ) -> Result<ExtensionReply, WireError> {
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
    ) -> Result<ExtensionReply, WireError> {
        self.call(id, "anth_direct", request, log)
    }

    fn call(
        &self,
        id: &str,
        entry: &str,
        payload: &str,
        log: std::sync::Arc<Logger>,
    ) -> Result<ExtensionReply, WireError> {
        let loaded = self.load(id)?;
        let wasm = loaded.wasm.as_deref().ok_or_else(|| {
            WireError::new(
                anth_wire::ErrorKind::Unsupported,
                "service_runtime_pending",
                "service extensions are not wired into the panel yet",
            )
        })?;
        let runtime = runtime::Loaded::load(loaded.manifest, wasm, Arc::clone(&self.exec))?;
        let reply = runtime.call(entry, payload, log, self.config.fuel)?;
        Ok(ExtensionReply {
            status: reply.status,
            value: reply.value,
        })
    }
}
