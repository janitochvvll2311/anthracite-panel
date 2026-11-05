//! What a component actually declares: the imports it needs and the exports it
//! offers.
//!
//! The runtime extracts it from the component; the composer and the grant compare
//! it (§7.12). Living here keeps the rule testable with values, without a
//! component in sight.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::grant::Grant;
use crate::id::InterfaceId;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    #[serde(default)]
    pub imports: BTreeSet<InterfaceId>,
    #[serde(default)]
    pub exports: BTreeSet<InterfaceId>,
}

/// Every difference between what a component declares and what its grant froze.
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
}

impl WorldMismatch {
    pub fn is_empty(&self) -> bool {
        self.extra_imports.is_empty()
            && self.missing_imports.is_empty()
            && self.extra_exports.is_empty()
            && self.missing_exports.is_empty()
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
        Ok(())
    }
}

impl World {
    pub fn new(imports: BTreeSet<InterfaceId>, exports: BTreeSet<InterfaceId>) -> Self {
        Self { imports, exports }
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

    /// Compares the real world with the frozen grant in both directions.
    ///
    /// The interface id carries the package version, so a drifted version is a
    /// different id and shows up as a difference (§7.12, D-Interface-Versioning).
    /// Structural type drift stays with the runtime, which has the component
    /// types; this check is the name-and-version half.
    pub fn check(&self, granted: &Grant) -> Result<(), WorldMismatch> {
        let granted_imports: BTreeSet<InterfaceId> = granted.import.iter().cloned().collect();
        let granted_exports: BTreeSet<InterfaceId> = granted.export.iter().cloned().collect();
        let mismatch = WorldMismatch {
            extra_imports: self.imports.difference(&granted_imports).cloned().collect(),
            missing_imports: granted_imports.difference(&self.imports).cloned().collect(),
            extra_exports: self.exports.difference(&granted_exports).cloned().collect(),
            missing_exports: granted_exports.difference(&self.exports).cloned().collect(),
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
}
