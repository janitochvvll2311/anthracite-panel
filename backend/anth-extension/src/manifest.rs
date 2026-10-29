//! The extension manifest: the only contract a bundle gets to declare.
//!
//! A manifest is data from a third party, so every field is untrusted. Values
//! are parsed into types that cannot hold a bad one, and a refusal names the
//! field. Nothing is repaired silently: a manifest the host had to guess at is
//! one it cannot claim to have enforced.
//!
//! The authority boundary is the capability. A extension declares a program and an
//! argv template whose only variable parts are typed arguments; it never sends
//! a finished argv. The host validates every value and renders the vector.

use std::collections::{BTreeMap, HashSet};

use anth_wire::WireError;
use serde::{Deserialize, Serialize};

/// Largest manifest accepted. A manifest is a small recipe; a megabyte already
/// bounds the parser's work.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Largest number of capabilities, anchors or UI slots one extension may declare.
pub const MAX_ENTRIES: usize = 256;

/// The manifest schema this host understands.
pub const MANIFEST_VERSION: u32 = 2;

/// The guest ABI this host drives. Must match `anth_plugin::ABI_VERSION`.
pub const ABI_VERSION: u32 = 3;

/// The module path inside a bundle is pinned, never read from a rewritten
/// manifest at load time.
pub const MANIFEST_FILE: &str = "manifest.toml";
pub const WASM_MANIFEST_FILE: &str = "wasm/manifest.toml";
pub const WEB_MANIFEST_FILE: &str = "web/manifest.toml";
pub const SERVICE_MANIFEST_FILE: &str = "service/manifest.toml";
pub const SIGNATURE_FILE: &str = "manifest.sig";
pub const WASM_FILE: &str = "wasm/extension.wasm";

/// Files that make up one manifest set. The root file is required; layer files
/// are optional and may be mixed with inline sections.
pub const MANIFEST_FILES: [&str; 4] = [
    MANIFEST_FILE,
    WASM_MANIFEST_FILE,
    WEB_MANIFEST_FILE,
    SERVICE_MANIFEST_FILE,
];

/// Programs a capability may name. The allowlist is the authority: a manifest
/// cannot introduce a program the host does not know.
pub const PROGRAM_ALLOWLIST: &[&str] = &["hostname", "uname", "id", "wg"];

/// Longest one rendered argv element may be.
pub const MAX_ARG_LEN: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    pub manifest_version: u32,
    pub extension: ExtensionSection,
    pub runtime: RuntimeSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm: Option<WasmSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceSection>,
    #[serde(default)]
    pub capability: Vec<Capability>,
    #[serde(default)]
    pub api: Vec<Anchor>,
    #[serde(default)]
    pub ui: Vec<UiSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSection {
    pub kind: RuntimeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Wasm,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSection {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub version: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmSection {
    pub path: String,
    pub sha256: String,
    pub abi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSection {
    pub socket: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub id: String,
    pub program: String,
    /// The argv template. Literal elements stay as written; `{name}` is
    /// replaced by one validated value from `accept`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Values the guest may supply, by name and type. Empty means "no
    /// arguments at all", which is the safe default.
    #[serde(default)]
    pub accept: Vec<AcceptedArg>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedArg {
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: ArgType,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgType {
    /// One argv element. Never split, so a space cannot become two arguments.
    #[default]
    String,
    /// A decimal integer, rendered as one element.
    Integer,
    /// A flag that contributes no element of its own.
    Switch,
    /// A Linux interface name: at most 15 bytes, no slash or whitespace.
    Ifname,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSlot {
    pub slot: String,
    pub tag: String,
    pub module: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub sha256: String,
}

impl ExtensionManifest {
    /// Parses and validates a manifest, then sorts its collections so two
    /// manifests that differ only in declaration order compare equal.
    pub fn parse(text: &str) -> Result<Self, WireError> {
        let mut files = BTreeMap::new();
        files.insert(MANIFEST_FILE.to_string(), text.as_bytes().to_vec());
        Self::parse_files(&files)
    }

    /// Parses the root file and merges the optional layer manifests.
    ///
    /// A layer manifest uses paths relative to its own directory. The loader
    /// rewrites them to bundle-relative paths once, so the rest of the host only
    /// sees one canonical manifest.
    pub fn parse_files(files: &BTreeMap<String, Vec<u8>>) -> Result<Self, WireError> {
        let core = files
            .get(MANIFEST_FILE)
            .ok_or_else(|| WireError::invalid(format!("the bundle has no `{MANIFEST_FILE}`")))?;
        if core.len() > MAX_MANIFEST_BYTES {
            return Err(WireError::too_large(format!(
                "the manifest is larger than {MAX_MANIFEST_BYTES} bytes"
            )));
        }
        let text = std::str::from_utf8(core)
            .map_err(|_| WireError::invalid(format!("`{MANIFEST_FILE}` is not UTF-8")))?;
        let mut manifest = toml::from_str::<Self>(text)
            .map_err(|error| WireError::invalid(format!("the manifest does not parse: {error}")))?;

        if let Some(bytes) = files.get(WASM_MANIFEST_FILE) {
            let part: WasmLayer = parse_layer(bytes, WASM_MANIFEST_FILE)?;
            if let Some(mut wasm) = part.wasm {
                if manifest.wasm.is_some() {
                    return Err(WireError::invalid(
                        "the wasm section is declared in both the root and the layer",
                    ));
                }
                if !wasm.path.starts_with("wasm/") {
                    wasm.path = format!("wasm/{}", wasm.path);
                }
                manifest.wasm = Some(wasm);
            }
            manifest.capability.extend(part.capability);
        }
        if let Some(bytes) = files.get(WEB_MANIFEST_FILE) {
            let part: WebLayer = parse_layer(bytes, WEB_MANIFEST_FILE)?;
            manifest.api.extend(part.api);
            for mut slot in part.ui {
                if !slot.module.starts_with("web/") {
                    slot.module = format!("web/{}", slot.module);
                }
                manifest.ui.push(slot);
            }
        }
        if let Some(bytes) = files.get(SERVICE_MANIFEST_FILE) {
            let part: ServiceLayer = parse_layer(bytes, SERVICE_MANIFEST_FILE)?;
            if manifest.service.is_some() {
                return Err(WireError::invalid(
                    "the service section is declared in both the root and the layer",
                ));
            }
            manifest.service = Some(part.service);
        }
        manifest.validate()?;
        manifest.normalize();
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), WireError> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(WireError::invalid(format!(
                "the manifest speaks version {}, this host speaks {MANIFEST_VERSION}",
                self.manifest_version
            )));
        }
        validate_id(&self.extension.id)?;
        if let Some(alias) = &self.extension.alias {
            validate_alias(alias)?;
        }
        if self.extension.version.trim().is_empty() {
            return Err(WireError::invalid("the manifest declares no version"));
        }
        if self.extension.name.trim().is_empty() {
            return Err(WireError::invalid("the manifest declares no name"));
        }
        match self.runtime.kind {
            RuntimeKind::Wasm => {
                if self.service.is_some() {
                    return Err(WireError::invalid(
                        "a wasm extension cannot declare a service section",
                    ));
                }
                let wasm = self.wasm.as_ref().ok_or_else(|| {
                    WireError::invalid("a wasm extension needs a `[wasm]` section")
                })?;
                if wasm.path != WASM_FILE {
                    return Err(WireError::invalid(format!(
                        "the module path is pinned to `{WASM_FILE}`, the manifest names `{}`",
                        wasm.path
                    )));
                }
                if wasm.abi != ABI_VERSION {
                    return Err(WireError::invalid(format!(
                        "extension `{}` speaks ABI {}, this daemon speaks {ABI_VERSION}",
                        self.extension.id, wasm.abi
                    )));
                }
                validate_sha256(&wasm.sha256, "the module")?;
            }
            RuntimeKind::Service => {
                if self.wasm.is_some() {
                    return Err(WireError::invalid(
                        "a service extension cannot declare a wasm section",
                    ));
                }
                if !self.capability.is_empty() {
                    return Err(WireError::invalid(
                        "a service extension declares routes, not capabilities",
                    ));
                }
                let service = self.service.as_ref().ok_or_else(|| {
                    WireError::invalid("a service extension needs a `[service]` section")
                })?;
                if !service.socket.starts_with('/')
                    || service.socket.contains("..")
                    || service.socket.len() > 256
                {
                    return Err(WireError::invalid(
                        "a service socket must be an absolute path without `..`",
                    ));
                }
            }
        }

        if self.capability.len() > MAX_ENTRIES
            || self.api.len() > MAX_ENTRIES
            || self.ui.len() > MAX_ENTRIES
        {
            return Err(WireError::invalid(format!(
                "a extension may declare at most {MAX_ENTRIES} capabilities, anchors and UI slots"
            )));
        }

        let mut capability_ids = HashSet::new();
        for capability in &self.capability {
            validate_capability_id(&capability.id)?;
            if !capability_ids.insert(capability.id.clone()) {
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
                if accepted.name.is_empty()
                    || accepted.name.len() > 64
                    || !accepted
                        .name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(WireError::invalid(format!(
                        "the capability `{}` accepts an invalid argument name `{}`",
                        capability.id, accepted.name
                    )));
                }
                if !names.insert(accepted.name.clone()) {
                    return Err(WireError::invalid(format!(
                        "the capability `{}` accepts `{}` twice",
                        capability.id, accepted.name
                    )));
                }
            }
            let mut used = HashSet::new();
            for argument in &capability.args {
                if argument.is_empty() || argument.len() > MAX_ARG_LEN {
                    return Err(WireError::invalid(format!(
                        "the capability `{}` has an empty or oversized argv element",
                        capability.id
                    )));
                }
                if argument.chars().any(|c| c.is_control() || c == '\0') {
                    return Err(WireError::invalid(format!(
                        "the capability `{}` has a control character in its argv template",
                        capability.id
                    )));
                }
                for name in placeholders(argument)? {
                    if !names.contains(&name) {
                        return Err(WireError::invalid(format!(
                            "the capability `{}` uses `{{{name}}}` but does not accept it",
                            capability.id
                        )));
                    }
                    used.insert(name);
                }
            }
            for name in &names {
                if !used.contains(name) {
                    return Err(WireError::invalid(format!(
                        "the capability `{}` accepts `{name}` but never uses it in argv",
                        capability.id
                    )));
                }
            }
        }

        let mut anchors = HashSet::new();
        for anchor in &self.api {
            let method = validate_method(&anchor.method)?;
            validate_anchor_path(&anchor.path)?;
            if !anchors.insert((method, anchor.path.clone())) {
                return Err(WireError::invalid(format!(
                    "the anchor {} {} is declared twice",
                    anchor.method, anchor.path
                )));
            }
        }

        let mut slots = HashSet::new();
        let mut modules = std::collections::HashMap::new();
        for slot in &self.ui {
            validate_id(&slot.slot)?;
            if slot.tag != slot.tag.to_lowercase() || !slot.tag.contains('-') {
                return Err(WireError::invalid(format!(
                    "the ui tag `{}` must be a lowercase custom-element name",
                    slot.tag
                )));
            }
            validate_web_module(&slot.module)?;
            validate_sha256(&slot.sha256, "the UI module")?;
            if let Some(previous) = modules.insert(slot.module.clone(), slot.sha256.clone())
                && previous != slot.sha256
            {
                return Err(WireError::invalid(format!(
                    "the module `{}` is declared with two different digests",
                    slot.module
                )));
            }
            if !slots.insert(slot.slot.clone()) {
                return Err(WireError::invalid(format!(
                    "the ui slot `{}` is declared twice",
                    slot.slot
                )));
            }
            if slot
                .label
                .as_ref()
                .is_some_and(|label| label.trim().is_empty())
            {
                return Err(WireError::invalid("a ui label cannot be blank"));
            }
        }
        Ok(())
    }

    fn normalize(&mut self) {
        self.capability.sort_by(|a, b| a.id.cmp(&b.id));
        for capability in &mut self.capability {
            capability.accept.sort_by(|a, b| a.name.cmp(&b.name));
        }
        self.api
            .sort_by(|a, b| (&a.method, &a.path).cmp(&(&b.method, &b.path)));
        self.ui.sort_by(|a, b| a.slot.cmp(&b.slot));
    }

    /// The extension surface an operator approves. Hashes and versions are not
    /// part of it: a reinstall with the same programs and routes but a new build
    /// does not need a fresh approval.
    pub fn surface(&self) -> ExtensionSurface {
        ExtensionSurface {
            runtime: self.runtime.kind,
            service_socket: self.service.as_ref().map(|service| service.socket.clone()),
            capability: self.capability.clone(),
            api: self.api.clone(),
            ui: self.ui.clone(),
        }
    }

    /// Every distinct `web/` module and its declared digest.
    pub fn web_modules(&self) -> Vec<(String, String)> {
        let mut modules: Vec<(String, String)> = self
            .ui
            .iter()
            .map(|slot| (slot.module.clone(), slot.sha256.clone()))
            .collect();
        modules.sort();
        modules.dedup();
        modules
    }
}

/// The part of a manifest a grant freezes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionSurface {
    pub runtime: RuntimeKind,
    pub service_socket: Option<String>,
    pub capability: Vec<Capability>,
    pub api: Vec<Anchor>,
    pub ui: Vec<UiSlot>,
}

/// A extension id is a dotted lowercase name, which is also what a bundle
/// directory is called. Restricting it here means a path built from an id
/// cannot escape.
pub fn validate_id(id: &str) -> Result<(), WireError> {
    if id.is_empty() || id.len() > 128 {
        return Err(WireError::invalid(
            "a extension id must be 1 to 128 characters",
        ));
    }
    let well_formed = id.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    });
    if !well_formed || !id.contains('.') {
        return Err(WireError::invalid(format!(
            "`{id}` is not a extension id: use lowercase dot-separated parts, like org.example.thing"
        )));
    }
    Ok(())
}

fn validate_capability_id(id: &str) -> Result<(), WireError> {
    validate_id(id).map_err(|_| {
        WireError::invalid(format!(
            "`{id}` is not a capability id: use lowercase dot-separated parts"
        ))
    })
}

fn validate_alias(alias: &str) -> Result<(), WireError> {
    if alias.is_empty() || alias.len() > 64 {
        return Err(WireError::invalid(
            "a extension alias must be 1 to 64 characters; omit the field instead",
        ));
    }
    if !alias
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(WireError::invalid(format!(
            "`{alias}` is not a extension alias: use lowercase letters, digits, `-` and `_`"
        )));
    }
    Ok(())
}

fn validate_method(method: &str) -> Result<String, WireError> {
    let method = method.to_uppercase();
    match method.as_str() {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" => Ok(method),
        other => Err(WireError::invalid(format!(
            "`{other}` is not a method an anchor can use"
        ))),
    }
}

fn validate_anchor_path(path: &str) -> Result<(), WireError> {
    if !path.starts_with('/') || path.len() > 256 {
        return Err(WireError::invalid(format!(
            "the anchor path `{path}` must start with `/` and be at most 256 characters"
        )));
    }
    if path.contains("..")
        || path.contains('?')
        || path.contains('#')
        || path.chars().any(|c| c.is_control() || c == '\\')
    {
        return Err(WireError::invalid(format!(
            "the anchor path `{path}` contains a component the host refuses"
        )));
    }
    Ok(())
}

fn validate_web_module(module: &str) -> Result<(), WireError> {
    if !module.starts_with("web/") || module.len() <= 4 {
        return Err(WireError::invalid(format!(
            "the UI module `{module}` must live under `web/`"
        )));
    }
    if module
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(WireError::invalid(format!(
            "the UI module `{module}` must be a plain path inside the bundle"
        )));
    }
    let allowed = module.ends_with(".js") || module.ends_with(".mjs") || module.ends_with(".css");
    if !allowed {
        return Err(WireError::invalid(format!(
            "the UI module `{module}` must end in `.js`, `.mjs` or `.css`"
        )));
    }
    if module.contains('\\') {
        return Err(WireError::invalid(format!(
            "the UI module `{module}` must use `/` separators"
        )));
    }
    Ok(())
}

fn validate_sha256(digest: &str, what: &str) -> Result<(), WireError> {
    let all_zero = digest.chars().all(|c| c == '0');
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(WireError::invalid(format!(
            "{what} needs a 64-character hex sha256"
        )));
    }
    if all_zero {
        return Err(WireError::invalid(format!(
            "{what} digest is all zeroes: run the packaging script to fill it in"
        )));
    }
    Ok(())
}

/// Extracts the `{name}` placeholders from one argv element.
fn placeholders(argument: &str) -> Result<Vec<String>, WireError> {
    let mut names = Vec::new();
    let mut rest = argument;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return Err(WireError::invalid(format!(
                "the argv element `{argument}` has an unclosed placeholder"
            )));
        };
        let name = &after[..close];
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(WireError::invalid(format!(
                "the argv element `{argument}` has an invalid placeholder"
            )));
        }
        names.push(name.to_string());
        rest = &after[close + 1..];
    }
    if rest.contains('}') {
        return Err(WireError::invalid(format!(
            "the argv element `{argument}` has an unmatched `}}`"
        )));
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn manifest() -> String {
        format!(
            r#"
manifest_version = 2

[extension]
id = "org.anth.hello"
alias = "hello"
version = "0.3.0"
name = "Hello"

[runtime]
kind = "wasm"

[wasm]
path = "wasm/extension.wasm"
abi = 3
sha256 = "{}"

[[capability]]
id = "host.hostname"
program = "hostname"

[[api]]
method = "GET"
path = "/status"

[[ui]]
slot = "dashboard.widget"
tag = "anth-hello"
module = "web/hello.js"
sha256 = "{}"
"#,
            digest('a'),
            digest('b')
        )
    }

    #[test]
    fn a_valid_manifest_parses_and_normalizes() {
        let manifest = ExtensionManifest::parse(&manifest()).expect("manifest");
        assert_eq!(manifest.extension.id, "org.anth.hello");
        assert_eq!(manifest.extension.alias.as_deref(), Some("hello"));
        assert_eq!(manifest.api[0].method, "GET");
        assert_eq!(manifest.web_modules().len(), 1);
    }

    #[test]
    fn ids_aliases_and_versions_are_validated() {
        assert!(validate_id("org.anth.hello").is_ok());
        assert!(validate_id("hello").is_err());
        assert!(validate_id("org..hello").is_err());
        assert!(validate_id("Org.Anth.Hello").is_err());
        assert!(validate_alias("hello-2").is_ok());
        assert!(validate_alias("Hello").is_err());

        let text = manifest().replace("manifest_version = 2", "manifest_version = 3");
        assert!(ExtensionManifest::parse(&text).is_err());
    }

    #[test]
    fn a_capability_cannot_use_a_program_outside_the_allowlist() {
        let text = manifest().replace("program = \"hostname\"", "program = \"sh\"");
        let error = ExtensionManifest::parse(&text).expect_err("program");
        assert!(error.message.contains("allowlist"), "{error}");
    }

    #[test]
    fn the_argv_template_must_match_the_accepted_arguments() {
        let text = manifest().replace(
            "program = \"hostname\"",
            "program = \"hostname\"\nargs = [\"--name\", \"{target}\"]\n\n[[capability.accept]]\nname = \"target\"\ntype = \"string\"",
        );
        let parsed = ExtensionManifest::parse(&text).expect("manifest");
        assert_eq!(parsed.capability[0].args, ["--name", "{target}"]);

        let missing = manifest().replace(
            "program = \"hostname\"",
            "program = \"hostname\"\nargs = [\"{target}\"]",
        );
        assert!(ExtensionManifest::parse(&missing).is_err());

        let unused = manifest().replace(
            "program = \"hostname\"",
            "program = \"hostname\"\n\n[[capability.accept]]\nname = \"target\"",
        );
        assert!(ExtensionManifest::parse(&unused).is_err());
    }

    #[test]
    fn anchors_and_ui_modules_are_confined() {
        assert!(validate_anchor_path("/status").is_ok());
        assert!(validate_anchor_path("status").is_err());
        assert!(validate_anchor_path("/../etc").is_err());
        assert!(validate_web_module("web/hello.js").is_ok());
        assert!(validate_web_module("web/../extension.wasm").is_err());
        assert!(validate_web_module("extension.wasm").is_err());
        assert!(validate_web_module("web/hello.exe").is_err());
    }

    #[test]
    fn duplicate_entries_are_refused() {
        let text = manifest().replace(
            "[[api]]\nmethod = \"GET\"\npath = \"/status\"",
            "[[api]]\nmethod = \"GET\"\npath = \"/status\"\n\n[[api]]\nmethod = \"GET\"\npath = \"/status\"",
        );
        assert!(ExtensionManifest::parse(&text).is_err());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn text() -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<char>(), 0..512)
            .prop_map(|chars| chars.into_iter().collect())
    }

    proptest! {
        #[test]
        fn arbitrary_manifest_text_never_panics_and_keeps_invariants(text in text()) {
            if let Ok(manifest) = ExtensionManifest::parse(&text) {
                match manifest.runtime.kind {
                    RuntimeKind::Wasm => {
                        let wasm = manifest.wasm.as_ref().expect("wasm");
                        prop_assert_eq!(wasm.path.as_str(), WASM_FILE);
                        prop_assert_eq!(wasm.abi, ABI_VERSION);
                        prop_assert_eq!(wasm.sha256.len(), 64);
                    }
                    RuntimeKind::Service => {
                        prop_assert!(manifest.wasm.is_none());
                        prop_assert!(manifest.service.as_ref().is_some_and(|service| service.socket.starts_with('/')));
                    }
                }
                prop_assert!(validate_id(&manifest.extension.id).is_ok());
                prop_assert!(manifest
                    .ui
                    .iter()
                    .all(|slot| slot.module.starts_with("web/") && !slot.module.contains("..")));
            }
        }
    }
}

/// `wasm/manifest.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WasmLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wasm: Option<WasmSection>,
    #[serde(default)]
    capability: Vec<Capability>,
}

/// `web/manifest.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebLayer {
    #[serde(default)]
    api: Vec<Anchor>,
    #[serde(default)]
    ui: Vec<UiSlot>,
}

/// `service/manifest.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceLayer {
    service: ServiceSection,
}

fn parse_layer<T: serde::de::DeserializeOwned>(bytes: &[u8], name: &str) -> Result<T, WireError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| WireError::invalid(format!("`{name}` is not UTF-8")))?;
    toml::from_str(text).map_err(|error| WireError::invalid(format!("`{name}`: {error}")))
}

/// Stable bytes signed by `manifest.sig`. File order and lengths are explicit,
/// so a name or a boundary cannot be confused with content.
pub fn manifest_message(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut out = b"anth-extension-manifest-v2\0".to_vec();
    for name in MANIFEST_FILES {
        if let Some(bytes) = files.get(name) {
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(bytes);
        }
    }
    out
}

#[cfg(test)]
mod split_tests {
    use super::*;

    fn core(runtime: &str) -> String {
        format!(
            r#"
manifest_version = 2
[extension]
id = "org.anth.split"
version = "0.4.0"
name = "Split"
{runtime}
"#
        )
    }

    #[test]
    fn layers_can_own_their_manifest_and_paths_are_relative() {
        let mut files = BTreeMap::new();
        files.insert(
            MANIFEST_FILE.to_string(),
            core("[runtime]\nkind = \"wasm\"").into_bytes(),
        );
        files.insert(
            WASM_MANIFEST_FILE.to_string(),
            b"[wasm]\npath = \"extension.wasm\"\nabi = 3\nsha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n[[capability]]\nid = \"host.hostname\"\nprogram = \"hostname\"\n".to_vec(),
        );
        files.insert(
            WEB_MANIFEST_FILE.to_string(),
            format!(
                "[[api]]\nmethod = \"GET\"\npath = \"/status\"\n\n[[ui]]\nslot = \"settings.extension\"\ntag = \"anth-split\"\nmodule = \"split.js\"\nsha256 = \"{}\"\n",
                "b".repeat(64)
            )
            .into_bytes(),
        );
        let manifest = ExtensionManifest::parse_files(&files).expect("manifest");
        assert_eq!(
            manifest.wasm.as_ref().expect("wasm").path,
            "wasm/extension.wasm"
        );
        assert_eq!(manifest.ui[0].module, "web/split.js");
        assert_eq!(manifest.capability.len(), 1);
        assert_eq!(manifest.api.len(), 1);
    }

    #[test]
    fn inline_and_layer_sections_are_mixed() {
        let mut files = BTreeMap::new();
        files.insert(
            MANIFEST_FILE.to_string(),
            format!(
                "{}[[capability]]\nid = \"host.uname\"\nprogram = \"uname\"\n",
                core("[runtime]\nkind = \"wasm\"\n[wasm]\npath = \"wasm/extension.wasm\"\nabi = 3\nsha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"")
            )
            .into_bytes(),
        );
        files.insert(
            WASM_MANIFEST_FILE.to_string(),
            b"[[capability]]\nid = \"host.hostname\"\nprogram = \"hostname\"\n".to_vec(),
        );
        let manifest = ExtensionManifest::parse_files(&files).expect("manifest");
        assert_eq!(manifest.capability.len(), 2);
    }

    #[test]
    fn a_service_manifest_has_no_wasm_and_freezes_the_socket() {
        let text =
            core("[runtime]\nkind = \"service\"\n[service]\nsocket = \"/run/anth/podman.sock\"");
        let manifest = ExtensionManifest::parse(&text).expect("manifest");
        assert_eq!(manifest.runtime.kind, RuntimeKind::Service);
        assert!(manifest.wasm.is_none());
        let grant = crate::ExtensionGrant::from_manifest(&manifest, "c".repeat(64), None);
        assert_eq!(grant.runtime, RuntimeKind::Service);
        assert_eq!(
            grant.service_socket.as_deref(),
            Some("/run/anth/podman.sock")
        );
        assert!(grant.wasm_sha256.is_none());
    }
}
