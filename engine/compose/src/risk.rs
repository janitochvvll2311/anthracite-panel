//! Approval's half of D-Risk-Recheck: the risk of an app over the **installed**
//! graph, and the grant that freezes it.
//!
//! The composition does not participate here. An operator approves an app's
//! authority before deciding where it runs, and the number must not depend on
//! the current composition: swapping a provider later is exactly what the
//! compose-time re-check exists for (§9.3).

use std::collections::{BTreeMap, BTreeSet};

use anth_contract::catalog;
use anth_contract::{AppId, Grant, InterfaceId, Risk, GRANT_VERSION};

use crate::{cycle_members, parse_interfaces, InstalledApp, ResolutionError};

/// One installed app, reduced to the ids the risk graph compares.
struct Entry {
    id: AppId,
    exports: Vec<InterfaceId>,
    imports: Vec<InterfaceId>,
}

/// The effective risk of every installed app.
///
/// Approval uses this to show the operator a meaningful number; it is computed
/// over what is installed, in dependency order, and never read from a manifest.
pub fn installed_risk(
    installed: &[InstalledApp],
) -> Result<BTreeMap<AppId, Risk>, ResolutionError> {
    let entries = index(installed)?;
    let exporters = exporters(&entries)?;
    let order = order(&entries, &exporters)?;
    Ok(risk_in_order(&entries, &order, &exporters))
}

/// The grant an approval produces. Everything here is operator or registry
/// input; the risk is computed, never supplied.
#[derive(Debug, Clone)]
pub struct Approval {
    /// The levels the operator presented (`--accept-write`, …).
    pub accepted: Vec<Risk>,
    pub approved_by: String,
    pub approved_at: String,
    pub manifest_sha256: String,
    pub wit_sha256: String,
    /// The detached signature, opaque to the engine.
    pub signature: Option<String>,
}

/// Builds the grant for `app` after computing its risk over the installed set.
///
/// A missing `--accept-*` level is refused here, before anything is written.
pub fn approve(
    app: &AppId,
    installed: &[InstalledApp],
    approval: Approval,
) -> Result<Grant, ResolutionError> {
    let found = installed
        .iter()
        .find(|installed| installed.manifest.app.id == app.as_str())
        .ok_or_else(|| ResolutionError::NotInstalled {
            app: app.to_string(),
            malformed: None,
        })?;
    found
        .manifest
        .validate()
        .map_err(ResolutionError::InvalidManifest)?;
    let risk = installed_risk(installed)?
        .get(app)
        .copied()
        .expect("the approved app is installed and indexed");
    let grant = Grant {
        version: GRANT_VERSION,
        app: app.clone(),
        app_version: found.manifest.app.version.clone(),
        manifest_sha256: approval.manifest_sha256,
        wit_sha256: approval.wit_sha256,
        signature: approval.signature,
        risk,
        accepted: approval.accepted,
        approved_by: approval.approved_by,
        approved_at: approval.approved_at,
        export: parse_interfaces(found.manifest.export.iter().map(|e| &e.interface))?,
        import: parse_interfaces(found.manifest.import.iter().map(|i| &i.interface))?,
    };
    grant.validate().map_err(ResolutionError::InvalidGrant)?;
    Ok(grant)
}

fn index(installed: &[InstalledApp]) -> Result<Vec<Entry>, ResolutionError> {
    let mut entries = Vec::with_capacity(installed.len());
    let mut seen: BTreeSet<AppId> = BTreeSet::new();
    for app in installed {
        app.manifest
            .validate()
            .map_err(ResolutionError::InvalidManifest)?;
        let id = AppId::parse(&app.manifest.app.id).map_err(ResolutionError::InvalidManifest)?;
        if !seen.insert(id.clone()) {
            return Err(ResolutionError::DuplicateInstalled {
                app: id.to_string(),
            });
        }
        entries.push(Entry {
            id,
            exports: parse_interfaces(app.manifest.export.iter().map(|e| &e.interface))?,
            imports: parse_interfaces(app.manifest.import.iter().map(|i| &i.interface))?,
        });
    }
    Ok(entries)
}

/// Interface id → index of its single exporter.
fn exporters(entries: &[Entry]) -> Result<BTreeMap<InterfaceId, usize>, ResolutionError> {
    let mut map: BTreeMap<InterfaceId, usize> = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        for export in &entry.exports {
            if let Some(previous) = map.insert(export.clone(), index) {
                return Err(ResolutionError::DuplicateExport {
                    interface: export.to_string(),
                    providers: vec![entries[previous].id.to_string(), entry.id.to_string()],
                });
            }
        }
    }
    for entry in entries {
        for import in &entry.imports {
            if !map.contains_key(import) && !catalog::is_native(import) {
                return Err(ResolutionError::UnresolvedImport {
                    app: entry.id.to_string(),
                    interface: import.to_string(),
                });
            }
        }
    }
    Ok(map)
}

fn order(
    entries: &[Entry],
    exporters: &BTreeMap<InterfaceId, usize>,
) -> Result<Vec<AppId>, ResolutionError> {
    let mut indegree: BTreeMap<AppId, usize> = entries
        .iter()
        .map(|entry| (entry.id.clone(), 0usize))
        .collect();
    let mut consumers: BTreeMap<AppId, BTreeSet<AppId>> = BTreeMap::new();
    for entry in entries {
        let mut providers: BTreeSet<AppId> = BTreeSet::new();
        for import in &entry.imports {
            if let Some(&index) = exporters.get(import) {
                providers.insert(entries[index].id.clone());
            }
        }
        for provider in providers {
            consumers
                .entry(provider)
                .or_default()
                .insert(entry.id.clone());
            *indegree
                .get_mut(&entry.id)
                .expect("every installed app has an indegree") += 1;
        }
    }

    let mut ready: BTreeSet<AppId> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(app, _)| app.clone())
        .collect();
    let mut ordered = Vec::with_capacity(entries.len());
    while let Some(app) = ready.pop_first() {
        ordered.push(app.clone());
        if let Some(next) = consumers.get(&app) {
            for consumer in next {
                let degree = indegree
                    .get_mut(consumer)
                    .expect("a consumer was registered");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(consumer.clone());
                }
            }
        }
    }
    if ordered.len() != entries.len() {
        let all: BTreeSet<AppId> = entries.iter().map(|entry| entry.id.clone()).collect();
        return Err(ResolutionError::Cycle {
            apps: cycle_members(&consumers, &all, &ordered),
        });
    }
    Ok(ordered)
}

fn risk_in_order(
    entries: &[Entry],
    order: &[AppId],
    exporters: &BTreeMap<InterfaceId, usize>,
) -> BTreeMap<AppId, Risk> {
    let mut risk: BTreeMap<AppId, Risk> = BTreeMap::new();
    for id in order {
        let entry = entries
            .iter()
            .find(|entry| &entry.id == id)
            .expect("the order only names indexed apps");
        let mut level = Risk::Read;
        for import in &entry.imports {
            let reached = match exporters.get(import) {
                Some(&index) => *risk
                    .get(&entries[index].id)
                    .expect("providers are ordered before their consumers"),
                None => catalog::risk_of(import).expect("an unprovided import is native"),
            };
            level = level.max(reached);
        }
        risk.insert(id.clone(), level);
    }
    risk
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{Manifest, GRANT_VERSION};

    fn manifest(id: &str, namespace: &str, exports: &[&str], imports: &[&str]) -> Manifest {
        let mut text = format!(
            "manifest_version = 2\n\n[app]\nid = \"{id}\"\nnamespace = \"{namespace}\"\nname = \"Test\"\nversion = \"0.1.0\"\n"
        );
        if !exports.is_empty() {
            text.push_str("\n[wasm]\npath = \"wasm/test.wasm\"\nsha256 = \"");
            text.push_str(&"a".repeat(64));
            text.push_str("\"\n");
        }
        for export in exports {
            text.push_str(&format!("\n[[export]]\ninterface = \"{export}\"\n"));
        }
        for import in imports {
            text.push_str(&format!("\n[[import]]\ninterface = \"{import}\"\n"));
        }
        let manifest = Manifest::parse(&text).expect("manifest parses");
        manifest.validate().expect("manifest validates");
        manifest
    }

    fn installed(manifest: Manifest) -> InstalledApp {
        InstalledApp {
            manifest,
            grant: None,
            enabled: true,
        }
    }

    fn app(id: &str) -> AppId {
        AppId::parse(id).expect("app id")
    }

    fn approval() -> Approval {
        Approval {
            accepted: vec![Risk::Write],
            approved_by: "operator-test".to_string(),
            approved_at: "2026-11-03T20:00:00Z".to_string(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: None,
        }
    }

    const GREETING: &str = "example:provider/greeting@1.0.0";
    const CONSUMER: &str = "org.example.consumer";
    const PROVIDER: &str = "org.example.provider";

    fn chain() -> Vec<InstalledApp> {
        vec![
            installed(manifest(
                PROVIDER,
                "example:provider",
                &[GREETING],
                &["anth:store/kv@1.0.0"],
            )),
            installed(manifest(CONSUMER, "example:consumer", &[], &[GREETING])),
        ]
    }

    #[test]
    fn risk_travels_through_installed_providers() {
        let risk = installed_risk(&chain()).expect("risk");
        assert_eq!(risk[&app(PROVIDER)], Risk::Write);
        assert_eq!(
            risk[&app(CONSUMER)],
            Risk::Write,
            "the consumer reaches the store through the provider"
        );
    }

    #[test]
    fn approval_ignores_grants_and_composition() {
        // Nothing is approved or enabled; the installed graph still decides.
        let mut apps = chain();
        apps[0].enabled = false;
        let risk = installed_risk(&apps).expect("risk");
        assert_eq!(risk[&app(CONSUMER)], Risk::Write);
    }

    #[test]
    fn an_unresolved_import_is_fatal() {
        let error = installed_risk(&[installed(manifest(
            CONSUMER,
            "example:consumer",
            &[],
            &["example:other/x@1.0.0"],
        ))])
        .expect_err("unresolved");
        assert!(matches!(error, ResolutionError::UnresolvedImport { .. }));
    }

    #[test]
    fn two_exporters_are_fatal() {
        let first = installed(manifest(
            "org.example.one",
            "example:shared",
            &["example:shared/x@1.0.0"],
            &[],
        ));
        let second = installed(manifest(
            "org.example.two",
            "example:shared",
            &["example:shared/x@1.0.0"],
            &[],
        ));
        let error = installed_risk(&[first, second]).expect_err("duplicate");
        assert!(matches!(error, ResolutionError::DuplicateExport { .. }));
    }

    #[test]
    fn a_cycle_is_fatal() {
        let a = "org.example.a";
        let b = "org.example.b";
        let a_interface = "example:a/x@1.0.0";
        let b_interface = "example:b/y@1.0.0";
        let a_app = installed(manifest(a, "example:a", &[a_interface], &[b_interface]));
        let b_app = installed(manifest(b, "example:b", &[b_interface], &[a_interface]));
        let error = installed_risk(&[a_app, b_app]).expect_err("cycle");
        assert_eq!(
            error,
            ResolutionError::Cycle {
                apps: vec![a.to_string(), b.to_string()],
            }
        );
    }

    #[test]
    fn approve_builds_a_grant_with_the_computed_risk() {
        let apps = chain();
        let grant = approve(&app(PROVIDER), &apps, approval()).expect("approve");
        assert_eq!(grant.version, GRANT_VERSION);
        assert_eq!(grant.app, app(PROVIDER));
        assert_eq!(grant.risk, Risk::Write);
        assert_eq!(grant.approved_by, "operator-test");
        assert_eq!(
            grant.import,
            vec![InterfaceId::parse("anth:store/kv@1.0.0").unwrap()]
        );
        grant.validate().expect("the grant validates itself");
    }

    #[test]
    fn approve_refuses_a_missing_level() {
        let apps = chain();
        let mut approval = approval();
        approval.accepted.clear();
        let error = approve(&app(PROVIDER), &apps, approval).expect_err("missing flag");
        assert!(
            matches!(error, ResolutionError::InvalidGrant(_)),
            "{error:?}"
        );
        assert!(error.to_string().contains("--accept-write"), "{error}");
    }

    #[test]
    fn approve_refuses_an_app_that_is_not_installed() {
        let error = approve(&app(PROVIDER), &[], approval()).expect_err("missing");
        assert!(matches!(error, ResolutionError::NotInstalled { .. }));
    }

    #[test]
    fn a_read_app_needs_no_flag() {
        let apps = vec![installed(manifest(
            CONSUMER,
            "example:consumer",
            &[],
            &["anth:env/time@1.0.0"],
        ))];
        let mut approval = approval();
        approval.accepted.clear();
        let grant = approve(&app(CONSUMER), &apps, approval).expect("read is the baseline");
        assert_eq!(grant.risk, Risk::Read);
    }
}
