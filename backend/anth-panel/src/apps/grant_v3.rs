//! Grant v3: the operator's frozen approval of a v3 manifest.
//!
//! The manifest declares; the grant records what was approved. The current
//! runtime still writes v2 grants. This module is the contract the registry and
//! the approval flow use after the migration.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::capability::{
    BinarySource, Export, Implementation, ManifestV3, Resource, SandboxProfile,
};
use super::host_catalog;
use super::registry::{self, AppState};

/// The grant format this module understands.
pub const GRANT_VERSION: u32 = 3;

/// One exported capability the operator approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedCapability {
    pub id: String,
    pub version: u32,
    /// `host`, `wasm`, `binary` or `service`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    /// Normalized resource strings, e.g. `socket:/run/user/1000/podman.sock`.
    #[serde(default)]
    pub resource: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<String>,
    pub risk: Risk,
}

/// One host import the operator approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedImport {
    pub id: String,
    pub version: u32,
    pub provider: String,
    pub risk: Risk,
}

/// A frozen grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantV3 {
    pub version: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub name: String,
    pub app_version: String,
    pub enabled: bool,
    pub manifest_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_signature: Option<String>,
    pub wasm_sha256: String,
    pub risk: Risk,
    pub approved_at_ms: u64,
    pub approved_by: String,
    pub capability: Vec<ApprovedCapability>,
    #[serde(default)]
    pub import: Vec<ApprovedImport>,
}

/// The confirmation flags an install or update must present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    Capabilities,
    Admin,
    Host,
    Package,
}

impl Approval {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capabilities => "capabilities",
            Self::Admin => "admin",
            Self::Host => "host",
            Self::Package => "package",
        }
    }
}

/// Host-computed authority level. The manifest cannot set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Read,
    Manage,
    Admin,
    Host,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Manage => "manage",
            Self::Admin => "admin",
            Self::Host => "host",
        }
    }
}

/// Computes the authority level without building the normalized set.
///
/// The registry uses this for display; `from_manifest` calls the same rules
/// when it freezes a grant.
pub fn risk(manifest: &ManifestV3) -> Result<Risk, String> {
    let mut host_risk = Risk::Read;
    for import in &manifest.import {
        let host = host_catalog::find(&import.id, import.version).ok_or_else(|| {
            format!(
                "the import `{}@{}` is not in the host catalog",
                import.id, import.version
            )
        })?;
        host_risk = host_risk.max(host.risk);
    }
    let mut risk = host_risk;
    for export in &manifest.export {
        risk = risk.max(export_risk(export, host_risk)?);
    }
    Ok(risk)
}

/// The flags an operator must present for this manifest.
///
/// The levels are cumulative: a host-exec capability needs the admin and host
/// confirmations; a package needs admin and the package confirmation.
pub fn approvals(manifest: &ManifestV3) -> Result<Vec<Approval>, String> {
    let risk = risk(manifest)?;
    let mut levels = vec![Approval::Capabilities];
    if risk >= Risk::Admin {
        levels.push(Approval::Admin);
    }
    let mut host = false;
    let mut package = false;
    for export in &manifest.export {
        match &export.implementation {
            Implementation::Host { operation } => {
                if host_catalog::operation(operation).is_some_and(|host| host.risk == Risk::Host) {
                    host = true;
                }
            }
            Implementation::Binary(binary) => {
                host |= binary.sandbox == SandboxProfile::Host
                    || matches!(binary.argv, super::capability::ArgvPolicy::Any);
                package |= binary.source == BinarySource::Package;
            }
            Implementation::Service(service) => {
                host |= service.sandbox == SandboxProfile::Host;
                package |= service.source == BinarySource::Package;
            }
            Implementation::Wasm { .. } => {}
        }
    }
    if host {
        levels.push(Approval::Host);
    }
    if package {
        levels.push(Approval::Package);
    }
    Ok(levels)
}

impl GrantV3 {
    /// Normalizes a manifest into the set the operator is asked to approve.
    ///
    /// The registry must resolve every import first; an app with an unresolved
    /// or conflicting surface cannot be granted.
    pub fn from_manifest(
        manifest: &ManifestV3,
        manifest_sha256: String,
        manifest_signature: Option<String>,
        approved_by: String,
        approved_at_ms: u64,
    ) -> Result<Self, String> {
        manifest.validate()?;
        let graph = registry::build(std::slice::from_ref(manifest))?;
        let app = graph
            .apps
            .first()
            .ok_or_else(|| "the registry did not return the app".to_string())?;
        if app.state != AppState::Enabled {
            let mut detail = Vec::new();
            for problem in &app.problems {
                detail.push(format!("{}: {}", problem.code, problem.message));
            }
            for import in &app.imports {
                if let Some(problem) = &import.problem {
                    detail.push(format!("{}: {}", problem.code, problem.message));
                }
            }
            return Err(format!(
                "the app is {}: {}",
                app.state.as_str(),
                detail.join("; ")
            ));
        }

        let mut imports = Vec::new();
        let mut host_risk = Risk::Read;
        for import in &manifest.import {
            let host = host_catalog::find(&import.id, import.version).ok_or_else(|| {
                format!(
                    "the import `{}@{}` is not in the host catalog",
                    import.id, import.version
                )
            })?;
            host_risk = host_risk.max(host.risk);
            imports.push(ApprovedImport {
                id: import.id.clone(),
                version: import.version,
                provider: import.provider.clone(),
                risk: host.risk,
            });
        }
        imports.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.version.cmp(&right.version))
        });

        let mut capability = Vec::new();
        for export in &manifest.export {
            let risk = export_risk(export, host_risk)?;
            capability.push(ApprovedCapability {
                id: export.id.clone(),
                version: export.version,
                kind: export.implementation.kind().to_string(),
                implementation_sha256: implementation_hash(export, manifest),
                sandbox: sandbox(export),
                resource: resources(export),
                limits: limits(export),
                risk,
            });
        }
        capability.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.version.cmp(&right.version))
        });

        let risk = capability
            .iter()
            .map(|capability| capability.risk)
            .chain(imports.iter().map(|import| import.risk))
            .max()
            .unwrap_or(Risk::Read);

        Ok(Self {
            version: GRANT_VERSION,
            id: manifest.app.id.clone(),
            alias: manifest.app.alias.clone(),
            name: manifest.app.name.clone(),
            app_version: manifest.app.version.clone(),
            enabled: true,
            manifest_sha256,
            manifest_signature,
            wasm_sha256: manifest.wasm.sha256.clone(),
            risk,
            approved_at_ms,
            approved_by,
            capability,
            import: imports,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != GRANT_VERSION {
            return Err(format!(
                "grant version {} is not {GRANT_VERSION}",
                self.version
            ));
        }
        if self.id.is_empty() || !is_sha256(&self.manifest_sha256) || !is_sha256(&self.wasm_sha256)
        {
            return Err("a grant names the app and pins the manifest and module".to_string());
        }
        if self.approved_by.is_empty() {
            return Err("a grant records who approved it".to_string());
        }
        let mut ids = BTreeSet::new();
        for capability in &self.capability {
            if capability.id.is_empty() || capability.version == 0 {
                return Err("an approved capability has an id and a version".to_string());
            }
            if !matches!(
                capability.kind.as_str(),
                "host" | "wasm" | "binary" | "service"
            ) {
                return Err(format!(
                    "the approved capability `{}` has an unknown kind",
                    capability.id
                ));
            }
            if !ids.insert(format!("{}@{}", capability.id, capability.version)) {
                return Err(format!(
                    "the approved capability `{}@{}` is duplicated",
                    capability.id, capability.version
                ));
            }
            if let Some(hash) = &capability.implementation_sha256
                && !is_sha256(hash)
            {
                return Err(format!(
                    "the approved capability `{}` has no valid implementation hash",
                    capability.id
                ));
            }
        }
        let mut import_ids = BTreeSet::new();
        for import in &self.import {
            if import.id.is_empty() || import.version == 0 || import.provider != "host" {
                return Err("an approved import names a host capability".to_string());
            }
            if !import_ids.insert(format!("{}@{}", import.id, import.version)) {
                return Err(format!(
                    "the approved import `{}@{}` is duplicated",
                    import.id, import.version
                ));
            }
        }
        let highest = self
            .capability
            .iter()
            .map(|capability| capability.risk)
            .chain(self.import.iter().map(|import| import.risk))
            .max()
            .unwrap_or(Risk::Read);
        if self.risk < highest {
            return Err("the grant risk is lower than one of its capabilities".to_string());
        }
        Ok(())
    }
}

fn export_risk(export: &Export, host_risk: Risk) -> Result<Risk, String> {
    Ok(match &export.implementation {
        Implementation::Host { operation } => {
            let host = host_catalog::operation(operation).ok_or_else(|| {
                format!(
                    "the export `{}` uses the unknown host operation `{operation}`",
                    export.id
                )
            })?;
            host.risk
        }
        Implementation::Wasm { .. } => host_risk,
        Implementation::Binary(binary) => process_risk(
            binary.source,
            binary.sandbox,
            &binary.resource,
            matches!(binary.argv, super::capability::ArgvPolicy::Any),
        ),
        Implementation::Service(service) => {
            process_risk(service.source, service.sandbox, &service.resource, false)
        }
    })
}

fn process_risk(
    source: BinarySource,
    sandbox: SandboxProfile,
    resources: &[Resource],
    argv_any: bool,
) -> Risk {
    if sandbox == SandboxProfile::Host || argv_any || source == BinarySource::Package {
        Risk::Host
    } else if sandbox == SandboxProfile::Network || !resources.is_empty() {
        Risk::Admin
    } else {
        Risk::Manage
    }
}

fn implementation_hash(export: &Export, manifest: &ManifestV3) -> Option<String> {
    match &export.implementation {
        Implementation::Binary(binary) => Some(binary.sha256.clone()),
        Implementation::Service(service) => Some(service.sha256.clone()),
        Implementation::Wasm { .. } => Some(manifest.wasm.sha256.clone()),
        Implementation::Host { .. } => None,
    }
}

fn sandbox(export: &Export) -> Option<String> {
    match &export.implementation {
        Implementation::Binary(binary) => Some(binary.sandbox.as_str().to_string()),
        Implementation::Service(service) => Some(service.sandbox.as_str().to_string()),
        _ => None,
    }
}

fn resources(export: &Export) -> Vec<String> {
    match &export.implementation {
        Implementation::Binary(binary) => binary.resource.iter().map(Resource::canonical).collect(),
        Implementation::Service(service) => {
            service.resource.iter().map(Resource::canonical).collect()
        }
        _ => Vec::new(),
    }
}

fn limits(export: &Export) -> Option<String> {
    match &export.implementation {
        Implementation::Binary(binary) => binary.limits.canonical(),
        Implementation::Service(service) => service.limits.canonical(),
        _ => None,
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::capability::{
        ArgvPolicy, BinaryImplementation, Export, Implementation, Import, Limits, ManifestV3,
        ServiceImplementation, ServiceProtocol, WasmSectionV3,
    };
    use crate::apps::manifest::{AppSection, RuntimeKind, RuntimeSection, WASM_FILE};
    use crate::apps::schema::Type;

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn base() -> ManifestV3 {
        ManifestV3 {
            manifest_version: 3,
            app: AppSection {
                id: "org.example.app".to_string(),
                alias: None,
                version: "0.1.0".to_string(),
                name: "Example".to_string(),
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

    fn binary_export(implementation: BinaryImplementation) -> Export {
        Export {
            id: "org.example.app.action".to_string(),
            version: 1,
            reason: None,
            input: Type::record([]),
            output: Type::record([]),
            implementation: Implementation::Binary(implementation),
        }
    }

    fn binary() -> BinaryImplementation {
        BinaryImplementation {
            source: BinarySource::Bundle,
            path: "helpers/action".to_string(),
            sha256: SHA.to_string(),
            argv: ArgvPolicy::Fixed {
                args: vec!["--json".to_string()],
            },
            sandbox: SandboxProfile::Resource,
            resource: vec![Resource::Socket {
                path: "/run/example.sock".to_string(),
                alias: "example".to_string(),
            }],
            limits: Limits {
                memory_bytes: Some(64 * 1024 * 1024),
                pids: Some(32),
                wall_seconds: Some(30),
                output_bytes: None,
            },
            package: None,
            package_version: None,
        }
    }

    #[test]
    fn approvals_follow_the_authority_not_the_label() {
        let mut read = base();
        read.import.push(host_import());
        assert_eq!(
            approvals(&read).expect("read"),
            vec![Approval::Capabilities]
        );

        let mut admin = base();
        admin.export.push(binary_export(binary()));
        assert_eq!(
            approvals(&admin).expect("admin"),
            vec![Approval::Capabilities, Approval::Admin]
        );

        let mut any = binary();
        any.argv = ArgvPolicy::Any;
        any.resource.clear();
        let mut host = base();
        host.export.push(binary_export(any));
        assert_eq!(
            approvals(&host).expect("host"),
            vec![Approval::Capabilities, Approval::Admin, Approval::Host]
        );

        let mut package = binary();
        package.source = BinarySource::Package;
        package.package = Some("example".to_string());
        package.package_version = Some("1.0.0".to_string());
        package.path = "/usr/bin/example".to_string();
        package.resource.clear();
        let mut app = base();
        app.export.push(binary_export(package));
        assert_eq!(
            approvals(&app).expect("package"),
            vec![Approval::Capabilities, Approval::Admin, Approval::Package]
        );
    }

    #[test]
    fn a_host_import_sets_the_read_floor() {
        let mut manifest = base();
        manifest.import.push(host_import());
        let grant =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect("grant");
        assert_eq!(grant.risk, Risk::Read);
        assert_eq!(grant.import.len(), 1);
        assert_eq!(grant.import[0].risk, Risk::Read);
        assert!(grant.validate().is_ok());
    }

    #[test]
    fn a_resource_socket_raises_the_capability_and_app_to_admin() {
        let mut manifest = base();
        manifest.export.push(binary_export(binary()));
        let grant =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect("grant");
        assert_eq!(grant.risk, Risk::Admin);
        assert_eq!(grant.capability[0].risk, Risk::Admin);
        assert_eq!(
            grant.capability[0].resource,
            vec!["socket:/run/example.sock".to_string()]
        );
        assert_eq!(
            grant.capability[0].limits.as_deref(),
            Some("mem=67108864,pids=32,wall=30s")
        );
    }

    #[test]
    fn arbitrary_argv_or_host_sandbox_is_host_risk() {
        let mut any = binary();
        any.argv = ArgvPolicy::Any;
        any.resource.clear();
        let mut manifest = base();
        manifest.export.push(binary_export(any));
        let grant =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect("grant");
        assert_eq!(grant.risk, Risk::Host);

        let mut host = binary();
        host.sandbox = SandboxProfile::Host;
        host.resource.clear();
        let mut manifest = base();
        manifest.export.push(binary_export(host));
        let grant =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect("grant");
        assert_eq!(grant.risk, Risk::Host);
    }

    #[test]
    fn a_package_is_host_risk() {
        let mut package = binary();
        package.source = BinarySource::Package;
        package.package = Some("example".to_string());
        package.package_version = Some("1.0.0".to_string());
        package.path = "/usr/bin/example".to_string();
        let mut manifest = base();
        manifest.export.push(binary_export(package));
        let grant =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect("grant");
        assert_eq!(grant.risk, Risk::Host);
    }

    #[test]
    fn a_service_with_resources_is_admin_and_reports_its_protocol() {
        let mut manifest = base();
        manifest.export.push(Export {
            id: "org.example.app.action".to_string(),
            version: 1,
            reason: None,
            input: Type::record([]),
            output: Type::record([]),
            implementation: Implementation::Service(ServiceImplementation {
                source: BinarySource::Bundle,
                path: "helpers/action".to_string(),
                sha256: SHA.to_string(),
                protocol: ServiceProtocol::HttpFd,
                idle_seconds: 300,
                sandbox: SandboxProfile::Resource,
                resource: vec![Resource::Socket {
                    path: "/run/example.sock".to_string(),
                    alias: "example".to_string(),
                }],
                limits: Limits::default(),
                package: None,
                package_version: None,
            }),
        });
        let grant =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect("grant");
        assert_eq!(grant.capability[0].kind, "service");
        assert_eq!(grant.capability[0].risk, Risk::Admin);
        assert_eq!(grant.capability[0].sandbox.as_deref(), Some("resource"));
    }

    #[test]
    fn an_unresolved_import_cannot_be_granted() {
        let mut manifest = base();
        manifest.import.push(Import {
            id: "org.anth.host.missing".to_string(),
            version: 1,
            provider: "host".to_string(),
            input: Type::record([]),
            output: Type::record([]),
        });
        let error =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect_err("unresolved");
        assert!(error.contains("unresolved"), "{error}");
    }

    #[test]
    fn a_grant_round_trips_and_validates() {
        let mut manifest = base();
        manifest.import.push(host_import());
        manifest.export.push(binary_export(binary()));
        let grant =
            GrantV3::from_manifest(&manifest, SHA.to_string(), None, "uid:0".to_string(), 1)
                .expect("grant");
        assert!(grant.validate().is_ok());
        let text = toml::to_string(&grant).expect("encode");
        let parsed: GrantV3 = toml::from_str(&text).expect("parse");
        assert_eq!(grant, parsed);
    }
}
