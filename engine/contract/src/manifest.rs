//! The manifest: an index of interfaces, routes, slots and limits.
//!
//! It carries no types — those live in WIT — and nothing in it is trusted until
//! the composer resolves it and the operator approves the resulting grant. What
//! is checked here is everything that can be checked without a component: names,
//! paths, duplicates and the rules that keep a manifest internally consistent.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::id::{is_kebab, is_sha256, parse_version, AppId, InterfaceId, Namespace};
use crate::Risk;

/// The manifest schema this version speaks.
pub const MANIFEST_VERSION: u32 = 2;
/// The service/hybrid schema. v2 apps stay valid unchanged.
pub const SERVICE_MANIFEST_VERSION: u32 = 3;

/// The verbs a route may declare. `OPTIONS` belongs to the engine.
pub const ROUTE_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];

/// Subcommands of `anth app`. An alias may not shadow them, because
/// `anth app list` must mean one thing. A verb never collides: it is always the
/// second token, as in `anth app notes list`.
pub const RESERVED_ALIASES: &[&str] = &[
    "list", "show", "install", "upgrade", "approve", "revoke", "enable", "disable", "remove",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest_version: u32,
    pub app: AppSection,
    #[serde(default)]
    pub requires: Option<RequiresSection>,
    #[serde(default)]
    pub wasm: Option<WasmSection>,
    #[serde(default)]
    pub service: Option<ServiceSection>,
    #[serde(default)]
    pub export: Vec<ExportSection>,
    #[serde(default)]
    pub import: Vec<ImportSection>,
    #[serde(default)]
    pub route: Vec<RouteSection>,
    #[serde(default)]
    pub slot: Vec<SlotSection>,
    #[serde(default)]
    pub cli: Vec<CliSection>,
    #[serde(default)]
    pub limits: LimitsSection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSection {
    pub id: String,
    #[serde(default)]
    pub alias: Option<String>,
    pub namespace: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiresSection {
    #[serde(default)]
    pub engine: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmSection {
    pub path: String,
    pub sha256: String,
}

/// A native child process. v3 only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSection {
    pub path: String,
    pub sha256: String,
    pub class: ServiceClass,
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceClass {
    Exec,
    Socket,
    Device,
    Admin,
    Host,
}

impl ServiceClass {
    pub const ALL: &'static [ServiceClass] = &[
        ServiceClass::Exec,
        ServiceClass::Socket,
        ServiceClass::Device,
        ServiceClass::Admin,
        ServiceClass::Host,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Socket => "socket",
            Self::Device => "device",
            Self::Admin => "admin",
            Self::Host => "host",
        }
    }

    /// The risk a service adds to its app and to every consumer of its exports.
    pub const fn risk(self) -> Risk {
        match self {
            Self::Socket | Self::Device | Self::Admin => Risk::Admin,
            Self::Exec | Self::Host => Risk::Host,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportTarget {
    #[default]
    Wasm,
    Service,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSection {
    pub interface: String,
    /// Required when the app has both `[wasm]` and `[service]`; otherwise it is
    /// inferred from the half that exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ExportTarget>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportSection {
    pub interface: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteSection {
    pub method: String,
    pub path: String,
    pub call: String,
    /// The call returns a stream (§20). The response is chunked bytes instead of
    /// a JSON document; the function and its result shape are checked against
    /// `wit/` at install time.
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotSection {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub order: i64,
    /// Hidden contributions can still be opened by other views of the same app,
    /// but the panel does not list them in its navigation.
    #[serde(default)]
    pub hidden: bool,
    pub tag: String,
    pub module: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliSection {
    pub call: String,
    pub verb: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct LimitsSection {
    #[serde(default)]
    pub fuel: Option<u64>,
    #[serde(default)]
    pub memory_pages: Option<u32>,
    #[serde(default)]
    pub wall_ms: Option<u64>,
    #[serde(default)]
    pub bytes_total: Option<u64>,
    #[serde(default)]
    pub bytes_per_sec: Option<u64>,
}

impl Manifest {
    /// Parses a manifest from TOML. Unknown keys are errors, not warnings.
    #[cfg(feature = "parse")]
    pub fn parse(text: &str) -> Result<Self, Error> {
        toml::from_str(text)
            .map_err(|error| Error::invalid(format!("the manifest is not valid TOML: {error}")))
    }

    /// The short name the CLI uses: the alias, or the id's last label when that
    /// label is not a reserved command. `None` means the CLI must use the full
    /// id, which is better than a short name that means something else.
    pub fn cli_name(&self) -> Option<&str> {
        if let Some(alias) = &self.app.alias {
            return Some(alias.as_str());
        }
        let last = self.app.id.rsplit('.').next()?;
        if RESERVED_ALIASES.contains(&last) {
            None
        } else {
            Some(last)
        }
    }

    /// The half that implements an export. Validation rejects the ambiguous
    /// hybrid case where both halves exist and `target` is omitted.
    pub fn export_target(&self, export: &ExportSection) -> ExportTarget {
        export.target.unwrap_or_else(|| {
            if self.wasm.is_none() && self.service.is_some() {
                ExportTarget::Service
            } else {
                ExportTarget::Wasm
            }
        })
    }

    /// The half that implements an exported interface, if it is exported.
    pub fn interface_target(&self, interface: &str) -> Option<ExportTarget> {
        self.export
            .iter()
            .find(|export| export.interface == interface)
            .map(|export| self.export_target(export))
    }

    /// The risk the declared service class adds, independent of imports.
    pub fn service_risk(&self) -> Risk {
        self.service
            .as_ref()
            .map(|service| service.class.risk())
            .unwrap_or(Risk::Read)
    }

    /// Every rule that does not need a component or the catalog.
    pub fn validate(&self) -> Result<(), Error> {
        if !matches!(
            self.manifest_version,
            MANIFEST_VERSION | SERVICE_MANIFEST_VERSION
        ) {
            return Err(Error::invalid(format!(
                "manifest_version {} is not supported; this engine speaks \
                 {MANIFEST_VERSION} and {SERVICE_MANIFEST_VERSION}",
                self.manifest_version
            )));
        }
        if self.manifest_version == MANIFEST_VERSION && self.service.is_some() {
            return Err(Error::invalid("`[service]` needs `manifest_version = 3`"));
        }
        if self.manifest_version == SERVICE_MANIFEST_VERSION
            && self.wasm.is_none()
            && self.service.is_none()
        {
            return Err(Error::invalid(
                "a v3 app needs a `[wasm]` component, a `[service]`, or both",
            ));
        }
        if let Some(service) = &self.service {
            check_service(service)?;
        }

        AppId::parse(&self.app.id)?;
        parse_version(&self.app.version)?;
        if let Some(alias) = &self.app.alias {
            if !is_kebab(alias) {
                return Err(Error::invalid(format!(
                    "`{alias}` is not a valid CLI alias"
                )));
            }
            if RESERVED_ALIASES.contains(&alias.as_str()) {
                return Err(Error::invalid(format!("`{alias}` is a reserved command")));
            }
        }
        let (ns, pkg) = self.app.namespace.split_once(':').ok_or_else(|| {
            Error::invalid(format!(
                "`{}` is not a WIT package of the form namespace:package",
                self.app.namespace
            ))
        })?;
        if !is_kebab(ns) || !is_kebab(pkg) {
            return Err(Error::invalid(format!(
                "`{}` is not a valid WIT package",
                self.app.namespace
            )));
        }
        if Namespace::of(ns) == Namespace::Engine {
            return Err(Error::invalid(format!(
                "`{}` uses the engine's `anth` namespace; an app owns its own",
                self.app.namespace
            )));
        }

        let hybrid = self.wasm.is_some() && self.service.is_some();
        let mut wasm_exports = 0usize;
        let mut service_exports = 0usize;
        let mut exports = Vec::with_capacity(self.export.len());
        for export in &self.export {
            let id = InterfaceId::parse(&export.interface)?;
            if id.namespace_kind() == Namespace::Engine {
                return Err(Error::invalid(format!(
                    "`{}` is part of the native set; an app cannot export it",
                    export.interface
                )));
            }
            if id.package_id() != self.app.namespace {
                return Err(Error::invalid(format!(
                    "`{}` is exported but does not live under `{}`",
                    export.interface, self.app.namespace
                )));
            }
            if exports.contains(&export.interface) {
                return Err(Error::invalid(format!(
                    "`{}` is exported twice",
                    export.interface
                )));
            }
            let target = match export.target {
                Some(ExportTarget::Service) if self.manifest_version == MANIFEST_VERSION => {
                    return Err(Error::invalid(format!(
                        "`{}` is a service export; it needs `manifest_version = 3`",
                        export.interface
                    )));
                }
                Some(target) => target,
                None if hybrid => {
                    return Err(Error::invalid(format!(
                        "hybrid `[[export]]` `{}` must declare `target = \"wasm\"` or `target = \"service\"`",
                        export.interface
                    )));
                }
                None if self.wasm.is_none() && self.service.is_none() => {
                    return Err(Error::invalid(
                        "an app with `[[export]]` needs a `[wasm]` component",
                    ));
                }
                None if self.wasm.is_none() => ExportTarget::Service,
                None => ExportTarget::Wasm,
            };
            match target {
                ExportTarget::Wasm if self.wasm.is_none() => {
                    return Err(Error::invalid(format!(
                        "`{}` is a wasm export but the app has no `[wasm]`",
                        export.interface
                    )));
                }
                ExportTarget::Service if self.service.is_none() => {
                    return Err(Error::invalid(format!(
                        "`{}` is a service export but the app has no `[service]`",
                        export.interface
                    )));
                }
                ExportTarget::Wasm => wasm_exports += 1,
                ExportTarget::Service => service_exports += 1,
            }
            exports.push(export.interface.clone());
        }

        let mut imports = Vec::with_capacity(self.import.len());
        for import in &self.import {
            InterfaceId::parse(&import.interface)?;
            if imports.contains(&import.interface) {
                return Err(Error::invalid(format!(
                    "`{}` is imported twice",
                    import.interface
                )));
            }
            imports.push(import.interface.clone());
        }

        if self.wasm.is_some() && wasm_exports == 0 {
            return Err(Error::invalid(
                "a `[wasm]` section needs at least one wasm `[[export]]`",
            ));
        }
        if self.service.is_some() && service_exports == 0 {
            return Err(Error::invalid(
                "a `[service]` section needs at least one service `[[export]]`",
            ));
        }
        if let Some(wasm) = &self.wasm {
            check_relative(&wasm.path, "wasm path")?;
            if !wasm.path.ends_with(".wasm") {
                return Err(Error::invalid(format!(
                    "`{}` is not a .wasm file",
                    wasm.path
                )));
            }
            if !is_sha256(&wasm.sha256) {
                return Err(Error::invalid("the wasm `sha256` is not 64 lowercase hex"));
            }
        }

        let mut routes = Vec::with_capacity(self.route.len());
        for route in &self.route {
            if !ROUTE_METHODS.contains(&route.method.as_str()) {
                return Err(Error::invalid(format!(
                    "`{}` is not a route method; use one of {ROUTE_METHODS:?}",
                    route.method
                )));
            }
            check_route_path(&route.path)?;
            if route.stream && route.method == "HEAD" {
                return Err(Error::invalid(format!(
                    "route `{}` is a stream and cannot answer HEAD",
                    route.path
                )));
            }
            let (id, function) = parse_call(&route.call)?;
            if !exports.iter().any(|export| export == &id) {
                return Err(Error::invalid(format!(
                    "route `{}` calls `{}`, which the manifest does not export",
                    route.path, id
                )));
            }
            let _ = function;
            let key = (route.method.clone(), route.path.clone());
            if routes.contains(&key) {
                return Err(Error::invalid(format!(
                    "{} {} is declared twice",
                    route.method, route.path
                )));
            }
            routes.push(key);
        }

        let mut slots = Vec::with_capacity(self.slot.len());
        for slot in &self.slot {
            if !is_slot_id(&slot.id) {
                return Err(Error::invalid(format!(
                    "`{}` is not a valid slot id",
                    slot.id
                )));
            }
            if !is_kebab(&slot.tag) {
                return Err(Error::invalid(format!("`{}` is not a valid tag", slot.tag)));
            }
            check_module(&slot.module)?;
            if !is_sha256(&slot.sha256) {
                return Err(Error::invalid(
                    "the UI module `sha256` is not 64 lowercase hex",
                ));
            }
            let key = (slot.id.clone(), slot.tag.clone());
            if slots.contains(&key) {
                return Err(Error::invalid(format!(
                    "slot `{}` is contributed twice by `{}`",
                    slot.id, slot.tag
                )));
            }
            slots.push(key);
        }

        let mut verbs = Vec::with_capacity(self.cli.len());
        for cli in &self.cli {
            let (id, _) = parse_call(&cli.call)?;
            if !exports.iter().any(|export| export == &id) {
                return Err(Error::invalid(format!(
                    "CLI verb `{}` calls `{}`, which the manifest does not export",
                    cli.verb, id
                )));
            }
            if !is_kebab(&cli.verb) {
                return Err(Error::invalid(format!(
                    "`{}` is not a valid CLI verb",
                    cli.verb
                )));
            }
            if verbs.contains(&cli.verb) {
                return Err(Error::invalid(format!(
                    "`{}` is declared twice as a CLI verb",
                    cli.verb
                )));
            }
            verbs.push(cli.verb.clone());
        }

        for (name, value) in [
            ("fuel", self.limits.fuel),
            ("memory-pages", self.limits.memory_pages.map(u64::from)),
            ("wall-ms", self.limits.wall_ms),
            ("bytes-total", self.limits.bytes_total),
            ("bytes-per-sec", self.limits.bytes_per_sec),
        ] {
            if value == Some(0) {
                return Err(Error::invalid(format!(
                    "`{name}` must be greater than zero"
                )));
            }
        }

        Ok(())
    }
}

/// `interface#function`, both validated.
/// Splits `interface#function`, validating both halves. Public because the API
/// surface must expose the same split the manifest validated.
pub fn parse_call(input: &str) -> Result<(String, String), Error> {
    let (interface, function) = input.split_once('#').ok_or_else(|| {
        Error::invalid(format!("`{input}` is not of the form interface#function"))
    })?;
    InterfaceId::parse(interface)?;
    if !is_kebab(function) {
        return Err(Error::invalid(format!(
            "`{function}` is not a valid function name"
        )));
    }
    Ok((interface.to_string(), function.to_string()))
}

/// A relative path inside the bundle.
fn check_service(service: &ServiceSection) -> Result<(), Error> {
    let name = service.path.strip_prefix("bin/").ok_or_else(|| {
        Error::invalid(format!(
            "service path `{}` must be under `bin/`",
            service.path
        ))
    })?;
    if name.is_empty() || name.contains('/') || name == "." || name == ".." || name.starts_with('.')
    {
        return Err(Error::invalid(format!(
            "service path `{}` is not `bin/<file>`",
            service.path
        )));
    }
    if !is_sha256(&service.sha256) {
        return Err(Error::invalid(
            "the service binary `sha256` is not 64 lowercase hex",
        ));
    }
    if service.argv.is_empty() || service.argv.len() > 64 {
        return Err(Error::invalid(
            "service argv must have between 1 and 64 entries",
        ));
    }
    let mut sockets = 0usize;
    let mut data = 0usize;
    for arg in &service.argv {
        if arg.len() > 4096 {
            return Err(Error::invalid("a service argv entry is longer than 4 KiB"));
        }
        if arg.contains('\0') {
            return Err(Error::invalid("a service argv entry contains a NUL byte"));
        }
        sockets += arg.matches("{socket}").count();
        data += arg.matches("{data}").count();
        let rest = arg.replace("{socket}", "").replace("{data}", "");
        if rest.contains('{') || rest.contains('}') {
            return Err(Error::invalid(
                "a service argv entry has an unknown placeholder",
            ));
        }
    }
    if sockets != 1 {
        return Err(Error::invalid(
            "service argv must contain exactly one `{socket}`",
        ));
    }
    if data > 1 {
        return Err(Error::invalid(
            "service argv may contain at most one `{data}`",
        ));
    }
    Ok(())
}

fn check_relative(path: &str, what: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::invalid(format!(
            "the {what} `{path}` is not a relative path inside the bundle"
        )));
    }
    Ok(())
}

/// A route path: leading slash, no empty or dot segments, and not `assets`.
fn check_route_path(path: &str) -> Result<(), Error> {
    let rest = path
        .strip_prefix('/')
        .ok_or_else(|| Error::invalid(format!("route path `{path}` must start with `/`")))?;
    if rest.is_empty() {
        return Err(Error::invalid("a route path cannot be the empty root"));
    }
    if rest
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::invalid(format!(
            "route path `{path}` has an empty or relative segment"
        )));
    }
    if rest == "assets" || rest.starts_with("assets/") {
        return Err(Error::invalid(format!(
            "route path `{path}` uses `assets`, which is reserved for UI modules"
        )));
    }
    Ok(())
}

/// A UI module path. Only files a browser can load.
fn check_module(module: &str) -> Result<(), Error> {
    check_relative(module, "UI module")?;
    if !(module.ends_with(".js") || module.ends_with(".mjs") || module.ends_with(".css")) {
        return Err(Error::invalid(format!(
            "the UI module `{module}` is not a .js, .mjs or .css file"
        )));
    }
    Ok(())
}

/// Slot ids are dotted and lowercase: `panel.view`, `panel.nav.secondary`.
fn is_slot_id(input: &str) -> bool {
    !input.is_empty()
        && input.len() <= 64
        && input.split('.').all(is_kebab)
        && !input.contains("..")
}

#[cfg(all(test, feature = "parse"))]
mod tests {
    use super::*;

    const VALID: &str = r#"
manifest_version = 2

[app]
id = "org.example.notes"
alias = "notes"
namespace = "example:notes"
name = "Notes"
version = "0.1.0"

[wasm]
path = "wasm/notes.wasm"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[export]]
interface = "example:notes/list@1.0.0"

[[import]]
interface = "anth:store/kv@1.0.0"

[[route]]
method = "GET"
path = "/notes"
call = "example:notes/list@1.0.0#all"

[[slot]]
id = "panel.view"
label = "Notes"
icon = "notes"
order = 10
hidden = true
tag = "anth-notes"
module = "web/notes.js"
sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[[cli]]
call = "example:notes/list@1.0.0#all"
verb = "list"

[limits]
fuel = 10_000_000
memory-pages = 256
wall-ms = 5000
"#;

    fn valid() -> Manifest {
        Manifest::parse(VALID).expect("parse")
    }

    const SERVICE: &str = r#"
manifest_version = 3

[app]
id = "org.example.notes"
alias = "notes"
namespace = "example:notes"
name = "Notes"
version = "0.1.0"

[service]
path = "bin/notes"
sha256 = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
class = "host"
argv = ["--socket", "{socket}"]

[[export]]
interface = "example:notes/list@1.0.0"

[[export]]
interface = "example:notes/view@1.0.0"

[[route]]
method = "GET"
path = "/notes"
call = "example:notes/list@1.0.0#all"

[[cli]]
call = "example:notes/list@1.0.0#all"
verb = "list"
"#;

    const HYBRID: &str = r#"
manifest_version = 3

[app]
id = "org.example.notes"
namespace = "example:notes"
name = "Notes"
version = "0.1.0"

[wasm]
path = "wasm/notes.wasm"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[service]
path = "bin/notes"
sha256 = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
class = "socket"
argv = ["--socket", "{socket}"]

[[export]]
interface = "example:notes/list@1.0.0"
target = "service"

[[export]]
interface = "example:notes/view@1.0.0"
target = "wasm"
"#;

    #[test]
    fn a_complete_manifest_validates() {
        let manifest = valid();
        manifest.validate().expect("validate");
        assert_eq!(manifest.app.id, "org.example.notes");
        assert_eq!(manifest.slot[0].order, 10);
        assert!(manifest.slot[0].hidden);
    }

    #[test]
    fn unknown_keys_are_refused() {
        let text = VALID.replace("name = \"Notes\"", "name = \"Notes\"\nnmae = \"typo\"");
        let error = Manifest::parse(&text).expect_err("unknown key");
        assert!(error.message().contains("nmae"), "{error}");
    }

    #[test]
    fn an_export_must_live_under_the_app_namespace() {
        let mut manifest = valid();
        manifest.export[0].interface = "other:notes/list@1.0.0".to_string();
        let error = manifest.validate().expect_err("namespace");
        assert!(error.message().contains("does not live under"), "{error}");
    }

    #[test]
    fn an_app_cannot_export_a_native_interface() {
        let mut manifest = valid();
        manifest.export[0].interface = "anth:env/info@1.0.0".to_string();
        manifest.route.clear();
        manifest.cli.clear();
        let error = manifest.validate().expect_err("native export");
        assert!(error.message().contains("native set"), "{error}");
    }

    #[test]
    fn an_app_cannot_use_the_engine_namespace() {
        let mut manifest = valid();
        manifest.app.namespace = "anth:tools".to_string();
        manifest.export[0].interface = "anth:tools/list@1.0.0".to_string();
        manifest.route[0].call = "anth:tools/list@1.0.0#all".to_string();
        manifest.cli[0].call = "anth:tools/list@1.0.0#all".to_string();
        let error = manifest.validate().expect_err("namespace");
        assert!(error.message().contains("owns its own"), "{error}");
    }

    #[test]
    fn an_export_needs_a_component() {
        let mut manifest = valid();
        manifest.wasm = None;
        let error = manifest.validate().expect_err("wasm");
        assert!(error.message().contains("needs a `[wasm]`"), "{error}");
    }

    #[test]
    fn a_service_manifest_validates() {
        let manifest = Manifest::parse(SERVICE).expect("parse");
        manifest.validate().expect("validate");
        assert_eq!(manifest.export.len(), 2);
        assert_eq!(manifest.service_risk(), Risk::Host);
        assert_eq!(
            manifest.interface_target("example:notes/list@1.0.0"),
            Some(ExportTarget::Service)
        );
        assert_eq!(
            manifest.interface_target("example:notes/view@1.0.0"),
            Some(ExportTarget::Service)
        );
        assert_eq!(manifest.interface_target("example:other@1.0.0"), None);
    }

    #[test]
    fn a_hybrid_manifest_validates() {
        let manifest = Manifest::parse(HYBRID).expect("parse");
        manifest.validate().expect("validate");
        assert_eq!(manifest.service_risk(), Risk::Admin);
        assert_eq!(
            manifest.interface_target("example:notes/list@1.0.0"),
            Some(ExportTarget::Service)
        );
        assert_eq!(
            manifest.interface_target("example:notes/view@1.0.0"),
            Some(ExportTarget::Wasm)
        );
    }

    #[test]
    fn a_v3_app_needs_a_half() {
        let text = SERVICE
            .split("[service]")
            .next()
            .expect("prefix")
            .to_string();
        let manifest = Manifest::parse(&text).expect("parse");
        let error = manifest.validate().expect_err("no half");
        assert!(error.message().contains("`[service]`"), "{error}");
    }

    #[test]
    fn v2_cannot_declare_a_service() {
        let mut manifest = Manifest::parse(SERVICE).expect("parse");
        manifest.manifest_version = MANIFEST_VERSION;
        let error = manifest.validate().expect_err("v2 service");
        assert!(error.message().contains("manifest_version = 3"), "{error}");
    }

    #[test]
    fn a_service_needs_an_export() {
        let mut manifest = Manifest::parse(SERVICE).expect("parse");
        manifest.export.clear();
        let error = manifest.validate().expect_err("no export");
        assert!(
            error
                .message()
                .contains("at least one service `[[export]]`"),
            "{error}"
        );
    }

    #[test]
    fn a_hybrid_export_must_declare_its_target() {
        let mut manifest = Manifest::parse(HYBRID).expect("parse");
        manifest.export[0].target = None;
        let error = manifest.validate().expect_err("ambiguous target");
        assert!(error.message().contains("must declare `target"), "{error}");
    }

    #[test]
    fn a_service_export_needs_a_service_section() {
        let mut manifest = valid();
        manifest.manifest_version = SERVICE_MANIFEST_VERSION;
        manifest.export[0].target = Some(ExportTarget::Service);
        let error = manifest.validate().expect_err("no service");
        assert!(error.message().contains("no `[service]`"), "{error}");
    }

    #[test]
    fn a_service_path_is_bin_file() {
        for path in ["apps/fs", "bin/../fs", "bin/dir/fs", "bin/.fs", "fs"] {
            let mut manifest = Manifest::parse(SERVICE).expect("parse");
            manifest.service.as_mut().expect("service").path = path.to_string();
            let error = manifest.validate().expect_err(path);
            assert!(error.message().contains("bin/"), "{path}: {error}");
        }
    }

    #[test]
    fn a_service_argv_may_name_one_data_directory() {
        let mut manifest = Manifest::parse(SERVICE).expect("parse");
        manifest.service.as_mut().expect("service").argv = vec![
            "--socket".to_string(),
            "{socket}".to_string(),
            "--state".to_string(),
            "{data}/state.json".to_string(),
        ];
        manifest.validate().expect("data dir");

        manifest.service.as_mut().expect("service").argv = vec![
            "--socket".to_string(),
            "{socket}".to_string(),
            "{data}".to_string(),
            "{data}".to_string(),
        ];
        let error = manifest.validate().expect_err("two data dirs");
        assert!(error.message().contains("at most one `{data}`"), "{error}");
    }

    #[test]
    fn a_service_argv_needs_exactly_one_socket() {
        for argv in [
            vec!["--serve".to_string()],
            vec![
                "--socket".to_string(),
                "{socket}".to_string(),
                "{socket}".to_string(),
            ],
            vec!["--socket".to_string(), "{root}".to_string()],
        ] {
            let mut manifest = Manifest::parse(SERVICE).expect("parse");
            manifest.service.as_mut().expect("service").argv = argv.clone();
            let error = manifest.validate().expect_err("argv");
            assert!(
                error.message().contains("{socket}") || error.message().contains("placeholder"),
                "{argv:?}: {error}"
            );
        }
    }

    #[test]
    fn a_service_argv_is_bounded() {
        let mut manifest = Manifest::parse(SERVICE).expect("parse");
        let service = manifest.service.as_mut().expect("service");
        service.argv = (0..65).map(|index| format!("--{index}")).collect();
        let error = manifest.validate().expect_err("argv length");
        assert!(error.message().contains("64"), "{error}");

        let mut manifest = Manifest::parse(SERVICE).expect("parse");
        let service = manifest.service.as_mut().expect("service");
        service.argv = vec!["x".repeat(4097), "{socket}".to_string()];
        let error = manifest.validate().expect_err("argv entry");
        assert!(error.message().contains("4 KiB"), "{error}");
    }

    #[test]
    fn a_service_binary_is_hashed() {
        let mut manifest = Manifest::parse(SERVICE).expect("parse");
        manifest.service.as_mut().expect("service").sha256 = "not-a-hash".to_string();
        let error = manifest.validate().expect_err("hash");
        assert!(error.message().contains("sha256"), "{error}");
    }

    #[test]
    fn a_route_can_opt_into_streaming() {
        let text = VALID.replace(
            "path = \"/notes\"\ncall = \"example:notes/list@1.0.0#all\"",
            "path = \"/notes\"\ncall = \"example:notes/list@1.0.0#all\"\nstream = true",
        );
        let manifest = Manifest::parse(&text).expect("parse");
        assert!(manifest.route[0].stream);
        manifest.validate().expect("validate");
    }

    #[test]
    fn routes_are_buffered_unless_they_opt_in() {
        let manifest = valid();
        assert!(!manifest.route[0].stream);
    }

    #[test]
    fn a_stream_route_cannot_answer_head() {
        let mut manifest = valid();
        manifest.route[0].stream = true;
        manifest.route[0].method = "HEAD".to_string();
        let error = manifest.validate().expect_err("head stream");
        assert!(error.message().contains("cannot answer HEAD"), "{error}");
    }

    #[test]
    fn two_routes_cannot_share_a_method_and_path() {
        let mut manifest = valid();
        let mut second = manifest.route[0].clone();
        second.call = "example:notes/list@1.0.0#all".to_string();
        manifest.route.push(second);
        let error = manifest.validate().expect_err("duplicate route");
        assert!(error.message().contains("declared twice"), "{error}");
    }

    #[test]
    fn assets_is_reserved_for_ui_modules() {
        let mut manifest = valid();
        manifest.route[0].path = "/assets/x".to_string();
        let error = manifest.validate().expect_err("reserved");
        assert!(error.message().contains("reserved"), "{error}");
    }

    #[test]
    fn a_route_must_use_a_declared_method() {
        let mut manifest = valid();
        manifest.route[0].method = "OPTIONS".to_string();
        let error = manifest.validate().expect_err("method");
        assert!(error.message().contains("not a route method"), "{error}");
    }

    #[test]
    fn a_reserved_alias_is_refused() {
        let mut manifest = valid();
        manifest.app.alias = Some("install".to_string());
        let error = manifest.validate().expect_err("reserved");
        assert!(error.message().contains("reserved"), "{error}");
    }

    #[test]
    fn a_duplicate_verb_is_refused() {
        let mut manifest = valid();
        let second = manifest.cli[0].clone();
        manifest.cli.push(second);
        let error = manifest.validate().expect_err("duplicate verb");
        assert!(error.message().contains("twice"), "{error}");
    }

    #[test]
    fn a_zero_limit_is_refused() {
        let mut manifest = valid();
        manifest.limits.fuel = Some(0);
        let error = manifest.validate().expect_err("fuel");
        assert!(error.message().contains("greater than zero"), "{error}");
    }

    #[test]
    fn the_cli_name_falls_back_only_to_a_free_label() {
        let partial = Manifest::parse(VALID).expect("parse");
        assert_eq!(partial.cli_name(), Some("notes"));

        let text = VALID
            .replace("alias = \"notes\"\n", "")
            .replace("org.example.notes", "org.example.install");
        let manifest = Manifest::parse(&text).expect("parse");
        assert_eq!(manifest.cli_name(), None);
    }

    #[test]
    fn a_module_must_be_loadable() {
        let mut manifest = valid();
        manifest.slot[0].module = "web/notes.txt".to_string();
        let error = manifest.validate().expect_err("module");
        assert!(error.message().contains(".js"), "{error}");
    }
}
