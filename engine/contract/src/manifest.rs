//! The manifest: an index of interfaces, routes, slots and limits.
//!
//! It carries no types — those live in WIT — and nothing in it is trusted until
//! the composer resolves it and the operator approves the resulting grant. What
//! is checked here is everything that can be checked without a component: names,
//! paths, duplicates and the rules that keep a manifest internally consistent.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::id::{is_kebab, is_sha256, parse_version, AppId, InterfaceId, Namespace};

/// The manifest schema this version speaks.
pub const MANIFEST_VERSION: u32 = 2;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSection {
    pub interface: String,
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

    /// Every rule that does not need a component or the catalog.
    pub fn validate(&self) -> Result<(), Error> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(Error::invalid(format!(
                "manifest_version {} is not supported; this engine speaks {MANIFEST_VERSION}",
                self.manifest_version
            )));
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

        if exports.is_empty() && self.wasm.is_some() {
            return Err(Error::invalid(
                "a `[wasm]` section needs at least one `[[export]]`; remove one or the other",
            ));
        }
        if !exports.is_empty() && self.wasm.is_none() {
            return Err(Error::invalid(
                "an app with `[[export]]` needs a `[wasm]` component",
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
fn parse_call(input: &str) -> Result<(String, String), Error> {
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

    #[test]
    fn a_complete_manifest_validates() {
        let manifest = valid();
        manifest.validate().expect("validate");
        assert_eq!(manifest.app.id, "org.example.notes");
        assert_eq!(manifest.slot[0].order, 10);
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
