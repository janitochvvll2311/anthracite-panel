//! The capability graph: ownership, exact imports and surface conflicts.
//!
//! The registry is pure. It takes parsed manifests and the host catalog and
//! returns a deterministic view. It does not read disk, run a guest or write a
//! grant; the install and approval phases build on this result.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::capability::{HOST_NAMESPACE, Import, ManifestV3};
use super::grant_v3::{self, Risk};
use super::host_catalog;

/// The whole graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Graph {
    pub host: Vec<HostEntry>,
    pub apps: Vec<AppEntry>,
}

/// One capability the host provides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostEntry {
    pub id: String,
    pub version: u32,
    pub signature: String,
    pub risk: Risk,
    pub operation: String,
}

/// The state of one app in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    Enabled,
    Unresolved,
    Conflict,
}

impl AppState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Unresolved => "unresolved",
            Self::Conflict => "conflict",
        }
    }
}

/// One app and the entries it contributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppEntry {
    pub id: String,
    pub version: String,
    pub state: AppState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<Risk>,
    pub exports: Vec<ExportEntry>,
    pub imports: Vec<ImportEntry>,
    pub cli: Vec<CliEntry>,
    pub api: Vec<ApiEntry>,
    pub ui: Vec<UiEntry>,
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportEntry {
    pub id: String,
    pub version: u32,
    pub kind: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportEntry {
    pub id: String,
    pub version: u32,
    pub provider: String,
    pub resolved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<Problem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CliEntry {
    pub name: String,
    pub export: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiEntry {
    pub method: String,
    pub path: String,
    pub export: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiEntry {
    pub slot: String,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub code: String,
    pub message: String,
}

impl Problem {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

/// Builds the deterministic graph for one set of manifests.
pub fn build(manifests: &[ManifestV3]) -> Result<Graph, String> {
    let mut host = Vec::new();
    let mut host_by_id: BTreeMap<String, Vec<(u32, String)>> = BTreeMap::new();
    for export in host_catalog::exports() {
        export.signature.validate()?;
        let hash = export.signature.hash()?;
        let versions = host_by_id.entry(export.signature.id.clone()).or_default();
        if versions
            .iter()
            .any(|(version, _)| *version == export.signature.version)
        {
            return Err(format!(
                "the host exports `{}@{}` twice",
                export.signature.id, export.signature.version
            ));
        }
        versions.push((export.signature.version, hash.clone()));
        host.push(HostEntry {
            id: export.signature.id,
            version: export.signature.version,
            signature: hash,
            risk: export.risk,
            operation: export.operation.to_string(),
        });
    }
    host.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.version.cmp(&right.version))
    });

    let mut manifests: Vec<&ManifestV3> = manifests.iter().collect();
    manifests.sort_by(|left, right| left.app.id.cmp(&right.app.id));
    let mut seen_apps = BTreeSet::new();
    for manifest in &manifests {
        if !seen_apps.insert(manifest.app.id.clone()) {
            return Err(format!("the app `{}` is declared twice", manifest.app.id));
        }
        manifest.validate()?;
    }

    // UI tags are global custom elements; the first app in id order keeps one.
    let mut ui_tags: BTreeMap<String, String> = BTreeMap::new();
    let mut export_index: BTreeMap<String, String> = BTreeMap::new();
    let mut apps = Vec::new();

    for manifest in manifests {
        let mut conflict = false;
        let mut unresolved = false;
        let mut problems = Vec::new();

        let mut exports: Vec<ExportEntry> = manifest
            .export
            .iter()
            .map(|export| {
                let key = format!("{}@{}", export.id, export.version);
                if let Some(owner) = export_index.get(&key) {
                    conflict = true;
                    problems.push(Problem::new(
                        "duplicate_export",
                        format!(
                            "the export `{key}` is already provided by `{owner}`; this one is ignored"
                        ),
                    ));
                } else {
                    export_index.insert(key, manifest.app.id.clone());
                }
                Ok(ExportEntry {
                    id: export.id.clone(),
                    version: export.version,
                    kind: export.implementation.kind().to_string(),
                    signature: export.signature().hash()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        exports.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.version.cmp(&right.version))
        });

        let mut imports = Vec::new();
        for import in &manifest.import {
            imports.push(resolve_import(import, &host_by_id, &mut unresolved)?);
        }
        imports.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.version.cmp(&right.version))
        });

        for slot in &manifest.ui {
            if let Some(owner) = ui_tags.get(&slot.tag) {
                conflict = true;
                problems.push(Problem::new(
                    "ui_tag_conflict",
                    format!(
                        "the UI tag `{}` is already registered by `{owner}`",
                        slot.tag
                    ),
                ));
            } else {
                ui_tags.insert(slot.tag.clone(), manifest.app.id.clone());
            }
        }

        let mut cli: Vec<CliEntry> = manifest
            .cli
            .iter()
            .map(|command| CliEntry {
                name: command.name.clone(),
                export: command.export.clone(),
            })
            .collect();
        cli.sort_by(|left, right| left.name.cmp(&right.name));
        let mut api: Vec<ApiEntry> = manifest
            .api
            .iter()
            .map(|route| ApiEntry {
                method: route.method.to_uppercase(),
                path: route.path.clone(),
                export: route.export.clone(),
            })
            .collect();
        api.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.method.cmp(&right.method))
        });
        let mut ui: Vec<UiEntry> = manifest
            .ui
            .iter()
            .map(|slot| UiEntry {
                slot: slot.slot.clone(),
                tag: slot.tag.clone(),
            })
            .collect();
        ui.sort_by(|left, right| left.tag.cmp(&right.tag));

        let state = if conflict {
            AppState::Conflict
        } else if unresolved {
            AppState::Unresolved
        } else {
            AppState::Enabled
        };
        let risk = if state == AppState::Enabled {
            Some(grant_v3::risk(manifest)?)
        } else {
            None
        };
        apps.push(AppEntry {
            id: manifest.app.id.clone(),
            version: manifest.app.version.clone(),
            state,
            risk,
            exports,
            imports,
            cli,
            api,
            ui,
            problems,
        });
    }

    Ok(Graph { host, apps })
}

fn resolve_import(
    import: &Import,
    host_by_id: &BTreeMap<String, Vec<(u32, String)>>,
    unresolved: &mut bool,
) -> Result<ImportEntry, String> {
    let mut entry = ImportEntry {
        id: import.id.clone(),
        version: import.version,
        provider: import.provider.clone(),
        resolved: false,
        problem: None,
    };
    if import.provider != "host" {
        *unresolved = true;
        entry.problem = Some(Problem::new(
            "unsupported_provider",
            "v1 imports must use provider = \"host\"",
        ));
        return Ok(entry);
    }
    if !import.id.starts_with(&format!("{HOST_NAMESPACE}.")) {
        *unresolved = true;
        entry.problem = Some(Problem::new(
            "unsupported_namespace",
            format!("v1 imports target `{HOST_NAMESPACE}.*`"),
        ));
        return Ok(entry);
    }
    let Some(versions) = host_by_id.get(&import.id) else {
        *unresolved = true;
        entry.problem = Some(Problem::new(
            "unknown_capability",
            format!("the host does not export `{}`", import.id),
        ));
        return Ok(entry);
    };
    let Some((_, expected)) = versions
        .iter()
        .find(|(version, _)| *version == import.version)
    else {
        *unresolved = true;
        entry.problem = Some(Problem::new(
            "unknown_version",
            format!(
                "the host does not export `{}@{}`",
                import.id, import.version
            ),
        ));
        return Ok(entry);
    };
    let actual = import.signature().hash()?;
    if actual == *expected {
        entry.resolved = true;
    } else {
        *unresolved = true;
        entry.problem = Some(Problem::new(
            "signature_mismatch",
            format!(
                "the imported `{}@{}` does not match the host signature",
                import.id, import.version
            ),
        ));
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::capability::{
        ApiRoute, CliCommand, Export, Implementation, Import, ManifestV3, WasmSectionV3,
    };
    use crate::apps::manifest::{AppSection, RuntimeKind, RuntimeSection, UiSlot, WASM_FILE};
    use crate::apps::schema::Type;

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn app(id: &str) -> ManifestV3 {
        ManifestV3 {
            manifest_version: 3,
            app: AppSection {
                id: id.to_string(),
                alias: None,
                version: "0.1.0".to_string(),
                name: "Test".to_string(),
            },
            runtime: RuntimeSection {
                kind: RuntimeKind::Wasm,
            },
            wasm: WasmSectionV3 {
                path: WASM_FILE.to_string(),
                sha256: SHA.to_string(),
                abi: 4,
            },
            export: Vec::new(),
            import: Vec::new(),
            cli: Vec::new(),
            api: Vec::new(),
            ui: Vec::new(),
        }
    }

    fn host_import() -> Import {
        let host = host_catalog::exports().remove(0);
        Import {
            id: host.signature.id,
            version: host.signature.version,
            provider: "host".to_string(),
            input: host.signature.input,
            output: host.signature.output,
        }
    }

    fn own_export(id: &str) -> Export {
        Export {
            id: id.to_string(),
            version: 1,
            reason: None,
            input: Type::record([]),
            output: Type::record([]),
            implementation: Implementation::Wasm {
                entry: "action".to_string(),
            },
        }
    }

    #[test]
    fn an_exact_host_import_resolves() {
        let mut manifest = app("org.example.app");
        manifest.import.push(host_import());
        let graph = build(&[manifest]).expect("graph");
        assert_eq!(graph.apps[0].state, AppState::Enabled);
        assert_eq!(graph.apps[0].risk, Some(Risk::Read));
        assert!(graph.apps[0].imports[0].resolved);
    }

    #[test]
    fn a_missing_or_mismatched_host_import_is_unresolved() {
        let mut missing = app("org.example.app");
        missing.import.push(Import {
            id: "org.anth.host.missing".to_string(),
            version: 1,
            provider: "host".to_string(),
            input: Type::record([]),
            output: Type::record([]),
        });
        let graph = build(&[missing]).expect("graph");
        assert_eq!(graph.apps[0].state, AppState::Unresolved);
        assert_eq!(
            graph.apps[0].imports[0].problem.as_ref().unwrap().code,
            "unknown_capability"
        );

        let mut wrong = host_import();
        wrong.output = Type::string();
        let mut manifest = app("org.example.app");
        manifest.import.push(wrong);
        let graph = build(&[manifest]).expect("graph");
        assert_eq!(graph.apps[0].state, AppState::Unresolved);
        assert_eq!(
            graph.apps[0].imports[0].problem.as_ref().unwrap().code,
            "signature_mismatch"
        );
    }

    #[test]
    fn a_ui_tag_is_global_and_the_first_app_in_id_order_keeps_it() {
        let mut first = app("org.example.a");
        first.ui.push(UiSlot {
            slot: "settings.app".to_string(),
            tag: "anth-shared".to_string(),
            module: "web/shared.js".to_string(),
            label: None,
            sha256: SHA.to_string(),
        });
        let mut second = app("org.example.b");
        second.ui.push(first.ui[0].clone());

        let graph = build(&[second.clone(), first.clone()]).expect("graph");
        assert_eq!(graph.apps[0].id, "org.example.a");
        assert_eq!(graph.apps[0].state, AppState::Enabled);
        assert_eq!(graph.apps[1].id, "org.example.b");
        assert_eq!(graph.apps[1].state, AppState::Conflict);
        assert_eq!(graph.apps[1].problems[0].code, "ui_tag_conflict");
    }

    #[test]
    fn an_exact_duplicate_export_across_apps_marks_the_second_conflict() {
        let mut parent = app("org.example");
        parent.export.push(own_export("org.example.podman"));
        let mut child = app("org.example.podman");
        child.export.push(own_export("org.example.podman"));

        let graph = build(&[child, parent]).expect("graph");
        assert_eq!(graph.apps[0].id, "org.example");
        assert_eq!(graph.apps[0].state, AppState::Enabled);
        assert_eq!(graph.apps[1].id, "org.example.podman");
        assert_eq!(graph.apps[1].state, AppState::Conflict);
        assert_eq!(graph.apps[1].problems[0].code, "duplicate_export");
    }

    #[test]
    fn the_graph_is_deterministic() {
        let mut one = app("org.example.b");
        one.import.push(host_import());
        let mut two = app("org.example.a");
        two.export.push(own_export("org.example.a.action"));

        let forward = build(&[one.clone(), two.clone()]).expect("graph");
        let reverse = build(&[two, one]).expect("graph");
        assert_eq!(forward, reverse);
    }

    #[test]
    fn cli_and_api_entries_are_mapped_and_sorted() {
        let mut manifest = app("org.example.app");
        manifest.export.push(own_export("org.example.app.action"));
        manifest.cli.push(CliCommand {
            name: "action".to_string(),
            export: "org.example.app.action".to_string(),
        });
        manifest.api.push(ApiRoute {
            method: "post".to_string(),
            path: "/action".to_string(),
            export: "org.example.app.action".to_string(),
        });
        let graph = build(&[manifest]).expect("graph");
        assert_eq!(graph.apps[0].cli[0].name, "action");
        assert_eq!(graph.apps[0].api[0].method, "POST");
    }
}
