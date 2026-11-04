//! Approval's half of D-Risk-Recheck: the risk of an app over the **installed**
//! graph, and the grant that freezes it.
//!
//! The composition does not participate here. An operator approves an app's
//! authority before deciding where it runs, and the number must not depend on
//! the current composition: swapping a provider later is exactly what the
//! compose-time re-check exists for (§9.3).
//!
//! Only the approved app's closure is inspected. An unrelated installed app
//! that happens to be broken must not stop the operator from approving
//! something else; the registry refuses broken bundles at install time anyway.

use std::collections::{BTreeMap, BTreeSet};

use anth_contract::catalog;
use anth_contract::{AppId, Grant, InterfaceId, Risk, GRANT_VERSION};

use crate::{cycle_members, parse_interfaces, risk_over, InstalledApp, ResolutionError};

/// The effective risk of `app`, over the installed apps it can reach.
pub fn app_risk(app: &AppId, installed: &[InstalledApp]) -> Result<Risk, ResolutionError> {
    let mut imports: BTreeMap<AppId, Vec<InterfaceId>> = BTreeMap::new();
    let mut exporters: BTreeMap<InterfaceId, AppId> = BTreeMap::new();
    let mut stack = vec![app.clone()];
    while let Some(id) = stack.pop() {
        if imports.contains_key(&id) {
            continue;
        }
        let entry = find(&id, installed)?;
        entry
            .manifest
            .validate()
            .map_err(ResolutionError::InvalidManifest)?;
        let reachable = parse_interfaces(entry.manifest.import.iter().map(|i| &i.interface))?;
        for import in &reachable {
            if catalog::is_native(import) {
                continue;
            }
            // The exporter is searched by the raw interface string, so an
            // unrelated manifest is never parsed or validated to answer this.
            let candidates: Vec<&InstalledApp> = installed
                .iter()
                .filter(|candidate| {
                    candidate
                        .manifest
                        .export
                        .iter()
                        .any(|export| export.interface == import.to_string())
                })
                .collect();
            match candidates.as_slice() {
                [] => {
                    return Err(ResolutionError::UnresolvedImport {
                        app: id.to_string(),
                        interface: import.to_string(),
                    });
                }
                [candidate] => {
                    let provider = AppId::parse(&candidate.manifest.app.id)
                        .map_err(ResolutionError::InvalidManifest)?;
                    exporters.insert(import.clone(), provider.clone());
                    stack.push(provider);
                }
                _ => {
                    return Err(ResolutionError::DuplicateExport {
                        interface: import.to_string(),
                        providers: candidates
                            .iter()
                            .map(|candidate| candidate.manifest.app.id.clone())
                            .collect(),
                    });
                }
            }
        }
        imports.insert(id, reachable);
    }

    let order = order(&imports, &exporters)?;
    let risk = risk_over(
        &order,
        |id| imports.get(id).cloned().unwrap_or_default(),
        |import| exporters.get(import).cloned(),
    );
    Ok(*risk
        .get(app)
        .expect("the approved app is in its own closure"))
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
    let found = find(app, installed)?;
    found
        .manifest
        .validate()
        .map_err(ResolutionError::InvalidManifest)?;
    let risk = app_risk(app, installed)?;
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

fn find<'a>(
    app: &AppId,
    installed: &'a [InstalledApp],
) -> Result<&'a InstalledApp, ResolutionError> {
    let matches: Vec<&InstalledApp> = installed
        .iter()
        .filter(|installed| installed.manifest.app.id == app.as_str())
        .collect();
    match matches.as_slice() {
        [] => Err(ResolutionError::NotInstalled {
            app: app.to_string(),
            malformed: None,
        }),
        [one] => Ok(one),
        _ => Err(ResolutionError::DuplicateInstalled {
            app: app.to_string(),
        }),
    }
}

/// Topological order of the closure: providers before consumers.
fn order(
    imports: &BTreeMap<AppId, Vec<InterfaceId>>,
    exporters: &BTreeMap<InterfaceId, AppId>,
) -> Result<Vec<AppId>, ResolutionError> {
    let mut indegree: BTreeMap<AppId, usize> =
        imports.keys().map(|id| (id.clone(), 0usize)).collect();
    let mut consumers: BTreeMap<AppId, BTreeSet<AppId>> = BTreeMap::new();
    for (id, reachable) in imports {
        let mut providers: BTreeSet<AppId> = BTreeSet::new();
        for import in reachable {
            if let Some(provider) = exporters.get(import) {
                providers.insert(provider.clone());
            }
        }
        for provider in providers {
            consumers.entry(provider).or_default().insert(id.clone());
            *indegree.get_mut(id).expect("every app has an indegree") += 1;
        }
    }

    let mut ready: BTreeSet<AppId> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(app, _)| app.clone())
        .collect();
    let mut ordered = Vec::with_capacity(imports.len());
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
    if ordered.len() != imports.len() {
        let all: BTreeSet<AppId> = imports.keys().cloned().collect();
        return Err(ResolutionError::Cycle {
            apps: cycle_members(&consumers, &all, &ordered),
        });
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::Manifest;

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

    fn broken_manifest(id: &str) -> Manifest {
        // Parses, but does not validate: the version is not semver.
        Manifest::parse(&format!(
            "manifest_version = 2\n\n[app]\nid = \"{id}\"\nnamespace = \"example:broken\"\nname = \"Broken\"\nversion = \"nope\"\n"
        ))
        .expect("parse")
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
        let apps = chain();
        assert_eq!(app_risk(&app(PROVIDER), &apps).expect("risk"), Risk::Write);
        assert_eq!(
            app_risk(&app(CONSUMER), &apps).expect("risk"),
            Risk::Write,
            "the consumer reaches the store through the provider"
        );
    }

    #[test]
    fn approval_ignores_grants_and_composition() {
        // Nothing is approved or enabled; the installed graph still decides.
        let mut apps = chain();
        apps[0].enabled = false;
        assert_eq!(app_risk(&app(CONSUMER), &apps).expect("risk"), Risk::Write);
    }

    #[test]
    fn an_unrelated_broken_app_does_not_block_approval() {
        let mut apps = chain();
        apps.push(installed(broken_manifest("org.example.broken")));
        let grant = approve(&app(CONSUMER), &apps, approval()).expect("approval over the closure");
        assert_eq!(
            grant.risk,
            Risk::Write,
            "only the approved app's closure is inspected"
        );
    }

    #[test]
    fn an_unresolved_import_is_fatal() {
        let error = app_risk(
            &app(CONSUMER),
            &[installed(manifest(
                CONSUMER,
                "example:consumer",
                &[],
                &["example:other/x@1.0.0"],
            ))],
        )
        .expect_err("unresolved");
        assert!(matches!(error, ResolutionError::UnresolvedImport { .. }));
    }

    #[test]
    fn two_exporters_are_fatal() {
        let shared = "example:shared/x@1.0.0";
        let apps = vec![
            installed(manifest(
                "org.example.one",
                "example:shared",
                &[shared],
                &[],
            )),
            installed(manifest(
                "org.example.two",
                "example:shared",
                &[shared],
                &[],
            )),
            installed(manifest(CONSUMER, "example:consumer", &[], &[shared])),
        ];
        let error = app_risk(&app(CONSUMER), &apps).expect_err("duplicate");
        assert!(matches!(error, ResolutionError::DuplicateExport { .. }));
    }

    #[test]
    fn a_cycle_is_fatal() {
        let a = "org.example.a";
        let b = "org.example.b";
        let a_interface = "example:a/x@1.0.0";
        let b_interface = "example:b/y@1.0.0";
        let apps = vec![
            installed(manifest(a, "example:a", &[a_interface], &[b_interface])),
            installed(manifest(b, "example:b", &[b_interface], &[a_interface])),
        ];
        let error = app_risk(&app(a), &apps).expect_err("cycle");
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
