//! Plugin manifests, capability schemas and host-side argument validators.
//!
//! This module is the authority boundary. A plugin declares a *recipe*: which
//! program it needs and the shape of its arguments. The host decides whether
//! that recipe is acceptable, freezes it at install time, and fills it later
//! from values it validated itself.
//!
//! The split matters because a plugin that could send a finished argv would not
//! need a schema at all: one granted `wg` could ask for `show wg0
//! private-key`. So the argv in a manifest is a template whose only
//! substitutable parts are `{name}` placeholders, each bound to a validator
//! from [`ArgType`]. A plugin cannot add a validator, and a value that fails
//! its validator never reaches a program.
//!
//! Everything here is pure: parsing, validation and rendering take text or
//! plain data and return owned values, so the tests assert the exact argv a
//! capability would produce without running anything.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Largest manifest accepted, in bytes. A recipe is small; a megabyte is
/// already absurd and bounds the work the parser can be asked to do.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Largest number of capabilities, API anchors or UI anchors one plugin may
/// declare. The bound keeps a snapshot of the installed set readable and stops
/// an installer from being made to do unbounded work.
pub const MAX_ENTRIES: usize = 256;

/// The ABI this host implements. A manifest that asks for anything else is
/// refused at install time rather than failing at the first invocation.
pub const SUPPORTED_ABI: u32 = 1;

/// Longest a rendered argument may be. Programs take short values; a plugin
/// that needs a large payload belongs in a file, not in argv.
pub const MAX_ARG_LEN: usize = 512;

/// Plugin ids are reverse-DNS and become part of a URL path, so the shape is
/// restricted to what needs no escaping.
pub const MAX_ID_LEN: usize = 128;

/// Where a bundle keeps its module. Fixed rather than read from the manifest at
/// load time, so the file a grant was hashed against cannot be re-pointed by
/// editing `plugin.toml`.
pub const WASM_FILE: &str = "plugin.wasm";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginErrorKind {
    /// The bundle or the manifest is malformed.
    Invalid,
    /// The manifest reaches outside what the host permits: unknown program,
    /// unknown validator, path escaping its namespace.
    Denied,
    /// The plugin is not installed, or is installed but disabled.
    Missing,
    /// The plugin is already installed.
    Conflict,
    /// Reading or writing the bundle failed.
    Io,
    /// The hash of the wasm does not match the frozen grant.
    Tampered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginError {
    pub kind: PluginErrorKind,
    pub detail: String,
}

impl PluginError {
    pub fn new(kind: PluginErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn invalid(detail: impl Into<String>) -> Self {
        Self::new(PluginErrorKind::Invalid, detail)
    }

    pub fn denied(detail: impl Into<String>) -> Self {
        Self::new(PluginErrorKind::Denied, detail)
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for PluginError {}

/// A validator the host owns. The set is closed on purpose: a plugin cannot
/// define one, because a plugin-defined pattern would be `.*` in practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgType {
    /// A network interface name: what `ip link` and `wg` take.
    Ifname,
    /// A host name or label.
    Hostname,
    /// A TCP or UDP port.
    Port,
    /// A dotted-quad IPv4 address.
    Ipv4,
    /// A literal IPv6 address.
    Ipv6,
    /// An address with a prefix length.
    Cidr,
    /// Exactly 32 hex characters, the shape of a Curve25519 key.
    Hex32,
    /// Exactly 64 hex characters, the shape of a SHA-256 digest.
    Hex64,
    /// An absolute, normalized filesystem path.
    Path,
    /// A short printable string with no control characters.
    Text,
}

impl ArgType {
    /// Whether this type may carry free text. `Text` is the only one, and a
    /// capability that uses it has to say so explicitly.
    pub fn is_free_text(self) -> bool {
        matches!(self, ArgType::Text)
    }

    /// Checks one value. The rules are local and closed: no regular
    /// expressions, no locale, no filesystem access.
    pub fn validate(self, value: &str) -> Result<(), String> {
        if value.is_empty() {
            return Err("must not be empty".to_string());
        }
        if value.len() > MAX_ARG_LEN {
            return Err(format!("longer than {MAX_ARG_LEN} bytes"));
        }
        if value.chars().any(char::is_control) {
            return Err("contains a control character".to_string());
        }
        // A value that starts with `-` is read by the program as an option, not
        // as the operand it was validated to be. Checking it here rather than
        // per type matters: a new validator would otherwise inherit the hole,
        // and `wg show --force` is exactly the flag a plugin was not granted.
        if value.starts_with('-') {
            return Err("must not start with `-`".to_string());
        }

        match self {
            ArgType::Ifname => {
                if value.len() > 15 {
                    return Err("an interface name is at most 15 bytes".to_string());
                }
                if value.contains('/') || value.contains("..") || value.starts_with('.') {
                    return Err("not a plain interface name".to_string());
                }
                if value.chars().any(char::is_whitespace) {
                    return Err("not a plain interface name".to_string());
                }
                Ok(())
            }
            ArgType::Hostname => {
                if value.len() > 253 {
                    return Err("a host name is at most 253 bytes".to_string());
                }
                if value.ends_with('-') {
                    return Err("a host name cannot end with `-`".to_string());
                }
                if !value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
                {
                    return Err("a host name is letters, digits, `-` and `.`".to_string());
                }
                Ok(())
            }
            ArgType::Port => match value.parse::<u16>() {
                Ok(port) if port != 0 => Ok(()),
                _ => Err("not a port between 1 and 65535".to_string()),
            },
            ArgType::Ipv4 => match Ipv4Addr::from_str(value) {
                Ok(_) => Ok(()),
                Err(_) => Err("not an IPv4 address".to_string()),
            },
            ArgType::Ipv6 => match Ipv6Addr::from_str(value) {
                Ok(_) => Ok(()),
                Err(_) => Err("not an IPv6 address".to_string()),
            },
            ArgType::Cidr => {
                let (address, prefix) = value
                    .split_once('/')
                    .ok_or_else(|| "not a CIDR prefix".to_string())?;
                let bits = match Ipv4Addr::from_str(address) {
                    Ok(_) => 32,
                    Err(_) => {
                        Ipv6Addr::from_str(address).map_err(|_| "not an address".to_string())?;
                        128
                    }
                };
                let prefix: u8 = prefix
                    .parse()
                    .map_err(|_| "bad prefix length".to_string())?;
                if prefix > bits {
                    return Err(format!("prefix length is at most {bits}"));
                }
                Ok(())
            }
            ArgType::Hex32 => expect_hex(value, 32),
            ArgType::Hex64 => expect_hex(value, 64),
            ArgType::Path => {
                if !value.starts_with('/') {
                    return Err("a path must be absolute".to_string());
                }
                if std::path::Path::new(value)
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err("a path must not contain `..`".to_string());
                }
                Ok(())
            }
            ArgType::Text => {
                if value.len() > 128 {
                    return Err("free text is at most 128 bytes".to_string());
                }
                Ok(())
            }
        }
    }
}

fn expect_hex(value: &str, len: usize) -> Result<(), String> {
    if value.len() != len {
        return Err(format!("expected {len} hex characters"));
    }
    if !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("expected hex characters".to_string());
    }
    Ok(())
}

/// One declared argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgSpec {
    #[serde(rename = "type")]
    pub kind: ArgType,
    /// Whether the argument is optional. Absent means required.
    #[serde(default)]
    pub optional: bool,
}

/// One authorized command.
///
/// `job` names a well-known action the host already implements. That is the
/// strong form: the daemon owns the argv entirely and the plugin only supplies
/// argument values. `program` plus an `argv` template is the general form, and
/// it is accepted only when the program is on the host allowlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    #[serde(default)]
    pub job: Option<String>,
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub args: BTreeMap<String, ArgSpec>,
    /// Must be set for a capability whose template can receive `text`.
    #[serde(default)]
    pub accepts_text: bool,
}

impl Capability {
    /// Placeholder names this capability's template refers to.
    pub fn placeholders(&self) -> Result<BTreeSet<String>, PluginError> {
        let mut names = BTreeSet::new();
        for part in &self.argv {
            for name in placeholders_in(part)? {
                names.insert(name);
            }
        }
        Ok(names)
    }
}

/// Extracts `{name}` placeholders from one template part.
///
/// An unbalanced or nested brace is an error rather than literal text: silently
/// passing it through would make the rendered argv differ from what the admin
/// read in the manifest.
fn placeholders_in(part: &str) -> Result<Vec<String>, PluginError> {
    let mut names = Vec::new();
    let mut rest = part;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let end = after.find('}').ok_or_else(|| {
            PluginError::invalid(format!("`{part}` opens a placeholder it never closes"))
        })?;
        let name = &after[..end];
        if name.is_empty() {
            return Err(PluginError::invalid(format!(
                "`{part}` has an empty placeholder"
            )));
        }
        if name.contains('{') {
            return Err(PluginError::invalid(format!(
                "`{part}` nests a placeholder"
            )));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(PluginError::invalid(format!(
                "`{name}` is not a placeholder name"
            )));
        }
        names.push(name.to_string());
        rest = &after[end + 1..];
    }
    if rest.contains('}') {
        return Err(PluginError::invalid(format!(
            "`{part}` closes a placeholder it never opened"
        )));
    }
    Ok(names)
}

/// Renders one capability into the argv that would reach the program.
///
/// Every placeholder is replaced by the value its validator accepted, and every
/// supplied argument is checked against the capability's declarations. An
/// undeclared name is refused instead of being appended: appending is exactly
/// how a plugin would add a flag the admin never approved.
pub fn render_argv(
    capability: &Capability,
    values: &BTreeMap<String, String>,
) -> Result<Vec<String>, PluginError> {
    for (name, value) in values {
        let Some(spec) = capability.args.get(name) else {
            return Err(PluginError::denied(format!(
                "capability `{}` declares no argument `{name}`",
                capability.id
            )));
        };
        if spec.kind.is_free_text() && !capability.accepts_text {
            return Err(PluginError::denied(format!(
                "capability `{}` does not accept free text for `{name}`",
                capability.id
            )));
        }
        spec.kind.validate(value).map_err(|reason| {
            PluginError::denied(format!("argument `{name}` is invalid: {reason}"))
        })?;
    }

    for name in capability.placeholders()? {
        if !values.contains_key(&name) {
            return Err(PluginError::denied(format!(
                "capability `{}` requires `{name}`",
                capability.id
            )));
        }
    }

    let mut argv = Vec::with_capacity(capability.argv.len());
    for part in &capability.argv {
        let mut rendered = String::new();
        let mut rest = part.as_str();
        while let Some(start) = rest.find('{') {
            rendered.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let end = after.find('}').unwrap_or(after.len());
            let name = &after[..end];
            let value = values.get(name).ok_or_else(|| {
                PluginError::denied(format!(
                    "capability `{}` has no value for `{name}`",
                    capability.id
                ))
            })?;
            rendered.push_str(value);
            rest = &after[end + 1..];
        }
        rendered.push_str(rest);
        argv.push(rendered);
    }
    Ok(argv)
}

/// Where a plugin's interface attaches inside the shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiAnchor {
    pub slot: String,
    pub tag: String,
    pub module: String,
}

/// One HTTP anchor. `path` is always relative to the plugin's namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiAnchor {
    pub method: String,
    pub path: String,
}

/// The manifest as parsed from `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub plugin: PluginMeta,
    pub wasm: WasmRef,
    #[serde(default, rename = "capability")]
    pub capabilities: Vec<Capability>,
    #[serde(default, rename = "api")]
    pub api: Vec<ApiAnchor>,
    #[serde(default, rename = "ui")]
    pub ui: Vec<UiAnchor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub version: String,
    pub name: String,
    pub abi: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmRef {
    pub path: String,
    pub sha256: String,
}

/// Programs a plugin may name. Everything the panel already runs for its own
/// features, and nothing else.
///
/// This is the recipe shelf. A plugin cannot reach a program the admin never saw
/// the panel use, and it cannot pass a path: the match is on the bare program
/// name, resolved by the host.
pub const PROGRAM_ALLOWLIST: &[&str] = &[
    "ip",
    "nft",
    "wg",
    "wg-quick",
    "mount",
    "umount",
    "lsblk",
    "getent",
    "sfdisk",
    "mkfs",
    "systemctl",
    "journalctl",
    "ss",
];

/// Jobs the host implements itself, with an argv it owns. A plugin naming one of
/// these supplies no program and no template, only argument values.
pub const KNOWN_JOBS: &[&str] = &[
    "host.info",
    "network.list",
    "network.state",
    "process.list",
    "process.signal",
    "volume.list",
    "firewall.list",
];

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, PluginError> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err(PluginError::invalid(format!(
                "the manifest is larger than {MAX_MANIFEST_BYTES} bytes"
            )));
        }
        let manifest: Self = toml::from_str(text)
            .map_err(|error| PluginError::invalid(format!("invalid manifest: {error}")))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Checks the manifest against host policy, independently of whether it is
    /// installed. `install` calls this before writing any grant.
    pub fn validate(&self) -> Result<(), PluginError> {
        validate_id(&self.plugin.id)?;

        if self.plugin.version.trim().is_empty() {
            return Err(PluginError::invalid("the manifest has no version"));
        }
        if self.plugin.name.trim().is_empty() {
            return Err(PluginError::invalid("the manifest has no name"));
        }
        if self.plugin.abi != SUPPORTED_ABI {
            return Err(PluginError::denied(format!(
                "the manifest asks for ABI {}, this host implements {SUPPORTED_ABI}",
                self.plugin.abi
            )));
        }

        validate_wasm_ref(&self.wasm)?;

        if self.capabilities.len() > MAX_ENTRIES
            || self.api.len() > MAX_ENTRIES
            || self.ui.len() > MAX_ENTRIES
        {
            return Err(PluginError::invalid(format!(
                "a plugin may declare at most {MAX_ENTRIES} capabilities, API and UI anchors"
            )));
        }

        let mut seen = BTreeSet::new();
        for capability in &self.capabilities {
            validate_capability(capability)?;
            if !seen.insert(capability.id.clone()) {
                return Err(PluginError::invalid(format!(
                    "capability `{}` is declared twice",
                    capability.id
                )));
            }
        }

        let mut routes = BTreeSet::new();
        for anchor in &self.api {
            anchor_route(&self.plugin.id, anchor)?;
            if !routes.insert((anchor.method.to_ascii_uppercase(), anchor.path.clone())) {
                return Err(PluginError::invalid(format!(
                    "route `{} {}` is declared twice",
                    anchor.method, anchor.path
                )));
            }
        }

        for anchor in &self.ui {
            validate_ui_anchor(anchor)?;
        }

        Ok(())
    }
}

pub fn validate_id(id: &str) -> Result<(), PluginError> {
    if id.is_empty() || id.len() > MAX_ID_LEN {
        return Err(PluginError::invalid(format!(
            "a plugin id is 1 to {MAX_ID_LEN} bytes"
        )));
    }
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() < 2 {
        return Err(PluginError::invalid(
            "a plugin id is reverse-DNS, for example `org.anth.wireguard`",
        ));
    }
    for part in parts {
        if part.is_empty() {
            return Err(PluginError::invalid("a plugin id has an empty segment"));
        }
        if !part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(PluginError::invalid(format!(
                "`{part}` must be lowercase letters, digits or `-`"
            )));
        }
    }
    Ok(())
}

fn validate_wasm_ref(wasm: &WasmRef) -> Result<(), PluginError> {
    let path = wasm.path.trim();
    if path.is_empty() {
        return Err(PluginError::invalid("the manifest names no wasm module"));
    }
    if path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return Err(PluginError::denied(format!("`{path}` escapes the bundle")));
    }
    if wasm.sha256.len() != 64 || !wasm.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PluginError::invalid(
            "`wasm.sha256` must be 64 hex characters",
        ));
    }
    Ok(())
}

fn validate_capability(capability: &Capability) -> Result<(), PluginError> {
    if capability.id.is_empty() || capability.id.len() > 64 {
        return Err(PluginError::invalid("a capability id is 1 to 64 bytes"));
    }
    if !capability
        .id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
    {
        return Err(PluginError::invalid(format!(
            "capability id `{}` must be lowercase letters, digits, `.`, `_` or `-`",
            capability.id
        )));
    }

    match (&capability.job, &capability.program) {
        (Some(job), None) => {
            if !KNOWN_JOBS.contains(&job.as_str()) {
                return Err(PluginError::denied(format!(
                    "`{job}` is not a job this host implements"
                )));
            }
            if !capability.argv.is_empty() {
                // A job's argv belongs to the host. Accepting a template here
                // would let a plugin rewrite a command the admin believes is
                // fixed.
                return Err(PluginError::denied(format!(
                    "capability `{}` names a job and a template; a job owns its argv",
                    capability.id
                )));
            }
        }
        (None, Some(program)) => {
            if !PROGRAM_ALLOWLIST.contains(&program.as_str()) {
                return Err(PluginError::denied(format!(
                    "`{program}` is not a program plugins may run"
                )));
            }
            if capability.argv.is_empty() {
                return Err(PluginError::invalid(format!(
                    "capability `{}` names a program but no argv",
                    capability.id
                )));
            }
        }
        (Some(_), Some(_)) => {
            return Err(PluginError::invalid(format!(
                "capability `{}` sets both `job` and `program`",
                capability.id
            )));
        }
        (None, None) => {
            return Err(PluginError::invalid(format!(
                "capability `{}` sets neither `job` nor `program`",
                capability.id
            )));
        }
    }

    let placeholders = capability.placeholders()?;
    for name in capability.args.keys() {
        if !placeholders.contains(name) {
            return Err(PluginError::invalid(format!(
                "capability `{}` declares `{name}` but its argv never uses it",
                capability.id
            )));
        }
    }
    for name in &placeholders {
        if !capability.args.contains_key(name) {
            return Err(PluginError::invalid(format!(
                "capability `{}` uses `{{{name}}}` but declares no validator",
                capability.id
            )));
        }
    }

    // A text argument without the opt-in would be a capability that can never
    // take a value, so the manifest is refused rather than installed dead.
    if let Some(name) = capability
        .args
        .iter()
        .find(|(_, spec)| spec.kind.is_free_text())
        .map(|(name, _)| name)
        && !capability.accepts_text
    {
        return Err(PluginError::denied(format!(
            "capability `{}` declares free text for `{name}` without `accepts_text = true`",
            capability.id
        )));
    }

    if capability.accepts_text
        && !capability
            .args
            .values()
            .any(|spec| spec.kind.is_free_text())
    {
        return Err(PluginError::invalid(format!(
            "capability `{}` sets `accepts_text` but declares no text argument",
            capability.id
        )));
    }

    Ok(())
}

fn validate_api_anchor(anchor: &ApiAnchor) -> Result<(), PluginError> {
    let method = anchor.method.to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        return Err(PluginError::invalid(format!(
            "`{}` is not an HTTP method",
            anchor.method
        )));
    }
    if !anchor.path.starts_with('/') {
        return Err(PluginError::invalid(format!(
            "route `{}` must start with `/`",
            anchor.path
        )));
    }
    if anchor.path.contains("..") || anchor.path.contains('\\') {
        return Err(PluginError::denied(format!(
            "route `{}` escapes its namespace",
            anchor.path
        )));
    }
    if anchor.path.contains('?') || anchor.path.contains('#') {
        return Err(PluginError::invalid(format!(
            "route `{}` must not carry a query or fragment",
            anchor.path
        )));
    }
    Ok(())
}

/// Resolves a declared anchor against its plugin id, and refuses one that would
/// land outside the plugin's own prefix.
///
/// This is the namespacing rule, applied at install. It is *not* how the HTTP
/// handler authorizes a request — it never reads the manifest — but a manifest
/// that declares `/api/auth/login` is either a mistake or an attempt, and either
/// way the operator should hear about it at install rather than find a route
/// that silently does something else.
///
/// Two things it rejects that the schema-level check cannot see, because they
/// depend on the id:
///
/// - a path that, once joined, still contains `..` as a *segment after
///   normalization*, which a naive prefix check would wave through;
/// - a path starting with a second slash, which some clients normalize
///   differently from others.
pub fn anchor_route(plugin_id: &str, anchor: &ApiAnchor) -> Result<String, PluginError> {
    validate_api_anchor(anchor)?;
    validate_id(plugin_id)?;

    let path = anchor.path.trim();
    if path.starts_with("//") {
        return Err(PluginError::denied(format!(
            "route `{path}` starts with a second slash"
        )));
    }

    // Normalize segment by segment. An empty segment is a repeated slash and
    // collapses; `..` is refused outright rather than resolved, because a route
    // that needs traversal is not a route this host will construct.
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(PluginError::denied(format!(
                    "route `{path}` escapes its namespace"
                )));
            }
            other => parts.push(other),
        }
    }

    let mut route = format!("/api/plugins/{plugin_id}");
    for part in parts {
        route.push('/');
        route.push_str(part);
    }
    Ok(route)
}

/// Slots the shell knows how to fill. A plugin cannot invent one, because the
/// shell has nowhere to put it.
pub const UI_SLOTS: &[&str] = &[
    "dashboard.widget",
    "taskbar.item",
    "page",
    "panel.process.tab",
    "panel.network.tab",
    "panel.files.tab",
    "panel.volumes.tab",
    "panel.firewall.tab",
    "panel.accounts.tab",
];

fn validate_ui_anchor(anchor: &UiAnchor) -> Result<(), PluginError> {
    if !UI_SLOTS.contains(&anchor.slot.as_str()) {
        return Err(PluginError::denied(format!(
            "`{}` is not a slot the shell provides",
            anchor.slot
        )));
    }
    let tag = anchor.tag.trim();
    if !tag.starts_with("anth-")
        || !tag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(PluginError::invalid(format!(
            "`{tag}` must be a custom element name starting with `anth-`"
        )));
    }
    let module = anchor.module.trim();
    if !module.starts_with("web/")
        || module.starts_with('/')
        || module.split('/').any(|part| part == "..")
    {
        return Err(PluginError::denied(format!(
            "`{module}` must live under the bundle's `web/`"
        )));
    }
    Ok(())
}

/// One capability as frozen at install time.
///
/// The whole recipe is stored, not just the id. A grant holding ids alone would
/// leave the argv template to be read from the manifest on every invocation, and
/// the manifest is a file inside a bundle a local writer can rewrite. Freezing
/// the template, the program and the validators is what makes `render_argv`
/// authoritative rather than advisory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantedCapability {
    pub id: String,
    #[serde(default)]
    pub job: Option<String>,
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub args: BTreeMap<String, ArgSpec>,
    #[serde(default)]
    pub accepts_text: bool,
}

impl From<&Capability> for GrantedCapability {
    fn from(capability: &Capability) -> Self {
        Self {
            id: capability.id.clone(),
            job: capability.job.clone(),
            program: capability.program.clone(),
            argv: capability.argv.clone(),
            args: capability.args.clone(),
            accepts_text: capability.accepts_text,
        }
    }
}

impl GrantedCapability {
    /// The shape `render_argv` takes, so a frozen recipe is rendered by the same
    /// code that validated it at install.
    pub fn as_capability(&self) -> Capability {
        Capability {
            id: self.id.clone(),
            job: self.job.clone(),
            program: self.program.clone(),
            argv: self.argv.clone(),
            args: self.args.clone(),
            accepts_text: self.accepts_text,
        }
    }
}

/// The grant written to the config at install time.
///
/// This is the frozen recipe. The capabilities here are what the plugin may use,
/// whatever a later manifest claims, and `wasm_sha256` is what makes the freeze
/// real: a swapped module is caught before it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginGrant {
    pub id: String,
    pub version: String,
    pub wasm_sha256: String,
    /// Frozen recipes. This is the authority; `allowed_ids` is only a view.
    pub recipes: Vec<GrantedCapability>,
    /// Whether the plugin is loaded at all. `plugin approve` sets it after a
    /// version that declares new capabilities.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl PluginGrant {
    /// Builds the grant from a validated manifest.
    pub fn from_manifest(manifest: &Manifest) -> Self {
        Self {
            id: manifest.plugin.id.clone(),
            version: manifest.plugin.version.clone(),
            wasm_sha256: manifest.wasm.sha256.to_ascii_lowercase(),
            recipes: manifest
                .capabilities
                .iter()
                .map(GrantedCapability::from)
                .collect(),
            enabled: true,
        }
    }

    pub fn allows(&self, capability: &str) -> bool {
        self.enabled && self.recipes.iter().any(|recipe| recipe.id == capability)
    }

    /// The frozen recipe for `id`, if the grant allows it.
    ///
    /// Execution reads the recipe through this, never through the manifest, so
    /// the program and the template are always the approved ones.
    pub fn recipe(&self, id: &str) -> Option<&GrantedCapability> {
        if !self.enabled {
            return None;
        }
        self.recipes.iter().find(|recipe| recipe.id == id)
    }

    /// The capability ids this grant carries, in install order.
    pub fn allowed_ids(&self) -> Vec<String> {
        self.recipes
            .iter()
            .map(|recipe| recipe.id.clone())
            .collect()
    }
}

/// The public view of an installed plugin. Capabilities are listed, the wasm
/// bytes are not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub id: String,
    pub version: String,
    pub name: String,
    pub enabled: bool,
    pub capabilities: Vec<String>,
    pub api: Vec<ApiAnchor>,
    pub ui: Vec<UiAnchor>,
}

#[cfg(test)]
mod tests;
