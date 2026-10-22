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

#[test]
fn the_declared_anchor_is_the_path_the_router_answers() {
    let text = manifest();
    let path = text
        .lines()
        .skip_while(|line| !line.trim().starts_with("[[api]]"))
        .find_map(|line| line.trim().strip_prefix("path = "))
        .map(|rest| rest.trim().trim_matches('"').to_string())
        .expect("plugin.toml declares an [[api]] path");

    assert_eq!(
        path,
        anth_plugin_hello::logic::GREETING_PATH,
        "the manifest and the router must name the same anchor"
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
