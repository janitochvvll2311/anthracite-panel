//! Boot: resolve the composition and start what runs (§8.5, §6.1).
//!
//! Resolution decides; this module supplies each app's bytes, grant, budget and
//! data directory and asks the supervisor to start it. An instantiation failure
//! degrades that app and its dependents; it never takes the engine down.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

use anth_compose::grant::GrantStore;
use anth_compose::registry::{Installed, Registry};
use anth_compose::resolve;
use anth_contract::{AppId, Composition, Error, Grant, Tier};
use anth_runtime::{Budget, InstanceSpec, LogSettings};
use sha2::{Digest, Sha256};

use crate::config::LimitsSection;
use crate::service::{ServiceLaunch, ServiceSupervisor};
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

fn degrade(
    report: &mut BootReport,
    failed: &mut BTreeSet<AppId>,
    id: &AppId,
    reason: impl Into<String>,
) {
    failed.insert(id.clone());
    report.degraded.insert(id.clone(), reason.into());
}

pub async fn start_composed(
    supervisor: &Supervisor,
    services: &ServiceSupervisor,
    state: &Path,
    composition: &Composition,
    allow_third_party_service: bool,
    limits: &LimitsSection,
    log: LogSettings,
) -> Result<BootReport, Error> {
    services.clear_stale();
    let registry = Registry::new(state);
    let grants = GrantStore::new(state);
    let (apps, installed, problems) = registry.installed_apps(&grants)?;
    let mut report = BootReport::default();
    for problem in &problems {
        if let Ok(id) = AppId::parse(&problem.app) {
            report.degraded.insert(id, problem.message.clone());
        }
    }
    let resolution = match resolve(composition, &installed) {
        Ok(resolution) => resolution,
        Err(error) => {
            report.fatal = Some(error.into_error());
            return Ok(report);
        }
    };
    let mut dependencies: BTreeMap<AppId, Vec<AppId>> = BTreeMap::new();
    for link in &resolution.links {
        if let anth_compose::Provider::App(provider) = &link.provider {
            dependencies
                .entry(link.consumer.clone())
                .or_default()
                .push(provider.clone());
        }
    }
    let mut failed: BTreeSet<AppId> = resolution.degraded.keys().cloned().collect();
    for (id, degraded) in &resolution.degraded {
        report
            .degraded
            .insert(id.clone(), degraded.reason.code().to_string());
    }
    for problem in &problems {
        if let Ok(id) = AppId::parse(&problem.app) {
            failed.insert(id);
        }
    }
    for id in &resolution.order {
        if let Some(provider) = dependencies
            .get(id)
            .and_then(|providers| providers.iter().find(|provider| failed.contains(*provider)))
        {
            degrade(
                &mut report,
                &mut failed,
                id,
                format!("`{provider}` is not running"),
            );
            continue;
        }
        let Some(app) = apps.iter().find(|app| app.manifest.app.id == id.as_str()) else {
            degrade(&mut report, &mut failed, id, "not-installed");
            continue;
        };
        if app.manifest.service.is_some()
            && app.descriptor.tier == Tier::ThirdParty
            && !allow_third_party_service
        {
            degrade(
                &mut report,
                &mut failed,
                id,
                "third-party services are disabled; start with `[services] allow-third-party = true`",
            );
            continue;
        }
        let grant = match grants.load(id) {
            Ok(Some(grant)) => grant,
            Ok(None) => {
                degrade(&mut report, &mut failed, id, "unapproved");
                continue;
            }
            Err(error) => {
                degrade(
                    &mut report,
                    &mut failed,
                    id,
                    format!("the grant is unreadable: {}", error.message()),
                );
                continue;
            }
        };
        match start_one(supervisor, services, state, id, app, &grant, limits, log).await {
            Ok(()) => report.started.push(id.clone()),
            Err(error) => {
                degrade(&mut report, &mut failed, id, error.message());
            }
        }
    }
    Ok(report)
}

/// Starts both halves of an app: the service first, then the component. A
/// half that fails stops the other, so a hybrid never runs half-composed.
#[allow(clippy::too_many_arguments)]
async fn start_one(
    supervisor: &Supervisor,
    services: &ServiceSupervisor,
    state: &Path,
    id: &AppId,
    app: &Installed,
    grant: &Grant,
    limits: &LimitsSection,
    log: LogSettings,
) -> Result<(), Error> {
    let mut service_started = false;
    if app.manifest.service.is_some() {
        let launch = service_launch(state, id, app, limits)?;
        services.start(launch).await?;
        service_started = true;
    }
    if app.manifest.wasm.is_some() {
        let spec = match spec_for(state, id, app, grant, limits, log) {
            Ok(spec) => spec,
            Err(error) => {
                if service_started {
                    services.stop(id.as_str()).await;
                }
                return Err(error);
            }
        };
        let wall = std::time::Duration::from_millis(spec.budget.wall_ms);
        match tokio::time::timeout(wall, supervisor.start(spec)).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                if service_started {
                    services.stop(id.as_str()).await;
                }
                return Err(error);
            }
            Err(_) => {
                if service_started {
                    services.stop(id.as_str()).await;
                }
                return Err(Error::unavailable(format!("`{id}` start timed out")));
            }
        }
    }
    Ok(())
}

fn service_launch(
    state: &Path,
    id: &AppId,
    app: &Installed,
    limits: &LimitsSection,
) -> Result<ServiceLaunch, Error> {
    let service = app
        .manifest
        .service
        .as_ref()
        .expect("the caller checked for a service half");
    let relative = Path::new(&service.path);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Error::invalid(format!(
            "`{id}` names an unsafe service path `{}`",
            service.path
        )));
    }
    let binary = state.join("apps").join(id.as_str()).join(relative);
    let metadata = std::fs::symlink_metadata(&binary)
        .map_err(|error| Error::internal(format!("inspecting {}: {error}", binary.display())))?;
    if !metadata.is_file() {
        return Err(Error::invalid(format!(
            "{} is not a file",
            binary.display()
        )));
    }
    let bytes = std::fs::read(&binary)
        .map_err(|error| Error::internal(format!("reading {}: {error}", binary.display())))?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if hash != service.sha256 {
        return Err(Error::invalid(format!(
            "`{id}` service bytes do not match the manifest hash"
        )));
    }
    let socket = ServiceSupervisor::socket_path(state, id);
    let socket_text = socket
        .to_str()
        .ok_or_else(|| Error::internal("the state path is not UTF-8"))?;
    // `sockaddr_un.sun_path` is 108 bytes on Linux, NUL included.
    if socket_text.len() >= 108 {
        return Err(Error::invalid(format!(
            "the service socket path is {} bytes; the kernel limit is 107",
            socket_text.len()
        )));
    }
    let data_dir = state.join("data").join(id.as_str());
    std::fs::create_dir_all(&data_dir)
        .map_err(|error| Error::internal(format!("creating {}: {error}", data_dir.display())))?;
    std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700)).map_err(
        |error| Error::internal(format!("setting {} to 0700: {error}", data_dir.display())),
    )?;
    let data_text = data_dir
        .to_str()
        .ok_or_else(|| Error::internal("the state path is not UTF-8"))?;
    let shared_dir = state.join("shared");
    let shared_text = shared_dir
        .to_str()
        .ok_or_else(|| Error::internal("the state path is not UTF-8"))?;
    // The broker socket is the service's identity for peer calls (§22). The
    // listener arrives in phase 0c; until then the path is handed over and the
    // directory exists, so the argv is exactly what the contract freezes.
    let broker_dir = state.join("broker");
    std::fs::create_dir_all(&broker_dir)
        .map_err(|error| Error::internal(format!("creating {}: {error}", broker_dir.display())))?;
    std::fs::set_permissions(&broker_dir, std::fs::Permissions::from_mode(0o700)).map_err(
        |error| Error::internal(format!("setting {} to 0700: {error}", broker_dir.display())),
    )?;
    let broker_socket = broker_dir.join(format!("{id}.sock"));
    let broker_text = broker_socket
        .to_str()
        .ok_or_else(|| Error::internal("the state path is not UTF-8"))?;
    if service.uses_broker() && broker_text.len() >= 108 {
        return Err(Error::invalid(format!(
            "the broker socket path is {} bytes; the kernel limit is 107",
            broker_text.len()
        )));
    }
    let argv = service
        .argv
        .iter()
        .map(|argument| {
            argument
                .replace("{socket}", socket_text)
                .replace("{data}", data_text)
                .replace("{shared}", shared_text)
                .replace("{broker}", broker_text)
        })
        .collect();
    let exports = app
        .manifest
        .export
        .iter()
        .map(|export| (export.interface.clone(), app.manifest.export_target(export)))
        .collect();
    Ok(ServiceLaunch {
        app: id.to_string(),
        binary,
        class: service.class,
        argv,
        socket,
        data_dir,
        exports,
        start_timeout: std::time::Duration::from_millis(limits.start_timeout_ms),
    })
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
            roots: Vec::new(),
            listen: Vec::new(),
            egress: Vec::new(),
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

    #[tokio::test]
    async fn a_corrupt_catalogue_entry_is_reported_with_the_fatal() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        std::fs::set_permissions(dir.join("apps"), std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
        let broken = dir.join("apps/org.example.broken");
        std::fs::create_dir_all(&broken).expect("mkdir");
        std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let manifest = broken.join("manifest.toml");
        std::fs::write(&manifest, b"not a manifest").expect("write");
        std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let composition = Composition::parse(
            "version = 1\n\n[[profile]]\nid = \"default\"\napps = [\"org.example.broken\"]\n",
        )
        .expect("parse");
        let supervisor = Supervisor::new(std::sync::Arc::new(anth_runtime::FakeRuntime::new()));
        let services = crate::service::ServiceSupervisor::new(
            &dir,
            anth_compose::audit::AuditLog::new(&dir),
            std::time::Duration::from_secs(5),
        );
        let report = start_composed(
            &supervisor,
            &services,
            &dir,
            &composition,
            false,
            &LimitsSection::default(),
            LogSettings::default(),
        )
        .await
        .expect("boot");
        assert_eq!(
            report.fatal.as_ref().map(|error| error.kind().code()),
            Some("not-found")
        );
        let id = AppId::parse("org.example.broken").expect("id");
        assert!(
            report.degraded.contains_key(&id),
            "the catalogue problem is kept: {:?}",
            report.degraded
        );
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
