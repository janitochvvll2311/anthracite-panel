//! The panel contract (§7.7).
//!
//! A panel declares the slots it implements; the composition surface supplies
//! every contribution. The engine does not know which slots exist, so this is
//! where a contribution is mounted or deliberately ignored.

use std::collections::BTreeSet;

use anth_contract::{AppId, SurfaceSlot};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

/// A path segment keeps `/` as a separator but encodes anything a URL would
/// otherwise reinterpret (`?`, `#`, `%`, spaces, controls).
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// The three portable slots in v0.5.0.
pub const VIEW: &str = "panel.view";
pub const ACTION: &str = "panel.action";
pub const SETTINGS: &str = "panel.settings";

/// One slot contribution, with the app that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    pub app: AppId,
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub order: i64,
    pub tag: String,
    pub module: String,
    pub sha256: String,
}

impl Contribution {
    pub fn from_surface(app: &AppId, slot: &SurfaceSlot) -> Self {
        Self {
            app: app.clone(),
            id: slot.id.clone(),
            label: slot.label.clone(),
            icon: slot.icon.clone(),
            order: slot.order,
            tag: slot.tag.clone(),
            module: slot.module.clone(),
            sha256: slot.sha256.clone(),
        }
    }

    /// Where the panel fetches the module. The engine serves only modules the
    /// manifest declares, and the hash is the cache key (§6.4).
    pub fn asset_url(&self) -> String {
        let module = self
            .module
            .split('/')
            .map(|segment| utf8_percent_encode(segment, SEGMENT).to_string())
            .collect::<Vec<_>>()
            .join("/");
        format!("/api/apps/{}/assets/{module}", self.app)
    }
}

/// What a panel implements, and every contribution it was offered.
#[derive(Debug, Clone, Default)]
pub struct Slots {
    implemented: BTreeSet<String>,
    contributions: Vec<Contribution>,
}

impl Slots {
    pub fn new(implemented: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            implemented: implemented.into_iter().map(Into::into).collect(),
            contributions: Vec::new(),
        }
    }

    pub fn implements(&self, slot: &str) -> bool {
        self.implemented.contains(slot)
    }

    /// Adds the slots a running app contributes.
    pub fn add(&mut self, app: &AppId, slots: &[SurfaceSlot]) {
        self.contributions.extend(
            slots
                .iter()
                .map(|slot| Contribution::from_surface(app, slot)),
        );
    }

    /// Drops every contribution, so a panel can rebuild after a composition
    /// refresh without stale entries.
    pub fn clear(&mut self) {
        self.contributions.clear();
    }

    /// Contributions for a slot this panel implements, in deterministic order:
    /// `order`, then app id, then tag (§7.7).
    pub fn mounted(&self, slot: &str) -> Vec<&Contribution> {
        if !self.implements(slot) {
            return Vec::new();
        }
        let mut found: Vec<&Contribution> = self
            .contributions
            .iter()
            .filter(|contribution| contribution.id == slot)
            .collect();
        found.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.app.cmp(&right.app))
                .then_with(|| left.tag.cmp(&right.tag))
        });
        found
    }

    /// Contributions this panel cannot mount, so development can report them.
    pub fn ignored(&self) -> Vec<&Contribution> {
        self.contributions
            .iter()
            .filter(|contribution| !self.implements(&contribution.id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> AppId {
        AppId::parse(text).expect("app")
    }

    fn slot(id: &str, tag: &str, order: i64) -> SurfaceSlot {
        SurfaceSlot {
            id: id.to_string(),
            label: tag.to_string(),
            icon: None,
            order,
            tag: tag.to_string(),
            module: format!("web/{tag}.js"),
            sha256: "a".repeat(64),
        }
    }

    #[test]
    fn a_panel_mounts_only_what_it_implements_in_order() {
        let mut slots = Slots::new([VIEW]);
        let notes = app("org.example.notes");
        slots.add(&notes, &[slot(VIEW, "beta", 2), slot(ACTION, "run", 1)]);
        slots.add(&notes, &[slot(VIEW, "alpha", 1)]);
        let mounted = slots.mounted(VIEW);
        assert_eq!(mounted.len(), 2);
        assert_eq!(mounted[0].tag, "alpha");
        assert_eq!(mounted[1].tag, "beta");
        assert_eq!(slots.mounted(ACTION).len(), 0);
        assert_eq!(slots.ignored().len(), 1);
        assert_eq!(slots.ignored()[0].id, ACTION);
    }

    #[test]
    fn an_asset_url_is_the_engine_route() {
        let contribution =
            Contribution::from_surface(&app("org.example.notes"), &slot(VIEW, "notes", 1));
        assert_eq!(
            contribution.asset_url(),
            "/api/apps/org.example.notes/assets/web/notes.js"
        );
        let mut spaced = slot(VIEW, "notes", 1);
        spaced.module = "web/a b.js".to_string();
        let contribution = Contribution::from_surface(&app("org.example.notes"), &spaced);
        assert_eq!(
            contribution.asset_url(),
            "/api/apps/org.example.notes/assets/web/a%20b.js"
        );
    }

    #[test]
    fn contributions_tie_break_by_app_id() {
        let mut slots = Slots::new([VIEW]);
        slots.add(&app("org.example.b"), &[slot(VIEW, "same", 1)]);
        slots.add(&app("org.example.a"), &[slot(VIEW, "same", 1)]);
        let mounted = slots.mounted(VIEW);
        assert_eq!(mounted[0].app.as_str(), "org.example.a");
        assert_eq!(mounted[1].app.as_str(), "org.example.b");
        slots.clear();
        assert!(slots.mounted(VIEW).is_empty());
    }

    #[test]
    fn two_panels_can_declare_different_slots() {
        let mut custom = Slots::new(["my.panel.extra"]);
        custom.add(
            &app("org.example.notes"),
            &[slot("my.panel.extra", "x", 0), slot(VIEW, "view", 0)],
        );
        assert_eq!(custom.mounted("my.panel.extra").len(), 1);
        assert_eq!(custom.mounted(VIEW).len(), 0);
        assert_eq!(custom.ignored().len(), 1);
    }
}
