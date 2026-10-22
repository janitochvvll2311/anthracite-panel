//! Plugin support: manifests, bundles and the grant registry.
//!
//! A plugin extends the panel without receiving authority over the host. The
//! daemon is the chef: it runs programs. The manifest is a recipe: it declares
//! which programs a plugin needs and the shape of their arguments. The plugin is
//! a helper: it hands over declarative values for a recipe the admin already
//! approved.
//!
//! The invariant that makes this a boundary rather than a convention is that a
//! plugin never supplies an argv. It names a capability, the host looks that
//! capability up in the grant frozen at install time, validates every value with
//! a validator the host owns, and renders the argv itself. See `manifest.rs` for
//! the schema and `bundle.rs` for the archive side.

pub mod api;
pub mod bundle;
pub mod manifest;
pub mod runtime;

use std::path::{Path, PathBuf};

use ::serde::{Deserialize, Serialize};

pub use bundle::{Installed, MAX_BUNDLE_BYTES};
pub use manifest::{
    ApiAnchor, ArgSpec, ArgType, Capability, InstalledPlugin, Manifest, PluginError,
    PluginErrorKind, PluginGrant, UiAnchor,
};

/// Where installed plugins live, and which of them may do anything.
///
/// Grants live in the config (`plugins.grants`), not on disk next to the code,
/// because authority belongs where the admin already looks and edits. The bundle
/// directory is content; the config is policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginsConfig {
    /// Whether the daemon loads and runs plugins at all.
    pub enabled: bool,
    /// Root that holds one directory per installed plugin.
    pub root: String,
    /// Whether the HTTP API accepts a bundle upload. Gated like the other
    /// dangerous switches: installing means running code.
    pub install_from_web: bool,
    /// Instruction budget for one wasm invocation. Fuel is the only way to stop
    /// a module that loops, since wasm has no wall-clock interrupt.
    pub fuel: u64,
    /// Grants, one per installed plugin.
    pub grants: Vec<PluginGrant>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            root: DEFAULT_PLUGIN_ROOT.to_string(),
            install_from_web: false,
            fuel: 50_000_000,
            grants: Vec::new(),
        }
    }
}

/// Default install root. `/var/lib` rather than `/usr/lib` because a bundle is
/// mutable state that an install writes, not part of a package.
pub const DEFAULT_PLUGIN_ROOT: &str = "/var/lib/anth/plugins";

impl PluginsConfig {
    pub fn root_path(&self) -> PathBuf {
        PathBuf::from(&self.root)
    }

    pub fn grant(&self, id: &str) -> Option<&PluginGrant> {
        self.grants.iter().find(|grant| grant.id == id)
    }

    /// Whether the given capability is currently allowed for `plugin`.
    ///
    /// An uninstalled plugin, a disabled one, or one whose grant does not list
    /// the capability answers `false`. Callers need no second check.
    pub fn allows(&self, plugin: &str, capability: &str) -> bool {
        self.enabled && self.grant(plugin).is_some_and(|g| g.allows(capability))
    }

    /// Installs or replaces one grant. Replacing is what an approve does.
    pub fn upsert_grant(&mut self, grant: PluginGrant) {
        match self.grants.iter_mut().find(|g| g.id == grant.id) {
            Some(existing) => *existing = grant,
            None => self.grants.push(grant),
        }
    }

    pub fn remove_grant(&mut self, id: &str) -> Option<PluginGrant> {
        let index = self.grants.iter().position(|grant| grant.id == id)?;
        Some(self.grants.remove(index))
    }
}

/// The installed set, loaded once and refreshed when the config changes.
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    plugins: Vec<InstalledPlugin>,
    problems: Vec<String>,
}

impl PluginRegistry {
    /// Reads every installed bundle and confirms each module against its grant.
    ///
    /// Failures are collected, not propagated: one broken plugin must not stop
    /// the panel from starting, but it must also not disappear silently.
    pub fn load(config: &PluginsConfig) -> Self {
        if !config.enabled {
            return Self::default();
        }
        let (plugins, problems) = bundle::list(&config.root_path(), &config.grants);
        Self { plugins, problems }
    }

    pub fn plugins(&self) -> &[InstalledPlugin] {
        &self.plugins
    }

    /// Problems found while loading, for the operator to read.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    pub fn get(&self, id: &str) -> Option<&InstalledPlugin> {
        self.plugins.iter().find(|plugin| plugin.id == id)
    }

    /// Every UI anchor whose plugin is enabled, paired with its plugin id.
    ///
    /// The shell receives these instead of asking each plugin: an anchor is only
    /// advertised when its plugin loaded, so the UI cannot render a control that
    /// has no server half.
    pub fn ui_anchors(&self) -> Vec<(&str, &UiAnchor)> {
        self.plugins
            .iter()
            .filter(|plugin| plugin.enabled)
            .flat_map(|plugin| {
                plugin
                    .ui
                    .iter()
                    .map(move |anchor| (plugin.id.as_str(), anchor))
            })
            .collect()
    }

    /// Every API anchor whose plugin is enabled, paired with its plugin id.
    pub fn api_anchors(&self) -> Vec<(&str, &ApiAnchor)> {
        self.plugins
            .iter()
            .filter(|plugin| plugin.enabled)
            .flat_map(|plugin| {
                plugin
                    .api
                    .iter()
                    .map(move |anchor| (plugin.id.as_str(), anchor))
            })
            .collect()
    }
}

/// Installs a bundle and returns the grant to persist.
///
/// The caller writes the grant to the config. Splitting it here is deliberate:
/// this function only touches the plugin root, so it can be tested without a
/// config file, and the daemon decides when authority is recorded.
pub fn install_bundle(
    bundle_path: &Path,
    config: &PluginsConfig,
) -> Result<(Installed, PluginGrant), PluginError> {
    if !config.enabled {
        return Err(PluginError::new(
            PluginErrorKind::Denied,
            "plugins are disabled in the configuration",
        ));
    }
    bundle::install(bundle_path, &config.root_path())
}
