//! The manifest: what a bundle declares, and what the host refuses to accept.
//!
//! A manifest is data a third party ships, so every field here is untrusted. The
//! rule the whole module follows: a value is parsed into a type that cannot hold
//! a bad one, and a validation failure names the field. Nothing is repaired
//! silently — a manifest the host had to guess at is a manifest it cannot claim
//! to have enforced.

use std::collections::HashSet;

use anth_wire::WireError;
use serde::Deserialize;

/// The ABI this daemon can drive.
///
/// Must equal `anth_plugin::ABI_VERSION`. A bundle declaring another version is
/// refused by name rather than trapped at the first call, so an upgrade is
/// diagnosable from the error alone.
pub const ABI_VERSION: u32 = 2;

/// Programs a capability may name.
///
/// The allowlist is the authority, not the manifest: a manifest cannot introduce
/// a program the host does not know. `hostname` is here because it is what the
/// example plugin needs and it is read-only.
pub const PROGRAM_ALLOWLIST: &[&str] = &["hostname", "uname", "id"];

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub plugin: PluginSection,
    pub wasm: WasmSection,
    #[serde(default)]
    pub capability: Vec<Capability>,
    #[serde(default)]
    pub api: Vec<Anchor>,
    #[serde(default)]
    pub ui: Vec<UiSlot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginSection {
    pub id: String,
    /// A short name for this plugin, usable wherever an id is.
    ///
    /// Typing `plugin call org.anth.hello greet` is a mouthful, and the id has to
    /// stay globally unique, so a bundle may declare one short handle. It is
    /// resolved by the registry and never reaches a path, which is why it is
    /// validated separately from the id.
    #[serde(default)]
    pub alias: Option<String>,
    pub version: String,
    pub name: String,
    pub abi: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WasmSection {
    /// Always `plugin.wasm` after validation. Kept as a field so the manifest
    /// says what it means, and pinned so a rewritten manifest cannot re-point
    /// the module at another file in the bundle.
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Capability {
    pub id: String,
    pub program: String,
    /// Values the guest may supply, by name and type. Empty means "no arguments
    /// at all", which is the safe default: a guest that cannot name a value
    /// cannot add a flag to the argv.
    #[serde(default)]
    pub accept: Vec<AcceptedArg>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AcceptedArg {
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: ArgType,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgType {
    /// Anything, passed as one argv element. Never split, so a value cannot
    /// become two arguments.
    #[default]
    String,
    /// A decimal integer, passed as one argv element.
    Integer,
    /// A flag the caller may set, contributing no argv element of its own.
    Switch,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Anchor {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiSlot {
    pub slot: String,
    pub tag: String,
    pub module: String,
    pub label: Option<String>,
}

impl Manifest {
    /// Parses and validates a manifest.
    ///
    /// Every check here is one the host relies on later, which is why they run
    /// at load: a manifest that passes is one the runtime can index without
    /// re-checking, and a bundle that fails is never instantiated.
    pub fn parse(text: &str) -> Result<Self, WireError> {
        let manifest = toml::from_str::<Self>(text)
            .map_err(|e| WireError::invalid(format!("the manifest does not parse: {e}")))?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), WireError> {
        validate_id(&self.plugin.id)?;
        if let Some(alias) = &self.plugin.alias {
            validate_alias(alias)?;
        }
        if self.plugin.version.trim().is_empty() {
            return Err(WireError::invalid("the manifest declares no version"));
        }
        if self.plugin.name.trim().is_empty() {
            return Err(WireError::invalid("the manifest declares no name"));
        }
        if self.plugin.abi != ABI_VERSION {
            return Err(WireError::invalid(format!(
                "plugin `{}` speaks ABI {}, this daemon speaks {ABI_VERSION}",
                self.plugin.id, self.plugin.abi
            )));
        }
        if self.wasm.path != "plugin.wasm" {
            return Err(WireError::invalid(format!(
                "the module path is pinned to `plugin.wasm`, the manifest names `{}`",
                self.wasm.path
            )));
        }
        validate_sha256(&self.wasm.sha256)?;

        // A duplicate anchor is refused rather than deduplicated: two identical
        // routes would make "which one answered" depend on iteration order.
        let mut seen = HashSet::new();
        for anchor in &self.api {
            validate_method(&anchor.method)?;
            if !anchor.path.starts_with('/') {
                return Err(WireError::invalid(format!(
                    "the anchor `{}` must start with a slash",
                    anchor.path
                )));
            }
            if !seen.insert((anchor.method.to_uppercase(), anchor.path.clone())) {
                return Err(WireError::invalid(format!(
                    "the anchor {} {} is declared twice",
                    anchor.method, anchor.path
                )));
            }
        }

        let mut capabilities = HashSet::new();
        for capability in &self.capability {
            if !capabilities.insert(capability.id.clone()) {
                return Err(WireError::invalid(format!(
                    "the capability `{}` is declared twice",
                    capability.id
                )));
            }
            if !PROGRAM_ALLOWLIST.contains(&capability.program.as_str()) {
                return Err(WireError::invalid(format!(
                    "the capability `{}` names the program `{}`, which is not on the host allowlist ({})",
                    capability.id,
                    capability.program,
                    PROGRAM_ALLOWLIST.join(", ")
                )));
            }
            let mut names = HashSet::new();
            for accepted in &capability.accept {
                if !names.insert(accepted.name.clone()) {
                    return Err(WireError::invalid(format!(
                        "the capability `{}` accepts `{}` twice",
                        capability.id, accepted.name
                    )));
                }
            }
        }

        for slot in &self.ui {
            if slot.module.contains("..") || slot.module.starts_with('/') {
                return Err(WireError::invalid(format!(
                    "the ui module `{}` must be a relative path inside the bundle",
                    slot.module
                )));
            }
            if slot.tag != slot.tag.to_lowercase() || !slot.tag.contains('-') {
                return Err(WireError::invalid(format!(
                    "the ui tag `{}` must be a lowercase custom-element name",
                    slot.tag
                )));
            }
        }
        Ok(())
    }

    /// The capability a guest may run, if the manifest froze one under that id.
    pub fn capability(&self, id: &str) -> Option<&Capability> {
        self.capability.iter().find(|c| c.id == id)
    }
}

/// A plugin id is a dotted lowercase name, which is also what `package.sh`
/// writes. Restricting it here means a path built from an id cannot escape.
fn validate_id(id: &str) -> Result<(), WireError> {
    if id.is_empty() {
        return Err(WireError::invalid("the manifest declares no id"));
    }
    let well_formed = id.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    });
    if !well_formed || !id.contains('.') {
        return Err(WireError::invalid(format!(
            "`{id}` is not a plugin id: use lowercase dot-separated parts, like org.example.thing"
        )));
    }
    Ok(())
}

/// A short handle for a plugin, usable wherever an id is.
///
/// Deliberately stricter than the id: lowercase, no dots, and it must not look
/// like an id the registry could confuse it with. It never reaches a filesystem
/// path or a URL, but a short lowercase word is the whole point, so anything
/// else is a typo rather than a name.
fn validate_alias(alias: &str) -> Result<(), WireError> {
    if alias.is_empty() {
        return Err(WireError::invalid(
            "the manifest declares an empty alias; omit the field instead",
        ));
    }
    let well_formed = alias
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !well_formed {
        return Err(WireError::invalid(format!(
            "`{alias}` is not a plugin alias: use lowercase letters, digits, `-` and `_`"
        )));
    }
    Ok(())
}

fn validate_method(method: &str) -> Result<(), WireError> {
    match method.to_uppercase().as_str() {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => Ok(()),
        other => Err(WireError::invalid(format!(
            "`{other}` is not a method an anchor can use"
        ))),
    }
}

fn validate_sha256(digest: &str) -> Result<(), WireError> {
    let all_zero = digest.chars().all(|c| c == '0');
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(WireError::invalid(
            "the manifest needs a 64-character hex sha256 of the module",
        ));
    }
    if all_zero {
        return Err(WireError::invalid(
            "the module digest is all zeroes: run package.sh to fill it in",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
