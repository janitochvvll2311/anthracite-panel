//! Grant v3: the operator's frozen approval of a v3 manifest.
//!
//! The manifest declares; the grant records what was approved. The current
//! runtime still writes v2 grants. This module is the contract the registry and
//! the approval flow will use after the migration.

use serde::{Deserialize, Serialize};

/// The grant format this module understands.
pub const GRANT_VERSION: u32 = 3;

/// One capability the operator approved.
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

impl GrantV3 {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != GRANT_VERSION {
            return Err(format!(
                "grant version {} is not {GRANT_VERSION}",
                self.version
            ));
        }
        if self.id.is_empty() || self.manifest_sha256.len() != 64 || self.wasm_sha256.len() != 64 {
            return Err("a grant names the app and pins the manifest and module".to_string());
        }
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
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> GrantV3 {
        GrantV3 {
            version: GRANT_VERSION,
            id: "org.example.app".to_string(),
            alias: None,
            name: "Example".to_string(),
            app_version: "0.1.0".to_string(),
            enabled: true,
            manifest_sha256: "a".repeat(64),
            manifest_signature: None,
            wasm_sha256: "b".repeat(64),
            risk: Risk::Admin,
            approved_at_ms: 1,
            approved_by: "uid:0".to_string(),
            capability: vec![ApprovedCapability {
                id: "org.example.app.action".to_string(),
                version: 1,
                kind: "binary".to_string(),
                implementation_sha256: Some("c".repeat(64)),
                sandbox: Some("resource".to_string()),
                resource: vec!["socket:/run/example.sock".to_string()],
                limits: Some("mem=64MiB".to_string()),
            }],
        }
    }

    #[test]
    fn risk_levels_are_ordered() {
        assert!(Risk::Read < Risk::Manage);
        assert!(Risk::Manage < Risk::Admin);
        assert!(Risk::Admin < Risk::Host);
    }

    #[test]
    fn a_grant_round_trips_and_validates() {
        let grant = grant();
        assert!(grant.validate().is_ok());
        let text = toml::to_string(&grant).expect("encode");
        let parsed: GrantV3 = toml::from_str(&text).expect("parse");
        assert_eq!(grant, parsed);
    }
}
