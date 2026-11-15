//! The API DTOs (§6.6): the shape HTTP and the SDK share.
//!
//! They live in the contract so the SDK can re-export them instead of defining a
//! second copy that drifts. Documents are strict (`deny_unknown_fields`); views
//! are not: a client must ignore a field it is too old to know, and an embedded
//! document is flattened into a view so a new document field cannot break it.

use serde::{Deserialize, Serialize};

use crate::catalog::Risk;
use crate::composition::Composition;
use crate::descriptor::Tier;
use crate::grant::Grant;
use crate::id::{AppId, InterfaceId};
use crate::key::TrustedKey;
use crate::manifest::{ServiceClass, ServiceSection};

/// One installed app as the API presents it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppView {
    pub id: AppId,
    pub version: String,
    pub enabled: bool,
    /// The effective CLI alias: the operator's, then the manifest's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// The fingerprint that signed the bundle, if it was signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    pub tier: Tier,
    pub installed_at: String,
    /// The frozen grant, on the detail endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<GrantView>,
    /// Whether that grant is absent, current, stale or unreadable.
    pub grant_state: GrantState,
    /// Why the grant is unreadable, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_problem: Option<String>,
    /// The v4 service intent, when the app has a service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceView>,
}

/// How far a declared rule is enforced today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntentMode {
    /// The engine enforces it (L1 roots).
    Enforced,
    /// The engine grants the capability (privileged listeners).
    Capability,
    /// Shown at approval and frozen, not enforced (egress until L2).
    Intent,
}

/// The service intent as the approval surface presents it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceView {
    pub class: ServiceClass,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub listen: Vec<String>,
    #[serde(default)]
    pub egress: Vec<String>,
    pub roots_mode: IntentMode,
    pub listen_mode: IntentMode,
    pub egress_mode: IntentMode,
}

impl ServiceView {
    /// The mode is a property of this engine, not of the bundle. Phase 0a only
    /// freezes the intent: L1 and the port capability arrive in 0b, so until
    /// then every rule is published as `intent` rather than claiming a
    /// confinement the daemon does not yet apply.
    pub fn of(service: &ServiceSection) -> Self {
        Self {
            class: service.class,
            roots: service.roots.clone(),
            listen: service.listen.clone(),
            egress: service.egress.clone(),
            roots_mode: IntentMode::Intent,
            listen_mode: IntentMode::Intent,
            egress_mode: IntentMode::Intent,
        }
    }
}

/// The grant as a client sees it, without the strict-document rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrantView {
    pub app: AppId,
    pub app_version: String,
    pub risk: Risk,
    #[serde(default)]
    pub accepted: Vec<Risk>,
    pub approved_by: String,
    pub approved_at: String,
    #[serde(default)]
    pub export: Vec<InterfaceId>,
    #[serde(default)]
    pub import: Vec<InterfaceId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listen: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress: Vec<String>,
    pub manifest_sha256: String,
    pub wit_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl From<Grant> for GrantView {
    fn from(grant: Grant) -> Self {
        Self {
            app: grant.app,
            app_version: grant.app_version,
            risk: grant.risk,
            accepted: grant.accepted,
            approved_by: grant.approved_by,
            approved_at: grant.approved_at,
            export: grant.export,
            import: grant.import,
            roots: grant.roots,
            listen: grant.listen,
            egress: grant.egress,
            manifest_sha256: grant.manifest_sha256,
            wit_sha256: grant.wit_sha256,
            signature: grant.signature,
        }
    }
}

/// How the stored grant relates to the installed manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrantState {
    /// No grant was approved.
    None,
    /// The grant was approved for this manifest.
    Current,
    /// The grant names another version or another interface set: re-approval is
    /// needed (§9.5).
    Stale,
    /// The grant document cannot be read or does not satisfy its rules.
    Invalid,
}

/// An installed app the catalogue could not read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppProblem {
    pub app: String,
    pub message: String,
}

/// A trusted signer as the API presents it: the document plus its derived
/// fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyView {
    pub fingerprint: String,
    #[serde(flatten)]
    pub key: TrustedKey,
}

/// One app that runs, in boot order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceApp {
    pub id: AppId,
    pub version: String,
    /// A representative icon: a name or a `web/` asset with `icon_sha256`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_sha256: Option<String>,
    /// The effective risk: the maximum over its resolved imports (§9.3).
    pub risk: Risk,
    /// HTTP routes the app declares (§7.6).
    #[serde(default)]
    pub routes: Vec<SurfaceRoute>,
    /// UI slot contributions (§7.7).
    #[serde(default)]
    pub slots: Vec<SurfaceSlot>,
    /// CLI verbs (§7.9).
    #[serde(default)]
    pub cli: Vec<SurfaceCli>,
}

/// One HTTP route of a running app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRoute {
    pub method: String,
    pub path: String,
    pub interface: InterfaceId,
    pub function: String,
}

/// One UI slot contribution of a running app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSlot {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_sha256: Option<String>,
    pub order: i64,
    /// The panel renders this contribution only on request, not in navigation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// `<app-id>#<slot-tag>` of the view this contribution is grouped under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Sub-tab label when this contribution is the parent of a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    /// View tag this FAB is scoped to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// Actions of a declarative `panel.fab`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<SurfaceFabAction>,
    pub tag: String,
    pub module: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceFabAction {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_sha256: Option<String>,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub files: bool,
}

/// One CLI verb of a running app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceCli {
    pub verb: String,
    pub interface: InterfaceId,
    pub function: String,
}

/// One composed app that does not run, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DegradedApp {
    pub app: AppId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `unapproved`, `grant-stale`, `grant-invalid`, `disabled`, `blocked` or
    /// `risk-grown` (§8.5).
    pub reason: String,
    /// The interface and provider of a block, or the approved and effective
    /// risk that grew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// One resolved import of a running app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceLink {
    pub consumer: AppId,
    pub interface: InterfaceId,
    /// `engine`, or the id of the app that exports the interface.
    pub provider: String,
}

/// A resolution that would refuse a boot, carried in the view so the document
/// is still readable and repairable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FatalView {
    /// The wire code of §7.11.
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// What the composer resolved: what runs, what does not and why, and the links
/// that were built. `fatal` is set instead of failing the request when the
/// composition cannot boot: the operator needs the document to repair it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceView {
    pub apps: Vec<SurfaceApp>,
    pub degraded: Vec<DegradedApp>,
    pub links: Vec<SurfaceLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fatal: Option<FatalView>,
}

/// The composition document plus its resolution, as `GET
/// /api/engine/composition` serves it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionView {
    pub composition: Composition,
    pub surface: SurfaceView,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant::GRANT_VERSION;
    use serde_json::json;

    fn grant() -> Grant {
        Grant {
            version: GRANT_VERSION,
            app: AppId::parse("org.example.notes").expect("id"),
            app_version: "0.1.0".to_string(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: None,
            risk: Risk::Read,
            accepted: Vec::new(),
            approved_by: "operator-3f2a".to_string(),
            approved_at: "2026-11-03T20:00:00Z".to_string(),
            export: vec![InterfaceId::parse("example:notes/notes@1.0.0").expect("interface")],
            import: Vec::new(),
            roots: Vec::new(),
            listen: Vec::new(),
            egress: Vec::new(),
        }
    }

    fn view() -> AppView {
        AppView {
            id: AppId::parse("org.example.notes").expect("id"),
            version: "0.1.0".to_string(),
            enabled: true,
            alias: Some("notes".to_string()),
            signer: None,
            tier: Tier::ThirdParty,
            installed_at: "2026-11-03T20:00:00Z".to_string(),
            grant: Some(GrantView::from(grant())),
            grant_state: GrantState::Current,
            grant_problem: None,
            service: None,
        }
    }

    #[test]
    fn a_view_round_trips_through_json() {
        let view = view();
        let text = serde_json::to_string(&view).expect("render");
        assert_eq!(serde_json::from_str::<AppView>(&text).expect("parse"), view);
    }

    #[test]
    fn the_optional_fields_are_omitted_when_empty() {
        let mut view = view();
        view.alias = None;
        view.signer = None;
        view.grant = None;
        view.grant_state = GrantState::None;
        let value: serde_json::Value = serde_json::to_value(&view).expect("value");
        assert_eq!(value.get("alias"), None);
        assert_eq!(value.get("signer"), None);
        assert_eq!(value.get("grant"), None);
    }

    #[test]
    fn a_reader_may_not_know_a_field() {
        let mut value = serde_json::to_value(view()).expect("value");
        value["future"] = json!(true);
        serde_json::from_value::<AppView>(value).expect("a view is not strict");
    }

    #[test]
    fn a_future_grant_field_does_not_break_the_view() {
        let mut value = serde_json::to_value(view()).expect("value");
        value["grant"]["future_grant_field"] = json!(true);
        serde_json::from_value::<AppView>(value).expect("a grant view is not strict");
    }

    #[test]
    fn the_grant_state_has_its_four_names() {
        for (state, name) in [
            (GrantState::None, "none"),
            (GrantState::Current, "current"),
            (GrantState::Stale, "stale"),
            (GrantState::Invalid, "invalid"),
        ] {
            assert_eq!(
                serde_json::to_string(&state).expect("render"),
                format!("\"{name}\"")
            );
        }
    }

    #[test]
    fn a_key_view_round_trips_and_flattens_the_document() {
        use crate::key::{KeyAlgorithm, KEY_VERSION};
        let view = KeyView {
            fingerprint: "b".repeat(64),
            key: TrustedKey {
                version: KEY_VERSION,
                algorithm: KeyAlgorithm::Ed25519,
                public_key: "a".repeat(64),
                label: Some("laptop".to_string()),
                added_by: "uid:1000".to_string(),
                added_at: "2026-11-05T20:00:00Z".to_string(),
            },
        };
        let text = serde_json::to_string(&view).expect("render");
        let value: serde_json::Value = serde_json::from_str(&text).expect("value");
        assert_eq!(value["fingerprint"], "b".repeat(64));
        assert_eq!(value["public_key"], "a".repeat(64));
        assert_eq!(serde_json::from_str::<KeyView>(&text).expect("parse"), view);
    }

    #[test]
    fn a_composition_view_round_trips_through_json() {
        let app = AppId::parse("org.example.notes").expect("id");
        let view = CompositionView {
            composition: Composition::empty(),
            surface: SurfaceView {
                apps: vec![SurfaceApp {
                    id: app.clone(),
                    version: "0.1.0".to_string(),
                    icon: None,
                    icon_sha256: None,
                    risk: Risk::Read,
                    routes: vec![SurfaceRoute {
                        method: "GET".to_string(),
                        path: "/notes".to_string(),
                        interface: InterfaceId::parse("example:notes/notes@1.0.0")
                            .expect("interface"),
                        function: "list".to_string(),
                    }],
                    slots: vec![SurfaceSlot {
                        id: "panel.view".to_string(),
                        label: "Notes".to_string(),
                        icon: None,
                        icon_sha256: None,
                        order: 1,
                        hidden: false,
                        parent: None,
                        tab: None,
                        view: None,
                        actions: Vec::new(),
                        tag: "notes".to_string(),
                        module: "web/notes.js".to_string(),
                        sha256: "c".repeat(64),
                    }],
                    cli: vec![SurfaceCli {
                        verb: "list".to_string(),
                        interface: InterfaceId::parse("example:notes/notes@1.0.0")
                            .expect("interface"),
                        function: "list".to_string(),
                    }],
                }],
                degraded: vec![DegradedApp {
                    app: AppId::parse("org.example.consumer").expect("id"),
                    version: None,
                    reason: "blocked".to_string(),
                    detail: Some(json!({
                        "interface": "example:notes/notes@1.0.0",
                        "provider": "org.example.notes",
                    })),
                }],
                links: vec![SurfaceLink {
                    consumer: AppId::parse("org.example.consumer").expect("id"),
                    interface: InterfaceId::parse("example:notes/notes@1.0.0").expect("interface"),
                    provider: app.to_string(),
                }],
                fatal: None,
            },
        };
        let text = serde_json::to_string(&view).expect("render");
        assert_eq!(
            serde_json::from_str::<CompositionView>(&text).expect("parse"),
            view
        );
    }
}
