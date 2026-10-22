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
pub mod jobs;
pub mod manifest;
pub mod runtime;
pub mod signature;
pub mod web;
pub mod wireguard;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ::serde::{Deserialize, Serialize};

pub use bundle::{BundleInstall, Installed, MAX_BUNDLE_BYTES};
pub use manifest::validate_id;
pub use manifest::{
    ApiAnchor, ArgSpec, ArgType, Capability, InstalledPlugin, Manifest, PluginError,
    PluginErrorKind, PluginGrant, UiAnchor,
};

/// Where installed plugins live, and which of them may do anything.
///
/// Grants live in `<config_dir>/plugins.d/<id>.toml`, not inside the bundle:
/// the bundle is content, the grant file is policy. Keeping policy out of
/// `plugin.root` means rewriting a bundle cannot also rewrite the authority
/// that decides what the module may run.
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
    /// Require a valid Ed25519 signature before a bundle is installed.
    ///
    /// On by default. Turning it off is the deliberate escape hatch for a
    /// development build or a host that has not configured a key yet; every
    /// install then reports that the signature was not verified.
    pub require_signature: bool,
    /// Hex-encoded Ed25519 public key that signs plugin bundles.
    ///
    /// Empty means no key is configured. With `require_signature` on, installs
    /// are refused until both the key and a `plugin.sig` are present.
    pub public_key: String,
    /// Instruction budget for one wasm invocation. Fuel is the only way to stop
    /// a module that loops, since wasm has no wall-clock interrupt.
    pub fuel: u64,
    /// Optional directory for the per-plugin grant files. When absent, the
    /// directory is derived from the config file path (`<config_dir>/plugins.d`).
    #[serde(default)]
    pub grants_dir: Option<String>,
    /// Grants loaded from the grant directory. Not part of `config.toml`.
    #[serde(skip)]
    pub grants: Vec<PluginGrant>,
    /// The old inline `[[plugins.grants]]` block, read once for migration.
    /// Serialization always omits it, so the next save drops the block.
    #[serde(default, skip_serializing, rename = "grants")]
    legacy_grants: Vec<PluginGrant>,
    /// Set when a legacy block was found, so `load_or_create` knows to save the
    /// migrated files and rewrite the main config without it.
    #[serde(skip)]
    needs_grant_save: bool,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            root: DEFAULT_PLUGIN_ROOT.to_string(),
            install_from_web: false,
            require_signature: true,
            public_key: String::new(),
            fuel: 50_000_000,
            grants_dir: None,
            grants: Vec::new(),
            legacy_grants: Vec::new(),
            needs_grant_save: false,
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

    /// Directory that holds one `<id>.toml` grant file per plugin.
    ///
    /// An explicit `grants_dir` wins; otherwise the directory sits next to the
    /// config file. Passing the config path rather than storing a resolved
    /// value keeps a config that is moved (or a test with a temporary path)
    /// pointing at its own directory.
    pub fn grants_dir_for(&self, config_path: &Path) -> PathBuf {
        match self.grants_dir.as_deref().map(str::trim) {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => config_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .join("plugins.d"),
        }
    }

    /// Whether a legacy inline grant block still has to be dropped from the
    /// main config. `Config::load_or_create` saves once when this is true.
    pub fn needs_grant_save(&self) -> bool {
        self.needs_grant_save
    }

    /// Loads `<id>.toml` grants from the directory next to `config_path`.
    ///
    /// A missing directory means "no plugins installed". A malformed file, a
    /// filename that does not match its `id`, a symlink or a duplicate is a
    /// load error: policy silently disappearing is worse than a refused start.
    pub fn load_grants(&mut self, config_path: &Path) -> Result<(), String> {
        let dir = self.grants_dir_for(config_path);
        let mut loaded = Vec::new();
        let mut read_error = None;

        match fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry
                        .map_err(|error| format!("could not read {}: {error}", dir.display()))?;
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                        continue;
                    }
                    let stem = path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| format!("{} has no usable file name", path.display()))?;
                    validate_id(stem)
                        .map_err(|error| format!("{}: {}", path.display(), error.detail))?;
                    let metadata = fs::symlink_metadata(&path).map_err(|error| {
                        format!("could not inspect {}: {error}", path.display())
                    })?;
                    if metadata.file_type().is_symlink() {
                        return Err(format!(
                            "{} is a symlink; grant files must be regular files",
                            path.display()
                        ));
                    }
                    let text = fs::read_to_string(&path)
                        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
                    let grant: PluginGrant = toml::from_str(&text)
                        .map_err(|error| format!("could not parse {}: {error}", path.display()))?;
                    if grant.id != stem {
                        return Err(format!(
                            "{} declares id `{}`; the file name must match",
                            path.display(),
                            grant.id
                        ));
                    }
                    if loaded
                        .iter()
                        .any(|existing: &PluginGrant| existing.id == grant.id)
                    {
                        return Err(format!("duplicate grant for `{}`", grant.id));
                    }
                    loaded.push(grant);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                read_error = Some(format!("could not read {}: {error}", dir.display()));
            }
        }

        if let Some(error) = read_error {
            return Err(error);
        }

        let had_legacy = !self.legacy_grants.is_empty();
        if !loaded.is_empty() {
            if had_legacy {
                eprintln!(
                    "[WARN] {} also contains legacy [plugins.grants]; using the per-plugin files",
                    config_path.display()
                );
            }
            loaded.sort_by(|left, right| left.id.cmp(&right.id));
            self.grants = loaded;
        } else if had_legacy {
            // One-time migration: keep the legacy grants in memory, then save
            // them as files and rewrite the main config without the block.
            self.grants = std::mem::take(&mut self.legacy_grants);
            eprintln!(
                "[WARN] migrating [plugins.grants] to {}/; the main config will be rewritten",
                dir.display()
            );
        }
        self.legacy_grants.clear();
        self.needs_grant_save = had_legacy;
        Ok(())
    }

    /// Writes one grant file per plugin and removes files for grants that no
    /// longer exist.
    pub fn save_grants(&self, config_path: &Path) -> Result<(), String> {
        let dir = self.grants_dir_for(config_path);
        if self.grants.is_empty() {
            if dir.is_dir() {
                remove_stale_grants(&dir, &BTreeSet::new())?;
            }
            return Ok(());
        }

        create_private_dir(&dir)?;
        let mut keep = BTreeSet::new();
        for grant in &self.grants {
            validate_id(&grant.id).map_err(|error| error.detail)?;
            let name = format!("{}.toml", grant.id);
            let path = dir.join(&name);
            let body = toml::to_string_pretty(grant)
                .map_err(|error| format!("could not serialize `{}`: {error}", grant.id))?;
            let body = format!(
                "# Written by anth-panel. This is the frozen authority for the plugin.\n                 # Change it with `anth-panel plugin approve`, not by hand.\n{body}"
            );
            crate::config::write_private_atomic(&path, &body)
                .map_err(|error| format!("could not write {}: {error}", path.display()))?;
            keep.insert(name);
        }
        remove_stale_grants(&dir, &keep)
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

fn create_private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    // `mode` applies only on creation; tighten a directory made by an older
    // build or by hand.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not tighten {}: {error}", dir.display()))
}

fn remove_stale_grants(dir: &Path, keep: &BTreeSet<String>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not read {}: {error}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read {}: {error}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if keep.contains(name) {
            continue;
        }
        fs::remove_file(&path)
            .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
    }
    Ok(())
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
) -> Result<BundleInstall, PluginError> {
    if !config.enabled {
        return Err(PluginError::new(
            PluginErrorKind::Denied,
            "plugins are disabled in the configuration",
        ));
    }
    bundle::install(
        bundle_path,
        &config.root_path(),
        config.require_signature,
        &config.public_key,
    )
}
