//! The API DTOs (§6.6): the shape HTTP and the SDK share.
//!
//! They live in the contract so the SDK can re-export them instead of defining a
//! second copy that drifts. Documents are strict (`deny_unknown_fields`); views
//! are not: a client must ignore a field it is too old to know.

use serde::{Deserialize, Serialize};

use crate::descriptor::Tier;
use crate::grant::Grant;
use crate::id::AppId;

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
    pub grant: Option<Grant>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Risk;
    use crate::grant::GRANT_VERSION;
    use crate::id::InterfaceId;
    use serde_json::json;

    fn view() -> AppView {
        AppView {
            id: AppId::parse("org.example.notes").expect("id"),
            version: "0.1.0".to_string(),
            enabled: true,
            alias: Some("notes".to_string()),
            signer: None,
            tier: Tier::ThirdParty,
            installed_at: "2026-11-03T20:00:00Z".to_string(),
            grant: Some(Grant {
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
            }),
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
}
