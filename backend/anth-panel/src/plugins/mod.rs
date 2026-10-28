//! Plugins: signed bundles, frozen grants and the catalogue the panel shows.
//!
//! The bundle is content, the grant is policy and the daemon is the only thing
//! that turns one into the other. A plugin cannot widen its own grant; a
//! changed manifest is refused until the operator approves it again.
//!
//! This module owns the management surface. The wasm runtime, HTTP anchors and
//! the iframe bridge build on [`PluginService::load`] and the grant stored here.

pub mod bundle;
pub mod grants;
pub mod manifest;
pub mod signature;

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anth_wire::WireError;
use serde::{Deserialize, Serialize};

use crate::config::PluginsConfig;

pub use bundle::{BundleInstall, LoadedBundle};
pub use grants::{GrantStore, PluginGrant};
pub use manifest::{Anchor, Capability, Manifest, UiSlot, validate_id};

/// One entry in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginView {
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
pub struct PluginsView {
    pub enabled: bool,
    pub install_from_web: bool,
    pub plugins: Vec<PluginView>,
}

#[derive(Debug, Clone)]
pub struct PluginService {
    config: PluginsConfig,
    root: PathBuf,
    grants: GrantStore,
}

impl PluginService {
    pub fn new(config: PluginsConfig, root: PathBuf, grants_dir: PathBuf) -> Self {
        Self {
            config,
            root,
            grants: GrantStore::new(grants_dir),
        }
    }

    pub fn config(&self) -> &PluginsConfig {
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
    pub fn snapshot(&self) -> Result<PluginsView, WireError> {
        let entries = bundle::installed(&self.root, &self.grants)?;
        let mut plugins = Vec::with_capacity(entries.len());
        for (id, grant) in entries {
            plugins.push(self.view(&id, grant.as_ref()));
        }
        Ok(PluginsView {
            enabled: self.config.enabled,
            install_from_web: self.config.install_from_web,
            plugins,
        })
    }

    fn view(&self, id: &str, grant: Option<&PluginGrant>) -> PluginView {
        let Some(grant) = grant else {
            return PluginView {
                id: id.to_string(),
                alias: None,
                name: id.to_string(),
                version: String::new(),
                enabled: false,
                state: "unapproved".to_string(),
                error: Some(
                    "the bundle has no grant; run `plugin approve <id> --accept-capabilities`"
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
                Some("the operator switched this plugin off".to_string()),
            ),
            (true, Ok(_)) => ("enabled".to_string(), None),
            (true, Err(error)) => ("error".to_string(), Some(error.message)),
        };
        PluginView {
            id: grant.id.clone(),
            alias: grant.alias.clone(),
            name: grant.name.clone(),
            version: grant.plugin_version.clone(),
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
                "plugins are disabled; set `[plugins] enabled = true`",
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
                "plugins are disabled; set `[plugins] enabled = true`",
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

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<PluginGrant, WireError> {
        validate_id(id)?;
        let mut grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("plugin `{id}` is not installed")))?;
        grant.enabled = enabled;
        self.grants.write(&grant)?;
        Ok(grant)
    }

    pub fn remove(&self, id: &str) -> Result<(), WireError> {
        bundle::remove(&self.root, &self.grants, id)
    }

    /// Rewrites the grant from the manifest currently on disk.
    pub fn approve(&self, id: &str, accept_capabilities: bool) -> Result<PluginGrant, WireError> {
        if !accept_capabilities {
            return Err(WireError::invalid(
                "pass `--accept-capabilities` to approve the plugin's authority",
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

    /// Loads one enabled plugin, re-verifying its grant and every file hash.
    pub fn load(&self, id: &str) -> Result<LoadedBundle, WireError> {
        if !self.config.enabled {
            return Err(WireError::forbidden(
                "plugins are disabled; set `[plugins] enabled = true`",
            ));
        }
        validate_id(id)?;
        let grant = self
            .grants
            .read(id)?
            .ok_or_else(|| WireError::not_found(format!("plugin `{id}` is not installed")))?;
        if !grant.enabled {
            return Err(WireError::forbidden(format!("plugin `{id}` is disabled")));
        }
        bundle::load(
            &self.root,
            &grant,
            self.config.require_signature,
            &self.config.public_key,
        )
    }
}
