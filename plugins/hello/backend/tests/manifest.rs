//! The manifest and the code must agree.
//!
//! v0.1.0 shipped a manifest that declared a UI tag no module registered, and
//! nothing failed: the host only noticed at mount time. These are the fields the
//! two halves share, checked against the real files rather than a copy.

use std::fs;
use std::path::PathBuf;

fn manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugin.toml");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
}

fn value(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(&format!("{key} = ")))
        .map(|rest| rest.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| panic!("plugin.toml has no `{key}`"))
}

#[test]
fn the_declared_abi_is_the_one_the_crate_implements() {
    let declared = value(&manifest(), "abi");
    assert_eq!(
        declared,
        anth_plugin::ABI_VERSION.to_string(),
        "plugin.toml declares ABI {declared}, anth-plugin implements {}",
        anth_plugin::ABI_VERSION
    );
}

/// Every `path = "..."` inside an `[[api]]` block, in order.
fn declared_anchors(text: &str) -> Vec<String> {
    let mut in_api = false;
    let mut paths = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_api = line == "[[api]]";
            continue;
        }
        if in_api && let Some(rest) = line.strip_prefix("path = ") {
            paths.push(rest.trim().trim_matches('"').to_string());
        }
    }
    paths
}

/// The manifest and the router must name the same anchors, both ways.
///
/// One-way would let a route exist in code and be unreachable through the host,
/// or be declared and 404 — the v0.1.0 defect. A route the CLI cannot reach is a
/// route that only half exists.
#[test]
fn the_declared_anchors_are_the_routes_the_router_answers() {
    let declared = declared_anchors(&manifest());
    let implemented: Vec<String> = anth_plugin_hello::logic::ROUTES
        .iter()
        .map(|(_, path)| path.to_string())
        .collect();

    assert_eq!(
        declared, implemented,
        "plugin.toml and the router disagree about the anchors"
    );
    assert!(
        declared.len() > 1,
        "one anchor cannot show that the CLI reaches more than a default path"
    );
}

#[test]
fn the_declared_capability_is_the_one_the_logic_asks_for() {
    let text = manifest();
    let capability = text
        .lines()
        .skip_while(|line| !line.trim().starts_with("[[capability]]"))
        .find_map(|line| line.trim().strip_prefix("id = "))
        .map(|rest| rest.trim().trim_matches('"').to_string())
        .expect("plugin.toml declares a [[capability]] id");

    // Asking for a capability the manifest does not grant is refused by the host
    // at call time; catching it here names the mismatch instead.
    assert_eq!(capability, anth_plugin_hello::logic::hostname_capability());
}
