use super::*;

const GOOD: &str = r#"
[plugin]
id = "org.anth.hello"
version = "0.2.0"
name = "Hello"
abi = 2

[wasm]
path = "plugin.wasm"
sha256 = "0337900b20e3240510dfedb1152912063ce5f679e162324124832d3325d201b0"

[[capability]]
id = "host.hostname"
program = "hostname"

[[api]]
method = "GET"
path = "/greeting"

[[ui]]
slot = "dashboard.widget"
tag = "anth-hello"
module = "web/hello.js"
"#;

fn refused(text: &str) -> WireError {
    Manifest::parse(text).expect_err("must be refused")
}

#[test]
fn the_example_manifest_parses() {
    let manifest = Manifest::parse(GOOD).expect("the shipped manifest must parse");

    assert_eq!(manifest.plugin.id, "org.anth.hello");
    assert_eq!(manifest.api.len(), 1);
    assert_eq!(
        manifest.capability("host.hostname").unwrap().program,
        "hostname"
    );
    assert!(manifest.capability("host.other").is_none());
}

#[test]
fn another_abi_is_refused_by_number() {
    let error = refused(&GOOD.replace("abi = 2", "abi = 1"));

    assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
    assert!(error.message.contains("ABI 1"), "got: {}", error.message);
}

/// The module path is the one thing a rewritten manifest must not be able to
/// move, so naming another file is a refusal rather than a redirect.
#[test]
fn a_module_path_other_than_plugin_wasm_is_refused() {
    let text = GOOD.replace(r#"path = "plugin.wasm""#, r#"path = "other.wasm""#);

    assert!(refused(&text).message.contains("pinned"));
}

#[test]
fn an_all_zero_digest_is_refused_with_the_remedy() {
    let text = GOOD.replace(
        "0337900b20e3240510dfedb1152912063ce5f679e162324124832d3325d201b0",
        &"0".repeat(64),
    );

    assert!(refused(&text).message.contains("package.sh"));
}

#[test]
fn a_short_digest_is_refused() {
    let text = GOOD.replace(
        "0337900b20e3240510dfedb1152912063ce5f679e162324124832d3325d201b0",
        "abc123",
    );

    assert!(refused(&text).message.contains("64-character"));
}

/// The allowlist is the authority: a manifest cannot introduce a program the
/// host does not know, which is what stops a bundle from naming `/bin/sh`.
#[test]
fn a_program_outside_the_allowlist_is_refused() {
    let text = GOOD.replace(r#"program = "hostname""#, r#"program = "sh""#);
    let error = refused(&text);

    assert!(
        error.message.contains("allowlist"),
        "got: {}",
        error.message
    );
    assert!(error.message.contains("sh"));
}

#[test]
fn a_duplicate_anchor_is_refused() {
    let text = GOOD.replace(
        "[[ui]]",
        "[[api]]\nmethod = \"GET\"\npath = \"/greeting\"\n\n[[ui]]",
    );

    assert!(refused(&text).message.contains("declared twice"));
}

#[test]
fn an_anchor_without_a_leading_slash_is_refused() {
    let text = GOOD.replace(r#"path = "/greeting""#, r#"path = "greeting""#);

    assert!(refused(&text).message.contains("start with a slash"));
}

#[test]
fn an_unknown_method_is_refused() {
    let text = GOOD.replace(r#"method = "GET""#, r#"method = "TRACE""#);

    assert!(refused(&text).message.contains("TRACE"));
}

/// An id becomes a path under the plugin directory, so anything that could
/// climb out of it is refused at the door.
#[test]
fn an_id_that_could_escape_the_plugin_directory_is_refused() {
    for bad in ["../evil", "Org.Anth", "nodots", "org..thing", "org.an th"] {
        let text = GOOD.replace("org.anth.hello", bad);
        let error = refused(&text);
        assert!(
            error.message.contains("not a plugin id"),
            "`{bad}` should be refused, got: {}",
            error.message
        );
    }
}

#[test]
fn a_ui_module_cannot_climb_out_of_the_bundle() {
    let text = GOOD.replace("web/hello.js", "../../etc/passwd");

    assert!(refused(&text).message.contains("inside the bundle"));
}

#[test]
fn a_ui_tag_must_be_a_custom_element_name() {
    let text = GOOD.replace(r#"tag = "anth-hello""#, r#"tag = "Hello""#);

    assert!(refused(&text).message.contains("custom-element"));
}

#[test]
fn a_duplicate_capability_is_refused() {
    let text = GOOD.replace(
        "[[api]]",
        "[[capability]]\nid = \"host.hostname\"\nprogram = \"hostname\"\n\n[[api]]",
    );

    assert!(refused(&text).message.contains("declared twice"));
}

/// The safe default: a capability that declares no arguments takes none, which
/// is what stops a guest from adding a flag to a frozen argv.
#[test]
fn an_empty_capability_accepts_nothing() {
    let manifest = Manifest::parse(GOOD).expect("parsing");

    assert!(
        manifest
            .capability("host.hostname")
            .unwrap()
            .accept
            .is_empty()
    );
}

#[test]
fn a_manifest_without_a_plugin_section_is_refused() {
    assert!(Manifest::parse("[wasm]\npath = \"plugin.wasm\"\n").is_err());
}
