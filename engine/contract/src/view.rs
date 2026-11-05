//! The API DTOs (§6.6): the shape HTTP and the SDK share.
//!
//! They live in the contract so the SDK can re-export them instead of defining a
//! second copy that drifts. Documents are strict (`deny_unknown_fields`); views
//! are not: a client must ignore a field it is too old to know, and an embedded
//! document is flattened into a view so a new document field cannot break it.

use serde::{Deserialize, Serialize};

use crate::catalog::Risk;
use crate::descriptor::Tier;
use crate::grant::Grant;
use crate::id::{AppId, InterfaceId};

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
}
