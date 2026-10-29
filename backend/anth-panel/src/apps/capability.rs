//! Manifest v3: capabilities, imports, CLI, API and UI.
//!
//! This module is the contract for the capability registry. It does not run an
//! app yet; it parses and validates the declarations the registry will consume.
//! The current runtime still uses the v2 manifest until the migration phase.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::manifest::{AppSection, RuntimeKind, RuntimeSection, UiSlot, WASM_FILE, validate_id};
use super::schema::{Signature, Type};

/// The only manifest version this module understands.
pub const MANIFEST_VERSION: u32 = 3;

/// The app ABI a v3 manifest targets.
pub const APP_ABI_VERSION: u32 = 4;

/// Reserved namespace for capabilities provided by the panel itself.
pub const HOST_NAMESPACE: &str = "org.anth.host";

/// Most exports, imports, CLI commands, API routes or UI mounts one app may declare.
pub const MAX_ENTRIES: usize = 64;

/// Longest `reason` a manifest may carry.
pub const MAX_REASON: usize = 256;

/// Longest capability id.
pub const MAX_ID: usize = 160;

/// One complete v3 manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestV3 {
    pub manifest_version: u32,
    pub app: AppSection,
    pub runtime: RuntimeSection,
    pub wasm: WasmSectionV3,
    #[serde(default)]
    pub export: Vec<Export>,
    #[serde(default)]
    pub import: Vec<Import>,
    #[serde(default)]
    pub cli: Vec<CliCommand>,
    #[serde(default)]
    pub api: Vec<ApiRoute>,
    #[serde(default)]
    pub ui: Vec<UiSlot>,
}

/// The module every v3 app must ship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmSectionV3 {
    pub path: String,
    pub sha256: String,
    pub abi: u32,
}

/// One capability the app provides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub id: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub input: Type,
    pub output: Type,
    pub implementation: Implementation,
}

impl Export {
    pub fn signature(&self) -> Signature {
        Signature {
            id: self.id.clone(),
            version: self.version,
            input: self.input.clone(),
            output: self.output.clone(),
        }
    }
}

/// One capability the app consumes from the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    pub id: String,
    pub version: u32,
    pub provider: String,
    pub input: Type,
    pub output: Type,
}

impl Import {
    pub fn signature(&self) -> Signature {
        Signature {
            id: self.id.clone(),
            version: self.version,
            input: self.input.clone(),
            output: self.output.clone(),
        }
    }
}

/// How an export is implemented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Implementation {
    /// The panel implements the operation in Rust.
    Host { operation: String },
    /// An entry point in the app's own wasm module.
    Wasm { entry: String },
    /// A one-shot native program.
    Binary(BinaryImplementation),
    /// A long-lived native helper.
    Service(ServiceImplementation),
}

impl Implementation {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Host { .. } => "host",
            Self::Wasm { .. } => "wasm",
            Self::Binary(_) => "binary",
            Self::Service(_) => "service",
        }
    }
}

/// Where a native program comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinarySource {
    /// `helpers/` inside the signed bundle.
    Bundle,
    /// An operator-allowlisted path on the host.
    Host,
    /// A package installed by the panel after explicit approval.
    Package,
}

/// A one-shot native program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryImplementation {
    pub source: BinarySource,
    pub path: String,
    pub sha256: String,
    pub argv: ArgvPolicy,
    pub sandbox: SandboxProfile,
    #[serde(default)]
    pub resource: Vec<Resource>,
    pub limits: Limits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_version: Option<String>,
}

/// A long-lived native helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceImplementation {
    pub source: BinarySource,
    pub path: String,
    pub sha256: String,
    pub protocol: ServiceProtocol,
    pub idle_seconds: u32,
    pub sandbox: SandboxProfile,
    #[serde(default)]
    pub resource: Vec<Resource>,
    pub limits: Limits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_version: Option<String>,
}

/// Framing a helper uses on its inherited fd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceProtocol {
    #[serde(rename = "http-fd")]
    HttpFd,
}

/// Who chooses the argv of a native program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ArgvPolicy {
    /// Literal argv from the manifest.
    Fixed { args: Vec<String> },
    /// Typed placeholders resolved from the capability input.
    Schema { args: Vec<String> },
    /// The host validates subcommands and flags against a list.
    Allowlist { commands: Vec<String> },
    /// The wasm chooses argv. Host-level authority; off by default.
    Any,
}

/// The isolation applied to a native implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxProfile {
    ReadOnly,
    Resource,
    Network,
    Host,
}

/// One resource the panel opens for a native implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resource {
    Socket {
        path: String,
        #[serde(rename = "as")]
        alias: String,
    },
    Device {
        path: String,
        #[serde(rename = "as")]
        alias: String,
    },
    Dir {
        path: String,
        #[serde(default)]
        write: bool,
    },
}

/// Bounds a native implementation must respect.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pids: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<u64>,
}

/// One CLI command mapped to an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliCommand {
    pub name: String,
    pub export: String,
}

/// One HTTP route mapped to an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiRoute {
    pub method: String,
    pub path: String,
    pub export: String,
}

impl ManifestV3 {
    /// Parses and validates one complete manifest.
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(text)
            .map_err(|error| format!("the manifest is not valid TOML: {error}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(format!(
                "manifest version {} is not {MANIFEST_VERSION}",
                self.manifest_version
            ));
        }
        validate_id(&self.app.id).map_err(|error| error.to_string())?;
        if self.app.id == HOST_NAMESPACE || self.app.id.starts_with(&format!("{HOST_NAMESPACE}.")) {
            return Err(format!(
                "the `{HOST_NAMESPACE}.*` namespace is reserved for the host"
            ));
        }
        if self.runtime.kind != RuntimeKind::Wasm {
            return Err("v3 apps run wasm only".to_string());
        }
        if self.wasm.path != WASM_FILE {
            return Err(format!("the module path is pinned to `{WASM_FILE}`"));
        }
        if self.wasm.abi != APP_ABI_VERSION {
            return Err(format!(
                "the app ABI is {APP_ABI_VERSION}, the manifest says {}",
                self.wasm.abi
            ));
        }
        validate_sha256(&self.wasm.sha256, "the module")?;
        check_count(self.export.len(), "exports")?;
        check_count(self.import.len(), "imports")?;
        check_count(self.cli.len(), "CLI commands")?;
        check_count(self.api.len(), "API routes")?;
        check_count(self.ui.len(), "UI mounts")?;

        let mut export_ids = BTreeSet::new();
        for export in &self.export {
            export.validate(&self.app.id)?;
            let key = format!("{}@{}", export.id, export.version);
            if !export_ids.insert(key.clone()) {
                return Err(format!("the export `{key}` is declared twice"));
            }
        }

        let mut import_ids = BTreeSet::new();
        for import in &self.import {
            import.validate(&self.app.id)?;
            let key = format!("{}@{}", import.id, import.version);
            if !import_ids.insert(key.clone()) {
                return Err(format!("the import `{key}` is declared twice"));
            }
        }

        let mut cli_names = BTreeSet::new();
        for command in &self.cli {
            validate_slug(&command.name, "a CLI command")?;
            if !cli_names.insert(command.name.clone()) {
                return Err(format!(
                    "the CLI command `{}` is declared twice",
                    command.name
                ));
            }
            require_own_export(&self.export, &command.export, "the CLI command")?;
        }

        let mut api_routes = BTreeSet::new();
        for route in &self.api {
            route.validate(&self.export)?;
            let key = format!("{} {}", route.method.to_uppercase(), route.path);
            if !api_routes.insert(key.clone()) {
                return Err(format!("the API route `{key}` is declared twice"));
            }
        }

        let mut ui_tags = BTreeSet::new();
        for slot in &self.ui {
            validate_ui(slot)?;
            if !ui_tags.insert(slot.tag.clone()) {
                return Err(format!("the UI tag `{}` is declared twice", slot.tag));
            }
        }
        Ok(())
    }
}

impl Export {
    fn validate(&self, app_id: &str) -> Result<(), String> {
        validate_capability_id(&self.id)?;
        if self.id != app_id && !self.id.starts_with(&format!("{app_id}.")) {
            return Err(format!(
                "the export `{}` is outside the app namespace `{app_id}.*`",
                self.id
            ));
        }
        if self.version == 0 {
            return Err("an export version starts at 1".to_string());
        }
        if let Some(reason) = &self.reason
            && (reason.len() > MAX_REASON || reason.chars().any(char::is_control))
        {
            return Err(format!(
                "an export reason is at most {MAX_REASON} bytes and has no control characters"
            ));
        }
        self.input.validate()?;
        self.output.validate()?;
        self.signature().validate()?;
        self.implementation.validate()
    }
}

impl Import {
    fn validate(&self, _app_id: &str) -> Result<(), String> {
        validate_capability_id(&self.id)?;
        if self.provider != "host" {
            return Err(
                "v1 imports must use provider = \"host\"; plugin-to-plugin is not enabled"
                    .to_string(),
            );
        }
        if !self.id.starts_with(&format!("{HOST_NAMESPACE}.")) {
            return Err(format!(
                "a v1 import targets the `{HOST_NAMESPACE}.*` namespace"
            ));
        }
        if self.version == 0 {
            return Err("an import version starts at 1".to_string());
        }
        self.input.validate()?;
        self.output.validate()?;
        self.signature().validate()?;
        Ok(())
    }
}

impl Implementation {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Host { operation } => {
                validate_operation(operation)?;
                Ok(())
            }
            Self::Wasm { entry } => {
                validate_slug(entry, "a wasm entry")?;
                Ok(())
            }
            Self::Binary(binary) => binary.validate(),
            Self::Service(service) => service.validate(),
        }
    }
}

impl BinaryImplementation {
    fn validate(&self) -> Result<(), String> {
        validate_binary_common(
            self.source,
            &self.path,
            &self.sha256,
            self.sandbox,
            &self.resource,
            &self.limits,
            self.package.as_deref(),
            self.package_version.as_deref(),
        )?;
        self.argv.validate()
    }
}

impl ServiceImplementation {
    fn validate(&self) -> Result<(), String> {
        validate_binary_common(
            self.source,
            &self.path,
            &self.sha256,
            self.sandbox,
            &self.resource,
            &self.limits,
            self.package.as_deref(),
            self.package_version.as_deref(),
        )?;
        if self.idle_seconds == 0 || self.idle_seconds > 86_400 {
            return Err("a helper idle timeout is 1 to 86400 seconds".to_string());
        }
        Ok(())
    }
}

impl ArgvPolicy {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Fixed { args } | Self::Schema { args } => validate_args(args),
            Self::Allowlist { commands } => {
                if commands.is_empty() || commands.len() > MAX_ENTRIES {
                    return Err("an argv allowlist declares 1 to 64 commands".to_string());
                }
                for command in commands {
                    validate_slug(command, "an argv allowlist entry")?;
                }
                Ok(())
            }
            Self::Any => Ok(()),
        }
    }
}

impl ApiRoute {
    fn validate(&self, exports: &[Export]) -> Result<(), String> {
        let method = self.method.to_uppercase();
        if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
            return Err("an API route uses GET, POST, PUT, PATCH or DELETE".to_string());
        }
        if !self.path.starts_with('/')
            || self.path.contains("..")
            || self.path.contains('?')
            || self.path.len() > 256
        {
            return Err("an API path is rooted, has no query and no `..`".to_string());
        }
        require_own_export(exports, &self.export, "the API route")
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_binary_common(
    source: BinarySource,
    path: &str,
    sha256: &str,
    sandbox: SandboxProfile,
    resources: &[Resource],
    limits: &Limits,
    package: Option<&str>,
    package_version: Option<&str>,
) -> Result<(), String> {
    validate_sha256(sha256, "a helper")?;
    if path.is_empty() || path.len() > 512 || path.chars().any(char::is_control) {
        return Err("a helper path is 1 to 512 bytes".to_string());
    }
    match source {
        BinarySource::Bundle => {
            if !path.starts_with("helpers/") || path.contains("..") {
                return Err("a bundled helper lives under `helpers/`".to_string());
            }
        }
        BinarySource::Host => {
            if !path.starts_with('/') || path.contains("..") {
                return Err("a host helper path is absolute and has no `..`".to_string());
            }
        }
        BinarySource::Package => {
            let package = package.ok_or_else(|| "a package names its package".to_string())?;
            validate_slug(package, "a package name")?;
            let version =
                package_version.ok_or_else(|| "a package pins its version".to_string())?;
            if version.is_empty() || version.len() > 64 {
                return Err("a package version is 1 to 64 bytes".to_string());
            }
            if !path.starts_with('/') {
                return Err("a package binary path is absolute".to_string());
            }
        }
    }
    if sandbox == SandboxProfile::ReadOnly && !resources.is_empty() {
        return Err("a read-only sandbox cannot open resources".to_string());
    }
    for resource in resources {
        resource.validate()?;
    }
    limits.validate()
}

impl Resource {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Socket { path, alias } | Self::Device { path, alias } => {
                validate_resource_path(path)?;
                validate_slug(alias, "a resource alias")
            }
            Self::Dir { path, .. } => validate_resource_path(path),
        }
    }
}

impl Limits {
    fn validate(&self) -> Result<(), String> {
        if self.memory_bytes == Some(0) || self.memory_bytes.is_some_and(|value| value > 1 << 30) {
            return Err("a memory limit is 1 byte to 1 GiB".to_string());
        }
        if self.pids == Some(0) || self.pids.is_some_and(|value| value > 1024) {
            return Err("a pids limit is 1 to 1024".to_string());
        }
        if self.wall_seconds == Some(0) || self.wall_seconds.is_some_and(|value| value > 3600) {
            return Err("a wall limit is 1 to 3600 seconds".to_string());
        }
        if self.output_bytes == Some(0)
            || self
                .output_bytes
                .is_some_and(|value| value > 64 * 1024 * 1024)
        {
            return Err("an output limit is 1 byte to 64 MiB".to_string());
        }
        Ok(())
    }
}

fn validate_capability_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_ID {
        return Err(format!("a capability id is 1 to {MAX_ID} bytes"));
    }
    let well_formed = id.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    });
    if !well_formed || !id.contains('.') {
        return Err(format!(
            "`{id}` is not a capability id: use lowercase dot-separated parts"
        ));
    }
    Ok(())
}

fn validate_slug(value: &str, what: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(format!(
            "{what} is 1 to 64 lowercase letters, digits, `-` or `_`"
        ));
    }
    Ok(())
}

fn validate_operation(operation: &str) -> Result<(), String> {
    if operation.is_empty()
        || operation.len() > 128
        || !operation.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
    {
        return Err("a host operation is a lowercase dotted name".to_string());
    }
    Ok(())
}

fn validate_args(args: &[String]) -> Result<(), String> {
    if args.len() > 64 {
        return Err("argv holds at most 64 elements".to_string());
    }
    for arg in args {
        if arg.len() > 512 || arg.chars().any(char::is_control) {
            return Err("an argv element is at most 512 bytes".to_string());
        }
    }
    Ok(())
}

fn validate_resource_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || path.contains("..") || path.len() > 512 {
        return Err(
            "a resource path is absolute, has no `..` and is at most 512 bytes".to_string(),
        );
    }
    Ok(())
}

fn validate_sha256(hash: &str, what: &str) -> Result<(), String> {
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{what} needs a 64-character hex sha256"));
    }
    Ok(())
}

fn check_count(count: usize, what: &str) -> Result<(), String> {
    if count > MAX_ENTRIES {
        return Err(format!("an app declares at most {MAX_ENTRIES} {what}"));
    }
    Ok(())
}

fn require_own_export(exports: &[Export], id: &str, what: &str) -> Result<(), String> {
    if exports.iter().any(|export| export.id == id) {
        Ok(())
    } else {
        Err(format!(
            "{what} references `{id}`, which this app does not export"
        ))
    }
}

fn validate_ui(slot: &UiSlot) -> Result<(), String> {
    if slot.slot.is_empty() || slot.slot.len() > 64 || slot.slot.contains('/') {
        return Err("a UI slot is 1 to 64 bytes and has no `/`".to_string());
    }
    validate_slug(&slot.tag, "a UI tag")?;
    if !slot.tag.starts_with("anth-") {
        return Err("a UI tag starts with `anth-`".to_string());
    }
    let module = &slot.module;
    if module.is_empty()
        || module.contains("..")
        || !(module.ends_with(".js") || module.ends_with(".mjs") || module.ends_with(".css"))
    {
        return Err("a UI module is a .js, .mjs or .css path without `..`".to_string());
    }
    if let Some(label) = &slot.label
        && (label.len() > 64 || label.chars().any(char::is_control))
    {
        return Err("a UI label is at most 64 bytes".to_string());
    }
    validate_sha256(&slot.sha256, "a UI module")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::schema::{Field, PathScope};

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn manifest(exports: &str, imports: &str, cli: &str, api: &str) -> String {
        format!(
            r#"
manifest_version = 3
[app]
id = "org.example.app"
version = "0.1.0"
name = "Example"
[runtime]
kind = "wasm"
[wasm]
path = "wasm/plugin.wasm"
sha256 = "{SHA}"
abi = 4
{exports}
{imports}
{cli}
{api}
"#
        )
    }

    fn export(id: &str, implementation: &str) -> String {
        format!(
            r#"
[[export]]
id = "{id}"
version = 1
input = {{ type = "record", fields = {{ name = {{ type = "string" }} }} }}
output = {{ type = "record", fields = {{ result = {{ type = "int", min = 0 }} }} }}
[export.implementation]
{implementation}
"#
        )
    }

    #[test]
    fn a_valid_manifest_parses_and_hashes_its_signature() {
        let text = manifest(
            &export(
                "org.example.app.program.action",
                "kind = \"host\"\noperation = \"program.action\"",
            ),
            "",
            r#"
[[cli]]
name = "action"
export = "org.example.app.program.action"
"#,
            r#"
[[api]]
method = "POST"
path = "/action"
export = "org.example.app.program.action"
"#,
        );
        let parsed = ManifestV3::parse(&text).expect("manifest");
        let signature = parsed.export[0].signature();
        assert_eq!(signature.id, "org.example.app.program.action");
        assert!(signature.validate().is_ok());
        assert_eq!(parsed.cli[0].export, signature.id);
        assert_eq!(parsed.api[0].path, "/action");
    }

    #[test]
    fn an_export_outside_the_app_namespace_is_refused() {
        let text = manifest(
            &export(
                "org.other.app.action",
                "kind = \"host\"\noperation = \"program.action\"",
            ),
            "",
            "",
            "",
        );
        let error = ManifestV3::parse(&text).expect_err("ownership");
        assert!(error.contains("outside the app namespace"), "{error}");
    }

    #[test]
    fn the_host_namespace_is_reserved() {
        let text = r#"
manifest_version = 3
[app]
id = "org.anth.host.evil"
version = "0.1.0"
name = "Evil"
[runtime]
kind = "wasm"
[wasm]
path = "wasm/plugin.wasm"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
abi = 4
"#;
        let error = ManifestV3::parse(text).expect_err("reserved");
        assert!(error.contains("reserved"), "{error}");
    }

    #[test]
    fn duplicate_exact_exports_are_refused() {
        let one = export(
            "org.example.app.action",
            "kind = \"host\"\noperation = \"program.action\"",
        );
        let text = manifest(&format!("{one}{one}"), "", "", "");
        let error = ManifestV3::parse(&text).expect_err("duplicate");
        assert!(error.contains("declared twice"), "{error}");
    }

    #[test]
    fn v1_imports_must_target_the_host() {
        let text = manifest(
            "",
            r#"
[[import]]
id = "org.example.app.other"
version = 1
provider = "host"
input = { type = "record", fields = {} }
output = { type = "record", fields = {} }
"#,
            "",
            "",
        );
        let error = ManifestV3::parse(&text).expect_err("namespace");
        assert!(error.contains("org.anth.host"), "{error}");
    }

    #[test]
    fn a_cli_or_api_entry_must_map_to_an_own_export() {
        let text = manifest(
            "",
            "",
            r#"
[[cli]]
name = "other"
export = "org.example.app.other"
"#,
            "",
        );
        let error = ManifestV3::parse(&text).expect_err("mapping");
        assert!(error.contains("does not export"), "{error}");
    }

    #[test]
    fn a_bundled_helper_is_validated_and_a_read_only_sandbox_cannot_open_resources() {
        let implementation = r#"
kind = "binary"
source = "bundle"
path = "helpers/action"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
sandbox = "read-only"
argv = { mode = "fixed", args = ["--json"] }
limits = { wall_seconds = 5, output_bytes = 1024 }

[[export.implementation.resource]]
kind = "socket"
path = "/run/example.sock"
as = "example"
"#;
        let text = manifest(
            &export("org.example.app.action", implementation),
            "",
            "",
            "",
        );
        let error = ManifestV3::parse(&text).expect_err("resource");
        assert!(error.contains("read-only"), "{error}");
    }

    #[test]
    fn a_service_declares_a_protocol_and_an_idle_timeout() {
        let implementation = r#"
kind = "service"
source = "host"
path = "/usr/bin/example"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
protocol = "http-fd"
idle_seconds = 300
sandbox = "resource"
argv = { mode = "fixed", args = [] }
"#;
        // Services do not take argv; the parser refuses the extra field.
        let text = manifest(
            &export("org.example.app.action", implementation),
            "",
            "",
            "",
        );
        assert!(ManifestV3::parse(&text).is_err());

        let implementation = r#"
kind = "service"
source = "host"
path = "/usr/bin/example"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
protocol = "http-fd"
idle_seconds = 300
sandbox = "resource"
limits = { wall_seconds = 30 }
"#;
        let text = manifest(
            &export("org.example.app.action", implementation),
            "",
            "",
            "",
        );
        let parsed = ManifestV3::parse(&text).expect("service");
        assert_eq!(parsed.export[0].implementation.kind(), "service");
    }

    #[test]
    fn modules_and_ui_tags_are_pinned() {
        let text = manifest("", "", "", "");
        let mut parsed = ManifestV3::parse(&text).expect("base");
        parsed.ui.push(UiSlot {
            slot: "settings.app".to_string(),
            tag: "podman".to_string(),
            module: "web/podman.js".to_string(),
            label: None,
            sha256: SHA.to_string(),
        });
        assert!(parsed.validate().is_err());

        parsed.ui[0].tag = "anth-podman".to_string();
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn a_package_source_pins_the_package_and_its_binary() {
        let implementation = r#"
kind = "binary"
source = "package"
path = "/usr/bin/podman"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
package = "podman"
package_version = "5.4.2"
argv = { mode = "fixed", args = ["--version"] }
sandbox = "resource"
limits = { wall_seconds = 10, output_bytes = 4096 }
"#;
        let text = manifest(
            &export("org.example.app.action", implementation),
            "",
            "",
            "",
        );
        let parsed = ManifestV3::parse(&text).expect("package");
        assert_eq!(parsed.export[0].implementation.kind(), "binary");

        let missing = implementation.replace("package_version = \"5.4.2\"", "");
        let text = manifest(&export("org.example.app.action", &missing), "", "", "");
        assert!(ManifestV3::parse(&text).is_err());
    }

    #[test]
    fn a_path_type_is_not_a_plain_string() {
        let path = Type::path(PathScope::App);
        assert!(
            path.validate_value(&serde_json::json!("/etc/hosts"))
                .is_ok()
        );
        let record = Type::Record {
            fields: std::collections::BTreeMap::from([("path".to_string(), Field::required(path))]),
        };
        assert!(
            record
                .validate_value(&serde_json::json!({ "path": "notes.txt" }))
                .is_ok()
        );
    }
}
