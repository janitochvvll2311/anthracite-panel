//! What a component actually declares: the imports it needs and the exports it
//! offers.
//!
//! The runtime extracts it from the component; the composer and the grant compare
//! it (§7.12). Living here keeps the rule testable with values, without a
//! component in sight.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::grant::Grant;
use crate::id::InterfaceId;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    #[serde(default)]
    pub imports: BTreeSet<InterfaceId>,
    #[serde(default)]
    pub exports: BTreeSet<InterfaceId>,
    /// A canonical signature per interface, when the layer that read the types
    /// knows it. The form is lowercase hex SHA-256 of the interface's canonical
    /// WIT type text (D-World-Signature); empty means the comparison is names and
    /// versions only, which is all a grant can express.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub signatures: BTreeMap<InterfaceId, String>,
}

/// Every difference between what a component declares and what the granted world
/// expects.
///
/// A non-empty result is a refusal before instantiation (§7.12): an extra import
/// is authority the operator did not approve, and a missing export is an app that
/// cannot answer what its manifest promised.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldMismatch {
    #[serde(default)]
    pub extra_imports: Vec<InterfaceId>,
    #[serde(default)]
    pub missing_imports: Vec<InterfaceId>,
    #[serde(default)]
    pub extra_exports: Vec<InterfaceId>,
    #[serde(default)]
    pub missing_exports: Vec<InterfaceId>,
    /// Interfaces whose structural signature differs from the expected one.
    #[serde(default)]
    pub type_drift: Vec<InterfaceId>,
}

impl WorldMismatch {
    pub fn is_empty(&self) -> bool {
        self.extra_imports.is_empty()
            && self.missing_imports.is_empty()
            && self.extra_exports.is_empty()
            && self.missing_exports.is_empty()
            && self.type_drift.is_empty()
    }
}

impl fmt::Display for WorldMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        let mut part = |label: &str, values: &[InterfaceId], f: &mut fmt::Formatter<'_>| {
            if values.is_empty() {
                return Ok(());
            }
            if !first {
                write!(f, "; ")?;
            }
            first = false;
            write!(
                f,
                "{label} {}",
                values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        part("extra imports", &self.extra_imports, f)?;
        part("missing imports", &self.missing_imports, f)?;
        part("extra exports", &self.extra_exports, f)?;
        part("missing exports", &self.missing_exports, f)?;
        part("type drift", &self.type_drift, f)?;
        Ok(())
    }
}

impl From<WorldMismatch> for Error {
    fn from(mismatch: WorldMismatch) -> Self {
        let detail = serde_json::to_value(&mismatch).unwrap_or(serde_json::Value::Null);
        Error::denied(format!(
            "the component world does not match its grant: {mismatch}"
        ))
        .with_detail(detail)
    }
}

/// `anth:error` is a package of types, injected when the WIT is parsed: it is
/// never granted and never risked (§9.2), so it is not a difference.
fn is_injected(id: &InterfaceId) -> bool {
    id.namespace() == "anth" && id.package() == "error"
}

impl World {
    pub fn new(imports: BTreeSet<InterfaceId>, exports: BTreeSet<InterfaceId>) -> Self {
        Self {
            imports,
            exports,
            signatures: BTreeMap::new(),
        }
    }

    /// Attaches the structural signature the runtime or WIT parser computed.
    pub fn with_signature(mut self, id: InterfaceId, signature: impl Into<String>) -> Self {
        self.signatures.insert(id, signature.into());
        self
    }

    /// The imports the grant does not list, which is what a lying manifest looks
    /// like once the component is read.
    pub fn undeclared_imports<'a>(
        &'a self,
        allowed: &'a BTreeSet<InterfaceId>,
    ) -> impl Iterator<Item = &'a InterfaceId> {
        self.imports.iter().filter(|id| !allowed.contains(*id))
    }

    /// The exports the grant promised and the component does not implement.
    pub fn missing_exports<'a>(
        &'a self,
        promised: &'a BTreeSet<InterfaceId>,
    ) -> impl Iterator<Item = &'a InterfaceId> {
        promised.iter().filter(|id| !self.exports.contains(*id))
    }

    /// Compares the real world with the grant.
    ///
    /// The grant names interfaces, not their structure, so this is the
    /// name-and-version half. The runtime uses [`World::check_expected`] with the
    /// world built from the installed WIT, which carries signatures.
    pub fn check(&self, granted: &Grant) -> Result<(), WorldMismatch> {
        let expected = World::new(
            granted.import.iter().cloned().collect(),
            granted.export.iter().cloned().collect(),
        );
        self.check_expected(&expected)
    }

    /// Compares the real world with the world the installed WIT describes.
    ///
    /// The interface id carries the package version, so a drifted version is a
    /// different id and shows up as a difference (D-Interface-Versioning). If the
    /// expected world has a signature for an interface, the real one must carry
    /// the same signature; a missing or different one is structural drift
    /// (§7.12).
    pub fn check_expected(&self, expected: &World) -> Result<(), WorldMismatch> {
        let actual_imports: BTreeSet<InterfaceId> = self
            .imports
            .iter()
            .filter(|id| !is_injected(id))
            .cloned()
            .collect();
        let actual_exports: BTreeSet<InterfaceId> = self.exports.iter().cloned().collect();
        let expected_imports: BTreeSet<InterfaceId> = expected
            .imports
            .iter()
            .filter(|id| !is_injected(id))
            .cloned()
            .collect();
        let expected_exports: BTreeSet<InterfaceId> = expected.exports.iter().cloned().collect();
        let type_drift: BTreeSet<InterfaceId> = actual_imports
            .iter()
            .chain(actual_exports.iter())
            .filter(|id| match expected.signatures.get(*id) {
                Some(wanted) => self.signatures.get(*id) != Some(wanted),
                None => false,
            })
            .cloned()
            .collect();
        let mismatch = WorldMismatch {
            extra_imports: actual_imports
                .difference(&expected_imports)
                .cloned()
                .collect(),
            missing_imports: expected_imports
                .difference(&actual_imports)
                .cloned()
                .collect(),
            extra_exports: actual_exports
                .difference(&expected_exports)
                .cloned()
                .collect(),
            missing_exports: expected_exports
                .difference(&actual_exports)
                .cloned()
                .collect(),
            type_drift: type_drift.into_iter().collect(),
        };
        if mismatch.is_empty() {
            Ok(())
        } else {
            Err(mismatch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Risk;
    use crate::grant::GRANT_VERSION;
    use crate::id::AppId;

    fn id(text: &str) -> InterfaceId {
        InterfaceId::parse(text).expect("id")
    }

    fn grant(import: &[InterfaceId], export: &[InterfaceId]) -> Grant {
        Grant {
            version: GRANT_VERSION,
            app: AppId::parse("org.example.notes").expect("app"),
            app_version: "0.1.0".to_string(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: None,
            risk: Risk::Read,
            accepted: Vec::new(),
            approved_by: "operator-test".to_string(),
            approved_at: "2026-11-05T20:00:00Z".to_string(),
            export: export.to_vec(),
            import: import.to_vec(),
        }
    }

    #[test]
    fn an_extra_import_is_visible() {
        let world = World::new([id("anth:env/info@1.0.0")].into(), BTreeSet::new());
        let allowed = [id("anth:env/info@1.0.0")].into();
        assert_eq!(world.undeclared_imports(&allowed).count(), 0);

        let allowed = BTreeSet::new();
        assert_eq!(world.undeclared_imports(&allowed).count(), 1);
    }

    #[test]
    fn a_missing_export_is_visible() {
        let world = World::new(BTreeSet::new(), [id("example:notes/list@1.0.0")].into());
        let promised = [
            id("example:notes/list@1.0.0"),
            id("example:notes/write@1.0.0"),
        ]
        .into();
        let missing: Vec<_> = world.missing_exports(&promised).collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].to_string(), "example:notes/write@1.0.0");
    }

    #[test]
    fn a_world_that_matches_its_grant_passes() {
        let import = id("anth:env/info@1.0.0");
        let export = id("example:notes/list@1.0.0");
        let world = World::new([import.clone()].into(), [export.clone()].into());
        world.check(&grant(&[import], &[export])).expect("match");
    }

    #[test]
    fn an_extra_import_is_refused() {
        let granted = id("anth:env/info@1.0.0");
        let sneaked = id("anth:env/random@1.0.0");
        let world = World::new(
            [granted.clone(), sneaked.clone()].into(),
            [id("example:notes/list@1.0.0")].into(),
        );
        let error = world
            .check(&grant(&[granted], &[id("example:notes/list@1.0.0")]))
            .expect_err("extra import");
        assert_eq!(error.extra_imports, [sneaked]);
        assert!(error.to_string().contains("extra imports"), "{error}");
    }

    #[test]
    fn a_missing_import_is_refused() {
        let granted = id("anth:env/info@1.0.0");
        let world = World::new(BTreeSet::new(), [id("example:notes/list@1.0.0")].into());
        let error = world
            .check(&grant(
                std::slice::from_ref(&granted),
                &[id("example:notes/list@1.0.0")],
            ))
            .expect_err("missing import");
        assert_eq!(error.missing_imports, [granted]);
    }

    #[test]
    fn an_extra_export_is_refused() {
        let granted = id("example:notes/list@1.0.0");
        let sneaked = id("example:notes/secrets@1.0.0");
        let world = World::new(BTreeSet::new(), [granted.clone(), sneaked.clone()].into());
        let error = world
            .check(&grant(&[], &[granted]))
            .expect_err("extra export");
        assert_eq!(error.extra_exports, [sneaked]);
    }

    #[test]
    fn a_drifted_version_is_a_different_interface() {
        let granted = id("example:notes/list@1.0.0");
        let drifted = id("example:notes/list@1.1.0");
        let world = World::new(BTreeSet::new(), [drifted.clone()].into());
        let error = world
            .check(&grant(&[], std::slice::from_ref(&granted)))
            .expect_err("drift");
        assert_eq!(error.extra_exports, [drifted]);
        assert_eq!(error.missing_exports, [granted]);
    }

    #[test]
    fn the_injected_error_package_is_never_a_difference() {
        let injected = id("anth:error/kinds@1.0.0");
        let export = id("example:notes/list@1.0.0");
        let world = World::new([injected].into(), [export.clone()].into());
        world
            .check(&grant(&[], std::slice::from_ref(&export)))
            .expect("anth:error is injected, not granted");
    }

    #[test]
    fn a_structural_drift_is_detected_when_both_sides_carry_a_signature() {
        let export = id("example:notes/list@1.0.0");
        let world = World::new(BTreeSet::new(), [export.clone()].into())
            .with_signature(export.clone(), "a".repeat(64));
        let expected = World::new(BTreeSet::new(), [export.clone()].into())
            .with_signature(export.clone(), "b".repeat(64));
        let error = world.check_expected(&expected).expect_err("type drift");
        assert_eq!(error.type_drift, [export]);
        assert!(error.to_string().contains("type drift"), "{error}");
    }

    #[test]
    fn a_missing_signature_is_drift_when_the_expected_world_has_one() {
        let export = id("example:notes/list@1.0.0");
        let world = World::new(BTreeSet::new(), [export.clone()].into());
        let expected = World::new(BTreeSet::new(), [export.clone()].into())
            .with_signature(export.clone(), "b".repeat(64));
        let error = world.check_expected(&expected).expect_err("unknown type");
        assert_eq!(error.type_drift, [export]);
    }

    #[test]
    fn an_anth_error_export_is_still_a_real_export() {
        let sneaked = id("anth:error/kinds@1.0.0");
        let world = World::new(BTreeSet::new(), [sneaked.clone()].into());
        let error = world.check(&grant(&[], &[])).expect_err("extra export");
        assert_eq!(error.extra_exports, [sneaked]);
    }

    #[test]
    fn matching_signatures_pass() {
        let export = id("example:notes/list@1.0.0");
        let signature = "a".repeat(64);
        let world = World::new(BTreeSet::new(), [export.clone()].into())
            .with_signature(export.clone(), signature.clone());
        let expected =
            World::new(BTreeSet::new(), [export.clone()].into()).with_signature(export, signature);
        world.check_expected(&expected).expect("match");
    }

    #[test]
    fn a_world_mismatch_becomes_a_denied_error_with_structured_detail() {
        let sneaked = id("anth:env/random@1.0.0");
        let world = World::new([sneaked.clone()].into(), BTreeSet::new());
        let mismatch = world.check(&grant(&[], &[])).expect_err("extra import");
        let error: Error = mismatch.into();
        assert_eq!(error.kind(), crate::error::ErrorKind::Denied);
        assert!(error.message().contains("extra imports"), "{error}");
        let detail = error.envelope().error.detail.expect("detail");
        assert_eq!(detail["extra_imports"][0], sneaked.to_string());
    }
}
