//! The grant: the authority an operator approved for one app (§9.3).
//!
//! A grant freezes the app id, version, hashes, signature, interfaces and the
//! risk the operator accepted. It deliberately does **not** freeze the provider:
//! who provides an import is a composition fact, and changing the composition
//! must not invalidate every consumer's grant.
//!
//! The document is the guarantee, not the calculator: the engine computes the
//! risk over the resolved graph and writes the result here (§9.3, D-Risk-Recheck).

use serde::{Deserialize, Serialize};

use crate::catalog::Risk;
use crate::error::Error;
use crate::id::{is_sha256, parse_version, AppId, InterfaceId};

pub const GRANT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub version: u32,
    pub app: AppId,
    /// The version the grant was approved for. An upgrade needs a new grant.
    pub app_version: String,
    /// The bundle's manifest hash at approval.
    pub manifest_sha256: String,
    /// The WIT set hash at approval: changing exports is a new approval.
    pub wit_sha256: String,
    /// The detached signature, byte-for-byte as the bundle carried it. Its
    /// format belongs to the verifier; the grant only freezes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// The risk the operator approved, computed over the installed graph.
    pub risk: Risk,
    /// The levels the operator presented, which is what the audit records.
    #[serde(default)]
    pub accepted: Vec<Risk>,
    pub approved_by: String,
    /// RFC 3339, UTC. The audit log uses the same shape.
    pub approved_at: String,
    #[serde(default)]
    pub export: Vec<InterfaceId>,
    #[serde(default)]
    pub import: Vec<InterfaceId>,
}

impl Grant {
    #[cfg(feature = "parse")]
    pub fn parse(text: &str) -> Result<Self, Error> {
        toml::from_str(text)
            .map_err(|error| Error::invalid(format!("the grant is not valid TOML: {error}")))
    }

    #[cfg(feature = "parse")]
    pub fn to_toml(&self) -> Result<String, Error> {
        toml::to_string_pretty(self)
            .map_err(|error| Error::internal(format!("rendering the grant: {error}")))
    }

    /// The levels an approval of `risk` must present: everything above the read
    /// baseline up to and including the risk. `read` needs no flag.
    pub fn required_levels(risk: Risk) -> impl Iterator<Item = Risk> {
        Risk::ALL
            .iter()
            .copied()
            .filter(move |level| *level > Risk::Read && *level <= risk)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.version != GRANT_VERSION {
            return Err(Error::invalid(format!(
                "grant version {} is not supported; this engine speaks {GRANT_VERSION}",
                self.version
            )));
        }
        parse_version(&self.app_version)?;
        if !is_sha256(&self.manifest_sha256) {
            return Err(Error::invalid("`manifest_sha256` is not 64 lowercase hex"));
        }
        if !is_sha256(&self.wit_sha256) {
            return Err(Error::invalid("`wit_sha256` is not 64 lowercase hex"));
        }
        if self.approved_by.trim().is_empty() {
            return Err(Error::invalid("`approved_by` cannot be empty"));
        }
        humantime::parse_rfc3339(&self.approved_at).map_err(|error| {
            Error::invalid(format!(
                "`approved_at` is not an RFC 3339 timestamp: {error}"
            ))
        })?;

        for level in Self::required_levels(self.risk) {
            if !self.accepted.contains(&level) {
                return Err(Error::invalid(format!(
                    "the grant for `{}` does not accept `{level}`; approve with {}",
                    self.app,
                    level.approval_flag().unwrap_or("")
                )));
            }
        }

        check_unique(&self.export, "exported")?;
        check_unique(&self.import, "imported")?;
        Ok(())
    }
}

fn check_unique(interfaces: &[InterfaceId], what: &str) -> Result<(), Error> {
    let mut seen: Vec<InterfaceId> = Vec::with_capacity(interfaces.len());
    for interface in interfaces {
        if seen.contains(interface) {
            return Err(Error::invalid(format!(
                "`{interface}` is {what} twice in the grant"
            )));
        }
        seen.push(interface.clone());
    }
    Ok(())
}

#[cfg(all(test, feature = "parse"))]
mod tests {
    use super::*;

    fn grant() -> Grant {
        Grant {
            version: GRANT_VERSION,
            app: AppId::parse("org.example.notes").expect("id"),
            app_version: "0.1.0".to_string(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: Some("c2lnbmF0dXJl".to_string()),
            risk: Risk::Write,
            accepted: vec![Risk::Write],
            approved_by: "operator-3f2a".to_string(),
            approved_at: "2026-11-03T20:00:00Z".to_string(),
            export: vec![InterfaceId::parse("example:notes/list@1.0.0").expect("interface")],
            import: vec![InterfaceId::parse("anth:store/kv@1.0.0").expect("interface")],
        }
    }

    #[test]
    fn a_write_grant_with_its_flag_validates() {
        grant().validate().expect("valid");
    }

    #[test]
    fn read_needs_no_flag() {
        let mut grant = grant();
        grant.risk = Risk::Read;
        grant.accepted.clear();
        grant.validate().expect("read is the baseline");
    }

    #[test]
    fn a_write_grant_without_the_flag_is_refused() {
        let mut grant = grant();
        grant.accepted.clear();
        let error = grant.validate().expect_err("missing flag");
        assert!(error.message().contains("--accept-write"), "{error}");
    }

    #[test]
    fn an_admin_grant_needs_every_level_up_to_admin() {
        let mut grant = grant();
        grant.risk = Risk::Admin;
        grant.accepted = vec![Risk::Write, Risk::Admin];
        grant.validate().expect("both levels");

        grant.accepted = vec![Risk::Admin];
        let error = grant.validate().expect_err("write missing");
        assert!(error.message().contains("--accept-write"), "{error}");
    }

    #[test]
    fn extra_accepted_levels_are_allowed() {
        let mut grant = grant();
        grant.accepted = vec![Risk::Write, Risk::Admin];
        grant
            .validate()
            .expect("over-accepting does not change the risk");
    }

    #[test]
    fn a_bad_version_is_refused() {
        let mut grant = grant();
        grant.version = 2;
        assert!(grant.validate().is_err());
    }

    #[test]
    fn a_bad_hash_is_refused() {
        let mut grant = grant();
        grant.manifest_sha256 = "XYZ".to_string();
        assert!(grant.validate().is_err());
    }

    #[test]
    fn a_bad_timestamp_is_refused() {
        let mut grant = grant();
        grant.approved_at = "yesterday".to_string();
        assert!(grant.validate().is_err());
    }

    #[test]
    fn an_empty_approver_is_refused() {
        let mut grant = grant();
        grant.approved_by = "  ".to_string();
        assert!(grant.validate().is_err());
    }

    #[test]
    fn an_interface_cannot_be_frozen_twice() {
        let mut grant = grant();
        grant.export.push(grant.export[0].clone());
        let error = grant.validate().expect_err("duplicate");
        assert!(error.message().contains("twice"), "{error}");
    }

    #[test]
    fn the_document_round_trips_through_toml() {
        let grant = grant();
        let text = grant.to_toml().expect("render");
        assert_eq!(Grant::parse(&text).expect("parse"), grant);
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let text = grant().to_toml().expect("render");
        let text = text.replace("risk = ", "riisk = ");
        assert!(Grant::parse(&text).is_err());
    }
}
