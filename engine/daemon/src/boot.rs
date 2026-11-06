//! Boot: resolve the composition and start what runs (§8.5, §6.1).
//!
//! Resolution decides; this module supplies each app's bytes, grant, budget and
//! data directory and asks the supervisor to start it. An instantiation failure
//! degrades that app and its dependents; it never takes the engine down.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use anth_compose::grant::GrantStore;
use anth_compose::registry::{Installed, Registry};
use anth_compose::resolve;
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
    let (apps, installed, problems) = registry.installed_apps(&grants)?;
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
    for problem in &problems {
        if let Ok(id) = AppId::parse(&problem.app) {
            failed.insert(id.clone());
            report
                .degraded
                .entry(id)
                .or_insert_with(|| problem.message.clone());
        }
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
        let grant = match grants.load(id) {
            Ok(Some(grant)) => grant,
            Ok(None) => {
                failed.insert(id.clone());
                report.degraded.insert(id.clone(), "unapproved".to_string());
                continue;
            }
            Err(error) => {
                failed.insert(id.clone());
                report.degraded.insert(
                    id.clone(),
                    format!("the grant is unreadable: {}", error.message()),
                );
                continue;
            }
        };
        match spec_for(state, id, app, &grant, limits, log) {
            Ok(spec) => {
                let wall = std::time::Duration::from_millis(spec.budget.wall_ms);
                match tokio::time::timeout(wall, supervisor.start(spec)).await {
                    Ok(Ok(_)) => report.started.push(id.clone()),
                    Ok(Err(error)) => {
                        failed.insert(id.clone());
                        report
                            .degraded
                            .insert(id.clone(), error.message().to_string());
                    }
                    Err(_) => {
                        failed.insert(id.clone());
                        report
                            .degraded
                            .insert(id.clone(), "start timed out".to_string());
                    }
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{Descriptor, Manifest, Tier, DESCRIPTOR_VERSION, GRANT_VERSION};
    use std::path::PathBuf;

    fn app(id: &str, wasm: Option<(&str, String)>) -> Installed {
        let wasm = match wasm {
            Some((path, sha)) => format!("\n[wasm]\npath = \"{path}\"\nsha256 = \"{sha}\"\n"),
            None => String::new(),
        };
        let text = format!(
            "manifest_version = 2\n\n[app]\nid = \"{id}\"\nnamespace = \"example:test\"\nname = \"Test\"\nversion = \"0.1.0\"\n{wasm}"
        );
        Installed {
            manifest: Manifest::parse(&text).expect("manifest"),
            descriptor: Descriptor {
                version: DESCRIPTOR_VERSION,
                app: AppId::parse(id).expect("id"),
                enabled: true,
                alias: None,
                signer: None,
                tier: Tier::ThirdParty,
                installed_at: "2026-11-06T12:00:00Z".to_string(),
            },
        }
    }

    fn grant(id: &str) -> Grant {
        Grant {
            version: GRANT_VERSION,
            app: AppId::parse(id).expect("id"),
            app_version: "0.1.0".to_string(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: None,
            risk: anth_contract::Risk::Read,
            accepted: Vec::new(),
            approved_by: "uid:1000".to_string(),
            approved_at: "2026-11-06T12:00:00Z".to_string(),
            export: Vec::new(),
            import: Vec::new(),
        }
    }

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
            "anth-spec-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("apps/org.example.test/wasm")).expect("mkdir");
        dir
    }

    #[test]
    fn a_manifest_without_wasm_is_refused() {
        let dir = scratch();
        let id = AppId::parse("org.example.test").unwrap();
        let error = spec_for(
            &dir,
            &id,
            &app("org.example.test", None),
            &grant("org.example.test"),
            &LimitsSection::default(),
            LogSettings::default(),
        )
        .expect_err("no wasm");
        assert!(error.message().contains("no wasm"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unsafe_wasm_path_is_refused() {
        let dir = scratch();
        let id = AppId::parse("org.example.test").unwrap();
        let error = spec_for(
            &dir,
            &id,
            &app("org.example.test", Some(("../escape.wasm", "a".repeat(64)))),
            &grant("org.example.test"),
            &LimitsSection::default(),
            LogSettings::default(),
        )
        .expect_err("unsafe path");
        assert!(error.message().contains("unsafe wasm path"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hash_mismatch_is_refused() {
        let dir = scratch();
        std::fs::write(dir.join("apps/org.example.test/wasm/app.wasm"), b"bytes").expect("write");
        let id = AppId::parse("org.example.test").unwrap();
        let error = spec_for(
            &dir,
            &id,
            &app("org.example.test", Some(("wasm/app.wasm", "a".repeat(64)))),
            &grant("org.example.test"),
            &LimitsSection::default(),
            LogSettings::default(),
        )
        .expect_err("hash");
        assert!(error.message().contains("hash"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_wasm_file_is_refused() {
        let dir = scratch();
        let id = AppId::parse("org.example.test").unwrap();
        let error = spec_for(
            &dir,
            &id,
            &app("org.example.test", Some(("wasm/app.wasm", "a".repeat(64)))),
            &grant("org.example.test"),
            &LimitsSection::default(),
            LogSettings::default(),
        )
        .expect_err("missing file");
        assert!(error.message().contains("reading"), "{error}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
