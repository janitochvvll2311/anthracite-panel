//! The audit record (§6.8): one JSON object per line, append-only.
//!
//! The engine writes an entry for every install, approval, compose, enable,
//! disable, key change and denial. A denial is recorded even when nothing
//! changed: the record of what an app tried is the point of having an audit.
//!
//! The types live here and the file lives in the state tree. The daemon is the
//! only writer; the API reads the same lines.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Error;
use crate::id::AppId;

/// Every action the engine audits. The list is closed: a new action is a
/// contract change, not a free-form string in a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AuditAction {
    #[serde(rename = "app.install")]
    AppInstall,
    #[serde(rename = "app.upgrade")]
    AppUpgrade,
    #[serde(rename = "app.approve")]
    AppApprove,
    #[serde(rename = "app.revoke")]
    AppRevoke,
    #[serde(rename = "app.enable")]
    AppEnable,
    #[serde(rename = "app.disable")]
    AppDisable,
    #[serde(rename = "app.remove")]
    AppRemove,
    #[serde(rename = "compose")]
    Compose,
    #[serde(rename = "key.add")]
    KeyAdd,
    #[serde(rename = "key.remove")]
    KeyRemove,
    #[serde(rename = "acknowledge")]
    Acknowledge,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "session.login")]
    SessionLogin,
    #[serde(rename = "service.start")]
    ServiceStart,
    #[serde(rename = "service.stop")]
    ServiceStop,
    #[serde(rename = "service.crash")]
    ServiceCrash,
    #[serde(rename = "service.deny")]
    ServiceDeny,
}

impl AuditAction {
    pub const ALL: &'static [AuditAction] = &[
        AuditAction::AppInstall,
        AuditAction::AppUpgrade,
        AuditAction::AppApprove,
        AuditAction::AppRevoke,
        AuditAction::AppEnable,
        AuditAction::AppDisable,
        AuditAction::AppRemove,
        AuditAction::Compose,
        AuditAction::KeyAdd,
        AuditAction::KeyRemove,
        AuditAction::Acknowledge,
        AuditAction::Deny,
        AuditAction::SessionLogin,
        AuditAction::ServiceStart,
        AuditAction::ServiceStop,
        AuditAction::ServiceCrash,
        AuditAction::ServiceDeny,
    ];
}

/// How the action ended. `denied` is the one that must never be dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Ok,
    Denied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEntry {
    /// RFC 3339, UTC. The same shape grants use.
    pub at: String,
    /// The operator name, or `system`.
    pub actor: String,
    pub action: AuditAction,
    /// The app the action concerns, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    pub result: Outcome,
    /// What the action needs: the risk, the dependents broken, the capability
    /// denied. Free-form on purpose; the action decides its own shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl AuditEntry {
    /// One line, without the trailing newline: the store owns the framing.
    pub fn to_line(&self) -> Result<String, Error> {
        serde_json::to_string(self)
            .map_err(|error| Error::internal(format!("rendering an audit entry: {error}")))
    }

    pub fn parse_line(line: &str) -> Result<Self, Error> {
        serde_json::from_str(line)
            .map_err(|error| Error::invalid(format!("the audit entry is not valid JSON: {error}")))
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.actor.trim().is_empty() {
            return Err(Error::invalid("an audit entry needs an actor"));
        }
        humantime::parse_rfc3339(&self.at).map_err(|error| {
            Error::invalid(format!("`at` is not an RFC 3339 timestamp: {error}"))
        })?;
        if let Some(app) = &self.app {
            AppId::parse(app)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> AuditEntry {
        AuditEntry {
            at: "2026-11-03T20:00:00Z".to_string(),
            actor: "operator-3f2a".to_string(),
            action: AuditAction::AppApprove,
            app: Some("org.example.notes".to_string()),
            result: Outcome::Ok,
            detail: Some(serde_json::json!({"risk": "read"})),
        }
    }

    #[test]
    fn every_action_round_trips_through_its_name() {
        for action in AuditAction::ALL {
            let text = serde_json::to_string(action).expect("render");
            let parsed: AuditAction = serde_json::from_str(&text).expect("parse");
            assert_eq!(&parsed, action);
        }
    }

    #[test]
    fn the_action_names_are_the_documented_ones() {
        let names: Vec<String> = AuditAction::ALL
            .iter()
            .map(|action| {
                serde_json::to_string(action)
                    .unwrap()
                    .trim_matches('"')
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            [
                "app.install",
                "app.upgrade",
                "app.approve",
                "app.revoke",
                "app.enable",
                "app.disable",
                "app.remove",
                "compose",
                "key.add",
                "key.remove",
                "acknowledge",
                "deny",
                "session.login",
                "service.start",
                "service.stop",
                "service.crash",
                "service.deny",
            ]
        );
    }

    #[test]
    fn an_entry_round_trips_through_one_line() {
        let line = entry().to_line().expect("line");
        assert!(!line.contains('\n'), "one entry is one line");
        assert_eq!(AuditEntry::parse_line(&line).expect("parse"), entry());
    }

    #[test]
    fn a_denial_without_an_app_is_allowed() {
        let mut entry = entry();
        entry.app = None;
        entry.actor = "system".to_string();
        entry.action = AuditAction::Deny;
        entry.result = Outcome::Denied;
        entry.detail = Some(serde_json::json!({"capability": "anth:store/kv"}));
        entry.validate().expect("a denial may have no app");
    }

    #[test]
    fn an_empty_actor_is_refused() {
        let mut entry = entry();
        entry.actor = "  ".to_string();
        assert!(entry.validate().is_err());
    }

    #[test]
    fn a_bad_timestamp_is_refused() {
        let mut entry = entry();
        entry.at = "yesterday".to_string();
        assert!(entry.validate().is_err());
    }

    #[test]
    fn a_bad_app_id_is_refused() {
        let mut entry = entry();
        entry.app = Some("notes".to_string());
        assert!(entry.validate().is_err());
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let line = entry().to_line().expect("line");
        let line = line.replace("\"result\"", "\"outcome\"");
        assert!(AuditEntry::parse_line(&line).is_err());
    }

    #[test]
    fn an_unknown_action_is_refused() {
        let line = entry().to_line().expect("line");
        let line = line.replace("app.approve", "app.reinstall");
        assert!(AuditEntry::parse_line(&line).is_err());
    }
}
