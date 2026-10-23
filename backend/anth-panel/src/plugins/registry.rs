//! The plugins the daemon knows about, and the check that keeps them honest.
//!
//! A bundle is extracted to a directory and never edited in place, but nothing
//! stops someone rewriting a file inside it. So the digest is re-checked on
//! every load: if the module no longer matches the one the operator approved,
//! the plugin is *disabled* rather than trusted. That is the difference between
//! "we hashed it once" and "we know what we are running".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anth_wire::{ErrorKind, UiSlotSummary, WireError};
use sha2::{Digest, Sha256};

use super::manifest::Manifest;
use super::runtime::Loaded;

/// One plugin as the daemon sees it: either loaded, or disabled with a reason.
///
/// A disabled plugin stays in the registry so `plugin list` can say why, which is
/// the difference between a misconfiguration an operator can fix and a plugin
/// that silently does not exist.
#[derive(Debug)]
pub struct Entry {
    pub id: String,
    /// The short handle the manifest declared, if any. Resolved by [`Registry::get`].
    pub alias: Option<String>,
    pub name: String,
    pub version: String,
    pub anchors: Vec<(String, String)>,
    /// What the bundle asks the shell to mount, and where its module lives.
    ///
    /// Empty for a disabled plugin: the manifest did not survive validation, so
    /// what it asked to mount is not something the daemon should repeat as if it
    /// were verified. The anchors above are kept for the opposite reason — they
    /// explain why the entry exists at all.
    pub ui: Vec<UiSlotSummary>,
    /// The directory the bundle was read from, so a declared module can be
    /// resolved to a file inside it.
    pub dir: PathBuf,
    pub enabled: Result<(), String>,
    loaded: Option<Loaded>,
}

impl Entry {
    pub fn loaded(&self) -> Option<&Loaded> {
        self.loaded.as_ref()
    }

    pub fn usable(&self) -> Result<&Loaded, WireError> {
        match &self.loaded {
            Some(loaded) => Ok(loaded),
            None => Err(WireError::new(
                ErrorKind::Forbidden,
                format!(
                    "plugin `{}` is disabled: {}",
                    self.id,
                    self.enabled
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("no reason recorded")
                ),
            )),
        }
    }
}

/// Every plugin found at startup, keyed by id.
pub struct Registry {
    entries: BTreeMap<String, Entry>,
}

impl Registry {
    /// An empty registry, for a daemon started without a plugin directory.
    ///
    /// Only tests build one: a real daemon always goes through `load`, and the
    /// empty registry would silently serve no plugins if it were reachable from
    /// production code.
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// One disabled entry, for tests that need a resolvable id without a module.
    ///
    /// A disabled plugin is enough to prove a lookup landed on the right one:
    /// the refusal it produces names its own canonical id.
    #[cfg(test)]
    pub fn with_disabled(
        id: &str,
        alias: Option<&str>,
        anchors: &[(&str, &str)],
        reason: &str,
    ) -> Self {
        let mut entries = BTreeMap::new();
        entries.insert(
            id.to_string(),
            Entry {
                id: id.to_string(),
                alias: alias.map(str::to_string),
                name: id.to_string(),
                version: String::new(),
                anchors: anchors
                    .iter()
                    .map(|(method, path)| (method.to_string(), path.to_string()))
                    .collect(),
                ui: Vec::new(),
                dir: PathBuf::new(),
                enabled: Err(reason.to_string()),
                loaded: None,
            },
        );
        Self { entries }
    }

    /// Loads every bundle under `root`.
    ///
    /// A bundle that fails to load does not abort the daemon: it becomes a
    /// disabled entry that names the reason. One wrong plugin must not stop the
    /// panel from serving the rest, but it must also never be silently skipped.
    pub fn load(root: &Path) -> Self {
        let mut entries = BTreeMap::new();
        let Ok(read) = std::fs::read_dir(root) else {
            return Self { entries };
        };

        for candidate in read.flatten() {
            let path = candidate.path();
            if !path.is_dir() {
                continue;
            }
            let entry = load_one(&path);
            entries.insert(entry.id.clone(), entry);
        }
        Self { entries }
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// The plugin a caller named, by exact id first and then by alias.
    ///
    /// Two plugins may declare one alias, and picking by directory order would
    /// make the answer depend on the filesystem. Following `anchor_owner`, an
    /// ambiguous alias is a conflict the operator resolves, not a coin flip.
    pub fn resolve(&self, id_or_alias: &str) -> Result<Option<&Entry>, WireError> {
        if let Some(entry) = self.entries.get(id_or_alias) {
            return Ok(Some(entry));
        }

        let mut matches = self
            .entries
            .values()
            .filter(|entry| entry.alias.as_deref() == Some(id_or_alias));
        let Some(first) = matches.next() else {
            return Ok(None);
        };
        if let Some(second) = matches.next() {
            return Err(WireError::new(
                ErrorKind::Conflict,
                format!(
                    "both `{}` and `{}` declare the alias `{id_or_alias}`; use the full id",
                    first.id, second.id
                ),
            ));
        }
        Ok(Some(first))
    }

    /// The file a bundle declared as a UI module.
    ///
    /// Resolved against `ui[].module` rather than against the bundle
    /// directory, so the only files a request can reach are the ones the
    /// manifest named. A bundle holds `plugin.wasm` and `plugin.toml` beside
    /// its web assets, and serving "whatever is under the bundle" would hand
    /// both to any caller that guessed the name. An exact allowlist makes that
    /// unreachable by construction rather than by filtering afterwards.
    ///
    /// Both sides are normalised to the same shape first, so a redundant but
    /// harmless spelling still matches what the manifest declared. A path that
    /// cannot be normalised matches nothing: `normalise` refuses `..` outright
    /// rather than resolving it, and the same goes for an absolute path or a
    /// dotfile, so `web/../plugin.wasm` is `None` before the allowlist is even
    /// consulted.
    pub fn ui_module(&self, id_or_alias: &str, path: &str) -> Result<Option<&Entry>, WireError> {
        let Some(entry) = self.resolve(id_or_alias)? else {
            return Ok(None);
        };
        // A disabled bundle never got as far as a validated manifest, so it has
        // no declared modules to check against. Answering `404` would say the
        // module is not declared, when the truth is that nothing in the bundle
        // could be trusted — the reason the entry carries is the useful answer.
        entry.usable()?;
        let Some(wanted) = crate::www::normalise(path) else {
            return Ok(None);
        };
        let declared = entry
            .ui
            .iter()
            .any(|slot| crate::www::normalise(&slot.module).as_deref() == Some(wanted.as_str()));
        Ok(declared.then_some(entry))
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The plugin that declares an anchor, if exactly one does.
    ///
    /// Two plugins claiming one route is a conflict the operator has to resolve,
    /// so it is reported rather than resolved by directory order.
    pub fn anchor_owner(
        &self,
        method: &str,
        path: &str,
    ) -> Result<Option<(&Entry, &Loaded)>, WireError> {
        let mut owners = self.entries.values().filter(|entry| {
            entry
                .anchors
                .iter()
                .any(|(m, p)| m.eq_ignore_ascii_case(method) && p == path)
        });

        let Some(first) = owners.next() else {
            return Ok(None);
        };
        if let Some(second) = owners.next() {
            return Err(WireError::new(
                ErrorKind::Conflict,
                format!(
                    "both `{}` and `{}` declare {method} {path}",
                    first.id, second.id
                ),
            ));
        }
        match first.loaded() {
            Some(loaded) => Ok(Some((first, loaded))),
            None => Err(WireError::new(
                ErrorKind::Forbidden,
                format!(
                    "plugin `{}` declares {method} {path} but is disabled: {}",
                    first.id,
                    first
                        .enabled
                        .as_ref()
                        .err()
                        .map(String::as_str)
                        .unwrap_or("")
                ),
            )),
        }
    }
}

/// Reads one bundle directory, verifying it before anything runs.
fn load_one(dir: &Path) -> Entry {
    let id = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unnamed>".to_string());

    let mut entry = Entry {
        id: id.clone(),
        alias: None,
        name: id.clone(),
        version: String::new(),
        anchors: Vec::new(),
        ui: Vec::new(),
        dir: dir.to_path_buf(),
        enabled: Ok(()),
        loaded: None,
    };

    match try_load(dir) {
        Ok((manifest, loaded)) => {
            entry.alias = manifest.plugin.alias.clone();
            entry.name = manifest.plugin.name;
            entry.version = manifest.plugin.version;
            entry.anchors = manifest
                .api
                .iter()
                .map(|anchor| (anchor.method.to_uppercase(), anchor.path.clone()))
                .collect();
            entry.ui = manifest
                .ui
                .iter()
                .map(|slot| UiSlotSummary {
                    slot: slot.slot.clone(),
                    tag: slot.tag.clone(),
                    module: slot.module.clone(),
                    label: slot.label.clone(),
                })
                .collect();
            entry.loaded = Some(loaded);
        }
        Err(reason) => entry.enabled = Err(reason),
    }
    entry
}

fn try_load(dir: &Path) -> Result<(Manifest, Loaded), String> {
    let manifest_path = dir.join("plugin.toml");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let manifest = Manifest::parse(&text).map_err(|e| e.message)?;

    let module_path = dir.join("plugin.wasm");
    let wasm = std::fs::read(&module_path)
        .map_err(|e| format!("reading {}: {e}", module_path.display()))?;

    // The digest is the check that matters: the bytes about to be instantiated
    // are the bytes the operator approved, even if the file was rewritten since.
    let digest = hex(&Sha256::digest(&wasm));
    if digest != manifest.wasm.sha256.to_lowercase() {
        return Err(format!(
            "the module digest is {digest}, the manifest approves {}. The bundle was rewritten; reinstall it to approve the new bytes.",
            manifest.wasm.sha256
        ));
    }

    let loaded = Loaded::load(manifest.clone(), &wasm).map_err(|e| e.message)?;
    Ok((manifest, loaded))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// Reads one declared UI module out of its bundle.
///
/// The allowlist in [`Registry::ui_module`] decides *which* file may be read;
/// this decides *where*, and re-checks the answer. A bundle could ship a
/// symlink for the path its manifest declares, and canonicalising both sides
/// then catches it escaping: the allowlist names a path inside the bundle, not
/// a file, and the two are only the same thing if the link does not move.
pub fn read_ui_module(entry: &Entry, module: &str) -> Option<Vec<u8>> {
    let base = entry.dir.canonicalize().ok()?;
    let resolved = entry.dir.join(module).canonicalize().ok()?;
    if !resolved.starts_with(&base) || !resolved.is_file() {
        return None;
    }
    std::fs::read(resolved).ok()
}

/// The directory the daemon reads plugins from.
pub fn default_root() -> PathBuf {
    match std::env::var("ANTH_PLUGINS") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from("/var/lib/anth/plugins"),
    }
}
