//! The composer's side of the platform: resolution (§8.2) and the grant store
//! (§6.8, §9.3).
//!
//! Resolution is deterministic and pure: it reads no files, starts nothing and
//! grants nothing. The registry supplies the manifests and their approval state;
//! the supervisor is what instantiates the result. The grant store is the one
//! part that touches disk: one document per app, written atomically and read by
//! id.
//!
//! Keeping the decision here means the same function answers "would this boot?"
//! offline and "what should run?" at boot.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub mod audit;
pub mod bundle;
pub mod composition;
pub mod grant;
pub mod keys;
pub mod registry;
pub mod risk;
pub mod signature;

mod store;

use anth_contract::catalog;
use anth_contract::{
    parse_version, AppId, Composition, Error, Grant, InterfaceId, Manifest, Profile, Risk,
};

/// What a grant freezes about the installed bytes (D-Wit-Hash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalMaterial {
    pub manifest_sha256: String,
    pub wit_sha256: String,
    pub signature: Option<String>,
}

impl ApprovalMaterial {
    /// Whether a grant froze exactly these bytes.
    pub fn matches(&self, grant: &Grant) -> bool {
        grant.manifest_sha256 == self.manifest_sha256
            && grant.wit_sha256 == self.wit_sha256
            && grant.signature == self.signature
    }
}

/// What the registry knows about one installed app at compose time.
#[derive(Debug, Clone)]
pub struct InstalledApp {
    pub manifest: Manifest,
    /// The approved grant, or `None` when the operator never approved the app.
    pub grant: Option<Grant>,
    /// A grant document exists but cannot be authority: corrupt, malformed or
    /// unreadable. It degrades the app rather than masquerading as "unapproved".
    pub grant_invalid: bool,
    /// The operator enabled the app.
    pub enabled: bool,
    /// The hashes and signature of the installed bytes, when they are known. A
    /// grant is only current if they match it.
    pub material: Option<ApprovalMaterial>,
}

/// The result of a successful resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The apps that run, providers before consumers. Independent apps keep
    /// their composition order.
    pub order: Vec<AppId>,
    /// Composed apps that do not run, and why. Degradation is not a failure:
    /// approving or enabling is an engine operation, so the engine must boot to
    /// let the operator fix it (§8.5).
    pub degraded: BTreeMap<AppId, Degraded>,
    /// One link per import of every app that runs.
    pub links: Vec<Link>,
    /// The effective risk of each app that runs: the maximum over the
    /// transitive closure of its resolved imports (§9.3). At approval this is
    /// compared with the risk the operator accepted; a growth blocks the app
    /// until it is re-approved (D-Risk-Recheck).
    pub risk: BTreeMap<AppId, Risk>,
}

impl Resolution {
    /// Whether the app is in the boot order.
    pub fn runs(&self, app: &AppId) -> bool {
        self.order.contains(app)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degraded {
    pub reason: DegradedReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradedReason {
    /// Installed, but no grant has been approved.
    Unapproved,
    /// A grant exists, but it was approved for different content: the app was
    /// upgraded and waits for re-approval (§9.5).
    GrantStale,
    /// A grant exists but does not satisfy its own rules. It is not authority,
    /// and approving is an engine operation, so the app degrades rather than
    /// blocking the boot that would let the operator fix it.
    GrantInvalid,
    /// Approved, but disabled by the operator.
    Disabled,
    /// Runs, but the app that provides an interface it needs does not.
    Blocked {
        interface: InterfaceId,
        provider: AppId,
    },
    /// The effective risk grew past what the grant approved: the app is blocked
    /// until it is re-approved (D-Risk-Recheck).
    RiskGrown { approved: Risk, effective: Risk },
}

impl DegradedReason {
    /// The kebab-case reason of §8.5, shared by the API view and the boot log.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unapproved => "unapproved",
            Self::GrantStale => "grant-stale",
            Self::GrantInvalid => "grant-invalid",
            Self::Disabled => "disabled",
            Self::Blocked { .. } => "blocked",
            Self::RiskGrown { .. } => "risk-grown",
        }
    }
}

/// Who satisfies an import.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provider {
    /// The engine's closed native set (§9.2).
    Engine,
    App(AppId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub consumer: AppId,
    pub interface: InterfaceId,
    pub provider: Provider,
}

/// A failure that makes the composition unbootable.
///
/// The three fatal cases of §8.5 share a property: the composition file or the
/// installed set is the error message, so failing is safe — the operator can fix
/// it without a running engine.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolutionError {
    /// The composition document itself is malformed.
    InvalidComposition(Error),
    /// An installed manifest is malformed (the registry should have refused it).
    InvalidManifest(Error),
    /// A grant built or supplied for approval does not satisfy its own rules.
    InvalidGrant(Error),
    /// An app the caller needs is not installed: composed, or being approved.
    /// `malformed` carries the parse error of an id in the installed set that
    /// could not be read: the app is still missing, but the operator should
    /// know the other problem exists.
    NotInstalled {
        app: String,
        malformed: Option<String>,
    },
    /// The composition pins a version the installed app does not have.
    VersionMismatch {
        app: String,
        wanted: String,
        installed: String,
    },
    /// Two installed manifests claim the same id (the registry should prevent it).
    DuplicateInstalled { app: String },
    /// Two composed apps export the same interface id (§8.3).
    DuplicateExport {
        interface: String,
        providers: Vec<String>,
    },
    /// An import has no provider at all and is not native.
    UnresolvedImport { app: String, interface: String },
    /// Typed links cannot be cyclic (§8.2, step 6).
    Cycle { apps: Vec<String> },
}

impl fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidComposition(error) => {
                write!(f, "the composition is invalid: {error}")
            }
            Self::InvalidManifest(error) => write!(f, "an installed manifest is invalid: {error}"),
            Self::InvalidGrant(error) => write!(f, "the grant is invalid: {error}"),
            Self::NotInstalled { app, malformed } => {
                write!(f, "`{app}` is not installed")?;
                if let Some(malformed) = malformed {
                    write!(
                        f,
                        "; the installed set also has a malformed id: {malformed}"
                    )?;
                }
                Ok(())
            }
            Self::VersionMismatch {
                app,
                wanted,
                installed,
            } => write!(
                f,
                "`{app}` is pinned to {wanted}, and the installed app is {installed}"
            ),
            Self::DuplicateInstalled { app } => {
                write!(f, "`{app}` is installed twice; one id is one app")
            }
            Self::DuplicateExport {
                interface,
                providers,
            } => write!(
                f,
                "`{interface}` is exported by {}; the interface id must name exactly one app",
                providers.join(", ")
            ),
            Self::UnresolvedImport { app, interface } => write!(
                f,
                "`{app}` imports `{interface}`, which nothing composed provides"
            ),
            Self::Cycle { apps } => {
                write!(f, "typed links cannot be cyclic: {}", apps.join(" -> "))
            }
        }
    }
}

impl std::error::Error for ResolutionError {}

impl ResolutionError {
    /// The contract error at the API boundary: the code and the message a client
    /// sees.
    pub fn into_error(self) -> Error {
        let message = self.to_string();
        match self {
            Self::InvalidComposition(error)
            | Self::InvalidManifest(error)
            | Self::InvalidGrant(error) => error,
            Self::NotInstalled { app, .. } => Error::not_found(format!("`{app}` is not installed")),
            Self::DuplicateInstalled { app } => {
                Error::conflict(format!("`{app}` is installed twice"))
            }
            Self::VersionMismatch { .. }
            | Self::DuplicateExport { .. }
            | Self::UnresolvedImport { .. }
            | Self::Cycle { .. } => Error::invalid(message),
        }
    }
}

/// The composed apps that import an interface `app` exports.
///
/// Only apps the composition names count: an installed app that nobody composes
/// cannot break when this one is disabled, removed or revoked (§7.9).
pub fn composed_dependents(
    composition: &Composition,
    installed: &[InstalledApp],
    app: &AppId,
) -> Result<Vec<AppId>, Error> {
    let find = |id: &AppId| {
        installed
            .iter()
            .find(|candidate| candidate.manifest.app.id == id.as_str())
    };
    let Some(target) = find(app) else {
        return Ok(Vec::new());
    };
    let exports: BTreeSet<InterfaceId> =
        parse_interfaces(target.manifest.export.iter().map(|e| &e.interface))
            .map_err(ResolutionError::into_error)?
            .into_iter()
            .collect();
    let composed = composition
        .profile
        .first()
        .map(|profile| profile.apps.as_slice())
        .unwrap_or(&[]);
    let mut dependents = Vec::new();
    for entry in composed {
        let id = AppId::parse(Profile::app_id(entry))?;
        if &id == app {
            continue;
        }
        let Some(other) = find(&id) else {
            continue;
        };
        let imports: BTreeSet<InterfaceId> =
            parse_interfaces(other.manifest.import.iter().map(|i| &i.interface))
                .map_err(ResolutionError::into_error)?
                .into_iter()
                .collect();
        if !exports.is_disjoint(&imports) {
            dependents.push(id);
        }
    }
    Ok(dependents)
}

/// What the installed set says about an app's authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Approval {
    /// A grant that matches this manifest, with the risk it approved.
    Granted(Risk),
    /// No grant at all.
    Unapproved,
    /// A grant for different content: the app changed and needs re-approval.
    Stale,
    /// A grant that does not satisfy its own rules: corrupt, not authority.
    Invalid,
}

/// One composed app, parsed into the ids resolution compares.
struct Entry {
    id: AppId,
    approval: Approval,
    enabled: bool,
    exports: Vec<InterfaceId>,
    imports: Vec<InterfaceId>,
    /// The authority the app adds by existing: its `[service]` class.
    own_risk: Risk,
}

impl Entry {
    fn runs(&self) -> bool {
        self.enabled && matches!(self.approval, Approval::Granted(_))
    }

    fn approved_risk(&self) -> Option<Risk> {
        match self.approval {
            Approval::Granted(risk) => Some(risk),
            Approval::Unapproved | Approval::Stale | Approval::Invalid => None,
        }
    }

    /// Why the app does not run. Only meaningful when `runs` is false.
    fn degraded(&self) -> DegradedReason {
        match self.approval {
            Approval::Unapproved => DegradedReason::Unapproved,
            Approval::Stale => DegradedReason::GrantStale,
            Approval::Invalid => DegradedReason::GrantInvalid,
            Approval::Granted(_) => DegradedReason::Disabled,
        }
    }
}

/// Resolves a composition into the apps that run, in boot order.
pub fn resolve(
    composition: &Composition,
    installed: &[InstalledApp],
) -> Result<Resolution, ResolutionError> {
    composition
        .validate()
        .map_err(ResolutionError::InvalidComposition)?;

    // Installed apps are only inspected when the composition names them: a
    // broken app that nobody composes must not stop a boot. A malformed id is
    // remembered so a missing app is not diagnosed as "not installed" when the
    // installed set itself is the problem.
    let mut installed_by_id: BTreeMap<AppId, Vec<&InstalledApp>> = BTreeMap::new();
    let mut malformed: Option<String> = None;
    for app in installed {
        match AppId::parse(&app.manifest.app.id) {
            Ok(id) => installed_by_id.entry(id).or_default().push(app),
            Err(error) => {
                malformed.get_or_insert_with(|| error.message().to_string());
            }
        }
    }

    // Step 2: each composed id resolves to exactly one installed manifest.
    let mut composed: Vec<Entry> = Vec::with_capacity(composition.profile[0].apps.len());
    for entry in &composition.profile[0].apps {
        let id_text = Profile::app_id(entry);
        let id = AppId::parse(id_text).map_err(ResolutionError::InvalidComposition)?;
        let found = match installed_by_id.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
            [] => {
                return Err(ResolutionError::NotInstalled {
                    app: entry.clone(),
                    malformed,
                });
            }
            [found] => *found,
            _ => {
                return Err(ResolutionError::DuplicateInstalled {
                    app: id.to_string(),
                });
            }
        };
        found
            .manifest
            .validate()
            .map_err(ResolutionError::InvalidManifest)?;
        if let Some(pin) = Profile::app_version(entry) {
            let wanted = parse_version(pin).map_err(ResolutionError::InvalidComposition)?;
            let installed_version = parse_version(&found.manifest.app.version)
                .map_err(ResolutionError::InvalidManifest)?;
            if wanted != installed_version {
                return Err(ResolutionError::VersionMismatch {
                    app: id.to_string(),
                    wanted: pin.to_string(),
                    installed: found.manifest.app.version.clone(),
                });
            }
        }
        let exports = parse_interfaces(found.manifest.export.iter().map(|e| &e.interface))?;
        let imports = parse_interfaces(found.manifest.import.iter().map(|i| &i.interface))?;
        let approval = if found.grant_invalid {
            Approval::Invalid
        } else {
            match &found.grant {
                None => Approval::Unapproved,
                Some(grant) => match grant.validate() {
                    Err(_) => Approval::Invalid,
                    Ok(())
                        if grant_matches(&found.manifest, grant, &exports, &imports)
                            && found
                                .material
                                .as_ref()
                                .is_none_or(|material| material.matches(grant)) =>
                    {
                        Approval::Granted(grant.risk)
                    }
                    Ok(()) => Approval::Stale,
                },
            }
        };
        composed.push(Entry {
            id,
            approval,
            enabled: found.enabled,
            exports,
            imports,
            own_risk: found.manifest.service_risk(),
        });
    }

    // Step 4, first half: an interface has one provider or none. Two is a
    // configuration error whatever the apps' state.
    let mut exporters: BTreeMap<InterfaceId, Vec<&Entry>> = BTreeMap::new();
    for entry in &composed {
        for export in &entry.exports {
            exporters.entry(export.clone()).or_default().push(entry);
        }
    }
    for (interface, providers) in &exporters {
        if providers.len() > 1 {
            return Err(ResolutionError::DuplicateExport {
                interface: interface.to_string(),
                providers: providers
                    .iter()
                    .map(|provider| provider.id.to_string())
                    .collect(),
            });
        }
    }

    // Step 3: only apps that can run contribute imports. Apps that cannot run
    // start degraded; a provider that does not run blocks its consumers.
    let mut running: BTreeSet<AppId> = composed
        .iter()
        .filter(|entry| entry.runs())
        .map(|entry| entry.id.clone())
        .collect();
    let mut degraded: BTreeMap<AppId, Degraded> = composed
        .iter()
        .filter(|entry| !entry.runs())
        .map(|entry| {
            (
                entry.id.clone(),
                Degraded {
                    reason: entry.degraded(),
                },
            )
        })
        .collect();

    // Step 4: every import of an app that will run needs a provider. This is
    // checked before blocking: unresolved is a hole in the composition, and a
    // composition that would not boot must not look bootable because the app
    // that shows the hole is also blocked.
    for entry in &composed {
        if !running.contains(&entry.id) {
            continue;
        }
        for import in &entry.imports {
            if !exporters.contains_key(import) && !catalog::is_native(import) {
                return Err(ResolutionError::UnresolvedImport {
                    app: entry.id.to_string(),
                    interface: import.to_string(),
                });
            }
        }
    }

    // Steps 6 and 7 plus D-Risk-Recheck. An app whose effective risk grew past
    // what its grant approved is blocked, and blocking it can block its
    // consumers, so the pass repeats until the running set stops changing.
    loop {
        loop {
            let mut blocked: Vec<(AppId, DegradedReason)> = Vec::new();
            for entry in &composed {
                if !running.contains(&entry.id) {
                    continue;
                }
                for import in &entry.imports {
                    let providers = exporters.get(import).map(Vec::as_slice).unwrap_or(&[]);
                    match providers {
                        // A native or missing provider was decided above.
                        [] => continue,
                        [provider] => {
                            if !running.contains(&provider.id) {
                                blocked.push((
                                    entry.id.clone(),
                                    DegradedReason::Blocked {
                                        interface: import.clone(),
                                        provider: provider.id.clone(),
                                    },
                                ));
                                break;
                            }
                        }
                        // The duplicate check above already refused more than one.
                        _ => unreachable!("duplicate exporters are refused before resolution"),
                    }
                }
            }
            if blocked.is_empty() {
                break;
            }
            for (app, reason) in blocked {
                running.remove(&app);
                degraded.insert(app, Degraded { reason });
            }
        }

        let links = collect_links(&composed, &running, &exporters);
        let order = boot_order(&composed, &running, &links)?;
        let risk = effective_risk(&composed, &order, &exporters);

        let mut grown: Vec<(AppId, Risk, Risk)> = Vec::new();
        for entry in &composed {
            if !running.contains(&entry.id) {
                continue;
            }
            let Some(approved) = entry.approved_risk() else {
                continue;
            };
            let effective = risk[&entry.id];
            if effective > approved {
                grown.push((entry.id.clone(), approved, effective));
            }
        }
        if grown.is_empty() {
            return Ok(Resolution {
                order,
                degraded,
                links,
                risk,
            });
        }
        for (app, approved, effective) in grown {
            running.remove(&app);
            degraded.insert(
                app,
                Degraded {
                    reason: DegradedReason::RiskGrown {
                        approved,
                        effective,
                    },
                },
            );
        }
    }
}

/// The links the running apps hold, sorted so the output is stable.
fn collect_links(
    composed: &[Entry],
    running: &BTreeSet<AppId>,
    exporters: &BTreeMap<InterfaceId, Vec<&Entry>>,
) -> Vec<Link> {
    let mut links = Vec::new();
    for entry in composed {
        if !running.contains(&entry.id) {
            continue;
        }
        for import in &entry.imports {
            let provider = match exporters.get(import).and_then(|p| p.first()) {
                Some(provider) => Provider::App(provider.id.clone()),
                None => Provider::Engine,
            };
            links.push(Link {
                consumer: entry.id.clone(),
                interface: import.clone(),
                provider,
            });
        }
    }
    links.sort_by(|a, b| {
        (&a.consumer, &a.interface, &a.provider).cmp(&(&b.consumer, &b.interface, &b.provider))
    });
    links
}

/// Kahn's algorithm with a `BTreeSet` frontier: independent apps keep their
/// composition order, so two runs of the same input produce the same surface.
fn boot_order(
    composed: &[Entry],
    running: &BTreeSet<AppId>,
    links: &[Link],
) -> Result<Vec<AppId>, ResolutionError> {
    let position = |id: &AppId| {
        composed
            .iter()
            .position(|entry| &entry.id == id)
            .unwrap_or(usize::MAX)
    };
    let mut indegree: BTreeMap<AppId, usize> =
        running.iter().map(|app| (app.clone(), 0usize)).collect();
    let mut consumers: BTreeMap<AppId, BTreeSet<AppId>> = BTreeMap::new();
    for link in links {
        let Provider::App(provider) = &link.provider else {
            continue;
        };
        if consumers
            .entry(provider.clone())
            .or_default()
            .insert(link.consumer.clone())
        {
            *indegree
                .get_mut(&link.consumer)
                .expect("every running app has an indegree") += 1;
        }
    }

    let mut ready: BTreeSet<(usize, AppId)> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(app, _)| (position(app), app.clone()))
        .collect();
    let mut order = Vec::with_capacity(running.len());
    while let Some((_, app)) = ready.pop_first() {
        order.push(app.clone());
        if let Some(next) = consumers.get(&app) {
            for consumer in next {
                let degree = indegree
                    .get_mut(consumer)
                    .expect("a consumer was registered");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert((position(consumer), consumer.clone()));
                }
            }
        }
    }
    if order.len() != running.len() {
        return Err(ResolutionError::Cycle {
            apps: cycle_members(&consumers, running, &order),
        });
    }
    Ok(order)
}

/// The apps actually inside a cycle.
///
/// The leftovers of Kahn are the cycles plus everything downstream of them, so
/// naming all leftovers blames apps that would boot as soon as the cycle is
/// gone. A node is in a cycle when it can reach itself by following consumer
/// edges.
pub(crate) fn cycle_members(
    consumers: &BTreeMap<AppId, BTreeSet<AppId>>,
    running: &BTreeSet<AppId>,
    ordered: &[AppId],
) -> Vec<String> {
    let mut cyclic = Vec::new();
    for start in running.iter().filter(|app| !ordered.contains(app)) {
        let mut seen: BTreeSet<&AppId> = BTreeSet::new();
        let mut stack: Vec<&AppId> = consumers
            .get(start)
            .map(|next| next.iter().collect())
            .unwrap_or_default();
        let mut found = false;
        while let Some(node) = stack.pop() {
            if node == start {
                found = true;
                break;
            }
            if !seen.insert(node) {
                continue;
            }
            if let Some(next) = consumers.get(node) {
                stack.extend(next.iter());
            }
        }
        if found {
            cyclic.push(start.to_string());
        }
    }
    cyclic
}

/// Risk travels from provider to consumer: an app is as risky as the riskiest
/// thing it can reach. Boot order visits providers first, so one pass suffices.
fn effective_risk(
    composed: &[Entry],
    order: &[AppId],
    exporters: &BTreeMap<InterfaceId, Vec<&Entry>>,
) -> BTreeMap<AppId, Risk> {
    risk_over(
        order,
        |id| {
            composed
                .iter()
                .find(|entry| &entry.id == id)
                .map(|entry| entry.own_risk)
                .unwrap_or(Risk::Read)
        },
        |id| {
            composed
                .iter()
                .find(|entry| &entry.id == id)
                .map(|entry| entry.imports.clone())
                .unwrap_or_default()
        },
        |import| {
            exporters
                .get(import)
                .and_then(|providers| providers.first())
                .map(|provider| provider.id.clone())
        },
    )
}

/// The risk rule, in one place: an app is as risky as the riskiest thing it can
/// reach, and the boot order guarantees providers are computed first.
///
/// Both the compose-time re-check and the approval decide with this function;
/// only the graph they feed it differs.
pub(crate) fn risk_over(
    order: &[AppId],
    own_risk_of: impl Fn(&AppId) -> Risk,
    imports_of: impl Fn(&AppId) -> Vec<InterfaceId>,
    provider_of: impl Fn(&InterfaceId) -> Option<AppId>,
) -> BTreeMap<AppId, Risk> {
    let mut risk: BTreeMap<AppId, Risk> = BTreeMap::new();
    for id in order {
        // A service adds authority by existing, before any import: its class is
        // part of the app's risk and of its consumers' (§19 §3).
        let mut level = own_risk_of(id);
        for import in imports_of(id) {
            let reached = match provider_of(&import) {
                Some(provider) => *risk
                    .get(&provider)
                    .expect("providers are ordered before their consumers"),
                None => catalog::risk_of(&import).expect("an unprovided import is native"),
            };
            level = level.max(reached);
        }
        risk.insert(id.clone(), level);
    }
    risk
}

pub(crate) fn parse_interfaces<'a>(
    values: impl Iterator<Item = &'a String>,
) -> Result<Vec<InterfaceId>, ResolutionError> {
    values
        .map(|value| InterfaceId::parse(value).map_err(ResolutionError::InvalidManifest))
        .collect()
}

/// Whether the grant was approved for exactly this manifest.
///
/// A grant freezes the version and the interface sets. Hashes are verified by
/// the registry, which is the layer that owns the bundle bytes; here the
/// version and the sets are enough to tell "approved for this app" from "the
/// app was upgraded and the operator has not looked at it yet".
pub(crate) fn grant_matches(
    manifest: &Manifest,
    grant: &Grant,
    exports: &[InterfaceId],
    imports: &[InterfaceId],
) -> bool {
    grant.app.as_str() == manifest.app.id
        && grant.app_version == manifest.app.version
        && same_interfaces(&grant.export, exports)
        && same_interfaces(&grant.import, imports)
}

pub(crate) fn same_interfaces(left: &[InterfaceId], right: &[InterfaceId]) -> bool {
    let mut left: Vec<&InterfaceId> = left.iter().collect();
    let mut right: Vec<&InterfaceId> = right.iter().collect();
    left.sort();
    right.sort();
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{Profile, COMPOSITION_VERSION};

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

    fn grant_for(manifest: &Manifest, risk: Risk) -> Grant {
        Grant {
            version: anth_contract::GRANT_VERSION,
            app: AppId::parse(&manifest.app.id).expect("app id"),
            app_version: manifest.app.version.clone(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: None,
            risk,
            accepted: Grant::required_levels(risk).collect(),
            approved_by: "operator-test".to_string(),
            approved_at: "2026-11-03T20:00:00Z".to_string(),
            export: manifest
                .export
                .iter()
                .map(|export| InterfaceId::parse(&export.interface).expect("export"))
                .collect(),
            import: manifest
                .import
                .iter()
                .map(|import| InterfaceId::parse(&import.interface).expect("import"))
                .collect(),
        }
    }

    /// An approved app. The grant approves the maximum risk so a test that is
    /// not about a risk growth never trips one.
    fn installed(manifest: Manifest) -> InstalledApp {
        let grant = grant_for(&manifest, Risk::Package);
        InstalledApp {
            manifest,
            grant: Some(grant),
            enabled: true,
            grant_invalid: false,
            material: None,
        }
    }

    fn composition(apps: &[&str]) -> Composition {
        Composition {
            version: COMPOSITION_VERSION,
            profile: vec![Profile {
                id: "default".to_string(),
                apps: apps.iter().map(|app| app.to_string()).collect(),
            }],
        }
    }

    fn app(id: &str) -> AppId {
        AppId::parse(id).expect("app id")
    }

    fn interface(id: &str) -> InterfaceId {
        InterfaceId::parse(id).expect("interface id")
    }

    const GREETING: &str = "example:provider/greeting@1.0.0";
    const CONSUMER: &str = "org.example.consumer";
    const PROVIDER: &str = "org.example.provider";

    fn provider() -> InstalledApp {
        installed(manifest(PROVIDER, "example:provider", &[GREETING], &[]))
    }

    fn consumer(imports: &[&str]) -> InstalledApp {
        installed(manifest(CONSUMER, "example:consumer", &[], imports))
    }

    #[test]
    fn a_provider_boots_before_its_consumer() {
        let resolution = resolve(
            &composition(&[CONSUMER, PROVIDER]),
            &[consumer(&[GREETING]), provider()],
        )
        .expect("resolve");
        assert_eq!(resolution.order, [app(PROVIDER), app(CONSUMER)]);
        assert!(resolution.degraded.is_empty());
        assert_eq!(
            resolution.links,
            [Link {
                consumer: app(CONSUMER),
                interface: interface(GREETING),
                provider: Provider::App(app(PROVIDER)),
            }]
        );
    }

    #[test]
    fn a_native_import_is_provided_by_the_engine() {
        let resolution = resolve(
            &composition(&[CONSUMER]),
            &[consumer(&["anth:store/kv@1.0.0"])],
        )
        .expect("resolve");
        assert_eq!(resolution.order, [app(CONSUMER)]);
        assert_eq!(
            resolution.links[0].provider,
            Provider::Engine,
            "a native interface needs no app"
        );
    }

    #[test]
    fn risk_is_the_maximum_over_what_an_app_can_reach() {
        // The consumer only imports the provider's greeting, but the provider
        // reaches the store: the authority travels with the link.
        let provider = installed(manifest(
            PROVIDER,
            "example:provider",
            &[GREETING],
            &["anth:store/kv@1.0.0"],
        ));
        let resolution = resolve(
            &composition(&[CONSUMER, PROVIDER]),
            &[consumer(&[GREETING]), provider],
        )
        .expect("resolve");
        assert_eq!(resolution.risk[&app(PROVIDER)], Risk::Write);
        assert_eq!(resolution.risk[&app(CONSUMER)], Risk::Write);
    }

    #[test]
    fn environment_reads_are_read_risk() {
        let resolution = resolve(
            &composition(&[CONSUMER]),
            &[consumer(&["anth:env/time@1.0.0"])],
        )
        .expect("resolve");
        assert_eq!(resolution.risk[&app(CONSUMER)], Risk::Read);
    }

    #[test]
    fn an_unresolved_import_is_fatal() {
        let error = resolve(
            &composition(&[CONSUMER]),
            &[consumer(&["example:other/x@1.0.0"])],
        )
        .expect_err("unresolved");
        assert_eq!(
            error,
            ResolutionError::UnresolvedImport {
                app: CONSUMER.to_string(),
                interface: "example:other/x@1.0.0".to_string(),
            }
        );
    }

    #[test]
    fn two_exporters_of_one_interface_are_fatal() {
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
        let error = resolve(
            &composition(&["org.example.one", "org.example.two"]),
            &[first, second],
        )
        .expect_err("duplicate");
        assert_eq!(
            error,
            ResolutionError::DuplicateExport {
                interface: "example:shared/x@1.0.0".to_string(),
                providers: vec!["org.example.one".to_string(), "org.example.two".to_string()],
            }
        );
    }

    #[test]
    fn a_composed_app_must_be_installed() {
        let error = resolve(&composition(&[PROVIDER]), &[]).expect_err("missing");
        assert_eq!(
            error,
            ResolutionError::NotInstalled {
                app: PROVIDER.to_string(),
                malformed: None,
            }
        );
    }

    #[test]
    fn a_pin_that_does_not_match_is_fatal() {
        let error =
            resolve(&composition(&["org.example.provider@9.9.9"]), &[provider()]).expect_err("pin");
        assert_eq!(
            error,
            ResolutionError::VersionMismatch {
                app: PROVIDER.to_string(),
                wanted: "9.9.9".to_string(),
                installed: "0.1.0".to_string(),
            }
        );
    }

    #[test]
    fn a_matching_pin_resolves() {
        let resolution =
            resolve(&composition(&["org.example.provider@0.1.0"]), &[provider()]).expect("resolve");
        assert_eq!(resolution.order, [app(PROVIDER)]);
    }

    #[test]
    fn an_unapproved_provider_degrades_its_consumer() {
        let mut provider = provider();
        provider.grant = None;
        let resolution = resolve(
            &composition(&[CONSUMER, PROVIDER]),
            &[consumer(&[GREETING]), provider],
        )
        .expect("resolve");
        assert!(resolution.order.is_empty());
        assert_eq!(
            resolution.degraded[&app(PROVIDER)].reason,
            DegradedReason::Unapproved
        );
        assert_eq!(
            resolution.degraded[&app(CONSUMER)].reason,
            DegradedReason::Blocked {
                interface: interface(GREETING),
                provider: app(PROVIDER),
            }
        );
        assert!(resolution.links.is_empty(), "a blocked app holds no links");
    }

    #[test]
    fn a_disabled_provider_degrades_its_consumer() {
        let mut provider = provider();
        provider.enabled = false;
        let resolution = resolve(
            &composition(&[CONSUMER, PROVIDER]),
            &[consumer(&[GREETING]), provider],
        )
        .expect("resolve");
        assert_eq!(
            resolution.degraded[&app(PROVIDER)].reason,
            DegradedReason::Disabled
        );
        assert_eq!(
            resolution.degraded[&app(CONSUMER)].reason,
            DegradedReason::Blocked {
                interface: interface(GREETING),
                provider: app(PROVIDER),
            }
        );
    }

    #[test]
    fn blocking_is_transitive() {
        let a = "org.example.a";
        let b = "org.example.b";
        let c = "org.example.c";
        let a_interface = "example:a/x@1.0.0";
        let b_interface = "example:b/y@1.0.0";
        let mut provider_a = installed(manifest(a, "example:a", &[a_interface], &[]));
        provider_a.grant = None;
        let middle = installed(manifest(b, "example:b", &[b_interface], &[a_interface]));
        let last = installed(manifest(c, "example:c", &[], &[b_interface]));
        let resolution =
            resolve(&composition(&[a, b, c]), &[provider_a, middle, last]).expect("resolve");
        assert!(resolution.order.is_empty());
        assert_eq!(
            resolution.degraded[&app(a)].reason,
            DegradedReason::Unapproved
        );
        assert_eq!(
            resolution.degraded[&app(b)].reason,
            DegradedReason::Blocked {
                interface: interface(a_interface),
                provider: app(a),
            }
        );
        assert_eq!(
            resolution.degraded[&app(c)].reason,
            DegradedReason::Blocked {
                interface: interface(b_interface),
                provider: app(b),
            }
        );
    }

    #[test]
    fn a_grown_risk_blocks_the_app_and_its_consumers() {
        let manifest = manifest(
            PROVIDER,
            "example:provider",
            &[GREETING],
            &["anth:store/kv@1.0.0"],
        );
        let provider = InstalledApp {
            grant: Some(grant_for(&manifest, Risk::Read)),
            manifest,
            enabled: true,
            grant_invalid: false,
            material: None,
        };
        let resolution = resolve(
            &composition(&[CONSUMER, PROVIDER]),
            &[consumer(&[GREETING]), provider],
        )
        .expect("resolve");
        assert!(resolution.order.is_empty());
        assert_eq!(
            resolution.degraded[&app(PROVIDER)].reason,
            DegradedReason::RiskGrown {
                approved: Risk::Read,
                effective: Risk::Write,
            }
        );
        assert_eq!(
            resolution.degraded[&app(CONSUMER)].reason,
            DegradedReason::Blocked {
                interface: interface(GREETING),
                provider: app(PROVIDER),
            }
        );
    }

    #[test]
    fn a_consumer_whose_inherited_risk_grew_is_blocked() {
        // The provider is within its grant; the consumer approved `read` for an
        // import that now reaches the store. Only the consumer grows.
        let provider = installed(manifest(
            PROVIDER,
            "example:provider",
            &[GREETING],
            &["anth:store/kv@1.0.0"],
        ));
        let manifest = manifest(CONSUMER, "example:consumer", &[], &[GREETING]);
        let consumer = InstalledApp {
            grant: Some(grant_for(&manifest, Risk::Read)),
            manifest,
            enabled: true,
            grant_invalid: false,
            material: None,
        };
        let resolution =
            resolve(&composition(&[CONSUMER, PROVIDER]), &[consumer, provider]).expect("resolve");
        assert_eq!(resolution.order, [app(PROVIDER)]);
        assert_eq!(
            resolution.degraded[&app(CONSUMER)].reason,
            DegradedReason::RiskGrown {
                approved: Risk::Read,
                effective: Risk::Write,
            }
        );
    }

    #[test]
    fn a_risk_within_the_grant_runs() {
        let manifest = manifest(
            PROVIDER,
            "example:provider",
            &[GREETING],
            &["anth:store/kv@1.0.0"],
        );
        let provider = InstalledApp {
            grant: Some(grant_for(&manifest, Risk::Write)),
            manifest,
            enabled: true,
            grant_invalid: false,
            material: None,
        };
        let resolution = resolve(&composition(&[PROVIDER]), &[provider]).expect("resolve");
        assert_eq!(resolution.order, [app(PROVIDER)]);
        assert_eq!(resolution.risk[&app(PROVIDER)], Risk::Write);
    }

    #[test]
    fn a_grant_approved_for_another_version_is_stale() {
        let mut provider = provider();
        provider.grant.as_mut().expect("grant").app_version = "9.9.9".to_string();
        let resolution = resolve(&composition(&[PROVIDER]), &[provider]).expect("resolve");
        assert!(resolution.order.is_empty());
        assert_eq!(
            resolution.degraded[&app(PROVIDER)].reason,
            DegradedReason::GrantStale
        );
    }

    #[test]
    fn a_grant_approved_for_other_interfaces_is_stale() {
        let mut provider = provider();
        provider.grant.as_mut().expect("grant").export =
            vec![interface("example:provider/other@1.0.0")];
        let resolution = resolve(&composition(&[PROVIDER]), &[provider]).expect("resolve");
        assert_eq!(
            resolution.degraded[&app(PROVIDER)].reason,
            DegradedReason::GrantStale
        );
    }

    #[test]
    fn only_composed_apps_count_as_dependents() {
        let installed = vec![provider(), consumer(&[GREETING])];
        let both = composed_dependents(
            &composition(&[CONSUMER, PROVIDER]),
            &installed,
            &app(PROVIDER),
        )
        .expect("dependents");
        assert_eq!(both, vec![app(CONSUMER)]);
        let provider_only =
            composed_dependents(&composition(&[PROVIDER]), &installed, &app(PROVIDER))
                .expect("dependents");
        assert!(provider_only.is_empty());
    }

    #[test]
    fn a_grant_that_does_not_match_the_installed_bytes_is_stale() {
        let mut provider = installed(manifest(PROVIDER, "example:provider", &[GREETING], &[]));
        provider.material = Some(ApprovalMaterial {
            manifest_sha256: "c".repeat(64),
            wit_sha256: "d".repeat(64),
            signature: None,
        });
        let resolution = resolve(&composition(&[PROVIDER]), &[provider]).expect("resolve");
        assert_eq!(
            resolution.degraded[&app(PROVIDER)].reason,
            DegradedReason::GrantStale
        );
    }

    #[test]
    fn an_invalid_grant_degrades_the_app() {
        let mut provider = provider();
        // A package-risk grant without the flags does not satisfy the document.
        provider.grant.as_mut().expect("grant").accepted.clear();
        let resolution = resolve(&composition(&[PROVIDER]), &[provider]).expect("degrade");
        assert!(resolution.order.is_empty());
        assert_eq!(
            resolution.degraded[&app(PROVIDER)].reason,
            DegradedReason::GrantInvalid
        );
    }

    #[test]
    fn a_cycle_is_fatal() {
        let a = "org.example.a";
        let b = "org.example.b";
        let a_interface = "example:a/x@1.0.0";
        let b_interface = "example:b/y@1.0.0";
        let a_app = installed(manifest(a, "example:a", &[a_interface], &[b_interface]));
        let b_app = installed(manifest(b, "example:b", &[b_interface], &[a_interface]));
        let error = resolve(&composition(&[a, b]), &[a_app, b_app]).expect_err("cycle");
        assert_eq!(
            error,
            ResolutionError::Cycle {
                apps: vec![a.to_string(), b.to_string()],
            }
        );
    }

    #[test]
    fn a_cycle_error_names_only_the_apps_in_the_cycle() {
        let a = "org.example.a";
        let b = "org.example.b";
        let c = "org.example.c";
        let a_interface = "example:a/x@1.0.0";
        let b_interface = "example:b/y@1.0.0";
        let a_app = installed(manifest(a, "example:a", &[a_interface], &[b_interface]));
        let b_app = installed(manifest(b, "example:b", &[b_interface], &[a_interface]));
        let downstream = installed(manifest(c, "example:c", &[], &[a_interface]));
        let error =
            resolve(&composition(&[a, b, c]), &[a_app, b_app, downstream]).expect_err("cycle");
        assert_eq!(
            error,
            ResolutionError::Cycle {
                apps: vec![a.to_string(), b.to_string()],
            },
            "the app waiting on the cycle is not part of it"
        );
    }

    #[test]
    fn an_installed_app_outside_the_composition_is_ignored() {
        // The registry guarantees valid manifests, but a boot must not depend
        // on an app the composition does not name.
        let broken = InstalledApp {
            manifest: Manifest::parse(
                "manifest_version = 2\n\n[app]\nid = \"org.example.broken\"\nnamespace = \"example:broken\"\nname = \"Broken\"\nversion = \"not-a-version\"\n",
            )
            .expect("parse"),
            grant: None,
            enabled: true,
            grant_invalid: false,
            material: None,
        };
        let resolution = resolve(&composition(&[PROVIDER]), &[broken, provider()])
            .expect("the composed app still resolves");
        assert_eq!(resolution.order, [app(PROVIDER)]);
    }

    #[test]
    fn a_malformed_installed_id_does_not_hide_a_missing_app() {
        // The composed app is still missing; the malformed id is reported too
        // instead of replacing the diagnosis.
        let broken = InstalledApp {
            manifest: Manifest::parse(
                "manifest_version = 2\n\n[app]\nid = \"not-an-id\"\nnamespace = \"example:broken\"\nname = \"Broken\"\nversion = \"0.1.0\"\n",
            )
            .expect("parse"),
            grant: None,
            enabled: true,
            grant_invalid: false,
            material: None,
        };
        let error = resolve(&composition(&[PROVIDER]), &[broken]).expect_err("missing");
        assert!(
            matches!(
                error,
                ResolutionError::NotInstalled {
                    malformed: Some(_),
                    ..
                }
            ),
            "{error:?}"
        );
        let text = error.to_string();
        assert!(text.contains("not installed"), "{text}");
        assert!(text.contains("not-an-id"), "{text}");
    }

    #[test]
    fn an_unapproved_app_contributes_no_imports() {
        // The import is unresolved, but this app cannot run, so it does not
        // matter; it degrades instead of failing the boot (§8.5).
        let mut blocked = consumer(&["example:other/x@1.0.0"]);
        blocked.grant = None;
        let resolution = resolve(&composition(&[CONSUMER]), &[blocked]).expect("resolve");
        assert!(resolution.order.is_empty());
        assert_eq!(
            resolution.degraded[&app(CONSUMER)].reason,
            DegradedReason::Unapproved
        );
    }

    #[test]
    fn independent_apps_keep_their_composition_order() {
        let first = installed(manifest("org.example.one", "example:one", &[], &[]));
        let second = installed(manifest("org.example.two", "example:two", &[], &[]));
        let resolution = resolve(
            &composition(&["org.example.two", "org.example.one"]),
            &[first, second],
        )
        .expect("resolve");
        assert_eq!(
            resolution.order,
            [app("org.example.two"), app("org.example.one")]
        );
    }

    #[test]
    fn an_unresolved_import_wins_over_a_blocked_provider() {
        // The app cannot run while the provider is unapproved, but the missing
        // interface is still a hole in the composition: reporting only
        // "blocked" would declare bootable what is not.
        let mut provider = provider();
        provider.grant = None;
        let consumer = consumer(&["example:other/x@1.0.0", GREETING]);
        let error = resolve(&composition(&[CONSUMER, PROVIDER]), &[consumer, provider])
            .expect_err("unresolved");
        assert_eq!(
            error,
            ResolutionError::UnresolvedImport {
                app: CONSUMER.to_string(),
                interface: "example:other/x@1.0.0".to_string(),
            }
        );
    }

    #[test]
    fn an_empty_composition_resolves_to_nothing() {
        let resolution = resolve(&Composition::empty(), &[]).expect("resolve");
        assert!(resolution.order.is_empty());
        assert!(resolution.degraded.is_empty());
        assert!(resolution.links.is_empty());
    }

    #[test]
    fn a_second_profile_is_refused() {
        let mut composition = composition(&[]);
        composition.profile.push(Profile {
            id: "other".to_string(),
            apps: Vec::new(),
        });
        let error = resolve(&composition, &[]).expect_err("profiles");
        assert!(matches!(error, ResolutionError::InvalidComposition(_)));
    }
}
