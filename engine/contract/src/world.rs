//! What a component actually declares: the imports it needs and the exports it
//! offers.
//!
//! The runtime extracts it from the component; the composer and the grant compare
//! it (§7.12). Living here keeps the rule testable with values, without a
//! component in sight.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::InterfaceId;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct World {
    #[serde(default)]
    pub imports: BTreeSet<InterfaceId>,
    #[serde(default)]
    pub exports: BTreeSet<InterfaceId>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> InterfaceId {
        InterfaceId::parse(text).expect("id")
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
}
