//! Boot: resolve the composition and start what runs (§8.5, §6.1).
//!
//! Resolution decides; this module supplies each app's bytes, grant, budget and
//! data directory and asks the supervisor to start it. An instantiation failure
//! degrades that app and its dependents; it never takes the engine down.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use anth_compose::grant::GrantStore;
use anth_compose::registry::{Installed, Registry};
use anth_compose::{resolve, InstalledApp};
use anth_contract::{AppId, Composition, Error, Grant};
use anth_runtime::{Budget, InstanceSpec, LogSettings};
use sha2::{Digest, Sha256};

use crate::config::LimitsSection;
use crate::supervisor::Supervisor;

#[derive(Debug, Default)]
pub struct BootReport {
    /// Apps instantiated, in dependency order.
    pub started: Vec<AppId>,
    /// Composed apps that do not run, with the reason the surface uses.
    pub degraded: BTreeMap<AppId, String>,
    /// A resolution that would refuse a boot. The engine still serves so the
    /// operator can repair the document (§8.7).
    pub fatal: Option<Error>,
}

impl BootReport {
    pub fn runs(&self, app: &AppId) -> bool {
        self.started.contains(app)
    }
}

pub async fn start_composed(
    supervisor: &Supervisor,
    state: &Path,
    composition: &Composition,
    limits: &LimitsSection,
    log: LogSettings,
) -> Result<BootReport, Error> {
    let registry = Registry::new(state);
    let grants = GrantStore::new(state);
    let (apps, _problems) = registry.catalogue()?;
    let installed = installed_apps(&registry, &grants, &apps)?;
    let resolution = match resolve(composition, &installed) {
        Ok(resolution) => resolution,
        Err(error) => {
            return Ok(BootReport {
                fatal: Some(error.into_error()),
                ..BootReport::default()
            });
        }
    };
    let mut report = BootReport::default();
    let mut dependencies: BTreeMap<AppId, Vec<AppId>> = BTreeMap::new();
    for link in &resolution.links {
        if let anth_compose::Provider::App(provider) = &link.provider {
            dependencies
                .entry(link.consumer.clone())
                .or_default()
                .push(provider.clone());
        }
    }
    let mut failed: std::collections::BTreeSet<AppId> =
        resolution.degraded.keys().cloned().collect();
    for (id, degraded) in &resolution.degraded {
        report
            .degraded
            .insert(id.clone(), degraded.reason.code().to_string());
    }
    for id in &resolution.order {
        if let Some(provider) = dependencies
            .get(id)
            .and_then(|providers| providers.iter().find(|provider| failed.contains(*provider)))
        {
            failed.insert(id.clone());
            report
                .degraded
                .insert(id.clone(), format!("`{provider}` is not running"));
            continue;
        }
        let Some(app) = apps.iter().find(|app| app.manifest.app.id == id.as_str()) else {
            failed.insert(id.clone());
            report
                .degraded
                .insert(id.clone(), "not-installed".to_string());
            continue;
        };
        let Some(grant) = grants.load(id)? else {
            failed.insert(id.clone());
            report.degraded.insert(id.clone(), "unapproved".to_string());
            continue;
        };
        match spec_for(state, id, app, &grant, limits, log) {
            Ok(spec) => match supervisor.start(spec).await {
                Ok(_) => report.started.push(id.clone()),
                Err(error) => {
                    failed.insert(id.clone());
                    report
                        .degraded
                        .insert(id.clone(), error.message().to_string());
                }
            },
            Err(error) => {
                failed.insert(id.clone());
                report
                    .degraded
                    .insert(id.clone(), error.message().to_string());
            }
        }
    }
    Ok(report)
}

fn installed_apps(
    registry: &Registry,
    grants: &GrantStore,
    apps: &[Installed],
) -> Result<Vec<InstalledApp>, Error> {
    let mut installed = Vec::with_capacity(apps.len());
    for app in apps {
        let id = AppId::parse(&app.manifest.app.id)?;
        let (grant, grant_invalid) = match grants.load(&id) {
            Ok(grant) => (grant, false),
            Err(_) => (None, true),
        };
        installed.push(InstalledApp {
            manifest: app.manifest.clone(),
            grant,
            grant_invalid,
            enabled: app.descriptor.enabled,
            material: registry.approval_material(&id).ok().flatten(),
        });
    }
    Ok(installed)
}

fn spec_for(
    state: &Path,
    id: &AppId,
    app: &Installed,
    grant: &Grant,
    limits: &LimitsSection,
    log: LogSettings,
) -> Result<InstanceSpec, Error> {
    let wasm = app
        .manifest
        .wasm
        .as_ref()
        .ok_or_else(|| Error::invalid(format!("`{id}` has no wasm component to start")))?;
    let relative = Path::new(&wasm.path);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Error::invalid(format!(
            "`{id}` names an unsafe wasm path `{}`",
            wasm.path
        )));
    }
    let path = state.join("apps").join(id.as_str()).join(relative);
    let bytes = std::fs::read(&path)
        .map_err(|error| Error::internal(format!("reading {}: {error}", path.display())))?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if hash != wasm.sha256 {
        return Err(Error::invalid(format!(
            "`{id}` wasm bytes do not match the manifest hash"
        )));
    }
    let budget = Budget::new(
        app.manifest.limits.fuel.unwrap_or(limits.fuel),
        app.manifest
            .limits
            .memory_pages
            .unwrap_or(limits.memory_pages),
        app.manifest.limits.wall_ms.unwrap_or(limits.wall_ms),
    );
    let data = state.join("data").join(id.as_str());
    Ok(InstanceSpec::new(id.as_str(), bytes, budget)
        .granting(grant.import.clone())
        .promising(grant.export.clone())
        .data_dir(data)
        .logging(log))
}
