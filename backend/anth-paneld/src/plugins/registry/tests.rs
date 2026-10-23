//! Registry tests over real bundle directories.
//!
//! A bundle is a directory on disk, so these build one: the digest check, the
//! disabled-with-a-reason path and the anchor lookup are all properties of a
//! filesystem state, and a mock would let the very check that matters pass
//! without reading a file.

use super::*;

/// A directory that cleans itself up.
struct Bundle(PathBuf);

impl Bundle {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "anth-plugins-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("creating the bundle root");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The smallest guest that can be loaded, and its digest.
const GUEST: &str = r#"
(module
  (import "env" "anth_exec" (func $exec (param i32 i32 i32 i32) (result i64)))
  (import "env" "anth_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 1024) "\7b\22status\22:200,\22value\22:null\7d")
  (func (export "anth_abi_version") (result i32) (i32.const 2))
  (func (export "anth_alloc") (param i32) (result i32) (i32.const 2048))
  (func (export "anth_free") (param i32 i32))
  (func (export "anth_http") (param i32 i32) (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 23)))
)
"#;

fn wasm() -> Vec<u8> {
    wat::parse_str(GUEST).expect("the test guest must compile")
}

fn digest(wasm: &[u8]) -> String {
    hex(&Sha256::digest(wasm))
}

/// Writes one bundle named `id` under `root`, with the digest the manifest says.
fn write_bundle(root: &Path, id: &str, digest: &str, anchor: &str) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).expect("creating the plugin directory");
    std::fs::write(dir.join("plugin.wasm"), wasm()).expect("writing the module");
    std::fs::write(
        dir.join("plugin.toml"),
        format!(
            r#"
[plugin]
id = "{id}"
version = "0.2.0"
name = "Test"
abi = 2

[wasm]
path = "plugin.wasm"
sha256 = "{digest}"

[[api]]
method = "GET"
path = "{anchor}"
"#
        ),
    )
    .expect("writing the manifest");
    dir
}

#[test]
fn a_bundle_with_the_approved_digest_loads() {
    let root = Bundle::new("good");
    let module = wasm();
    write_bundle(root.path(), "org.test.good", &digest(&module), "/greeting");

    let registry = Registry::load(root.path());

    let entry = registry
        .get("org.test.good")
        .expect("the plugin must be there");
    assert!(entry.enabled.is_ok(), "got: {:?}", entry.enabled);
    assert_eq!(entry.name, "Test");
    assert_eq!(entry.version, "0.2.0");
    assert_eq!(
        entry.anchors,
        vec![("GET".to_string(), "/greeting".to_string())]
    );
    assert!(entry.loaded().is_some());
}

/// The check this whole module exists for: bytes that changed after approval
/// disable the plugin instead of running it.
#[test]
fn a_rewritten_module_disables_the_plugin_and_says_so() {
    let root = Bundle::new("rewritten");
    let approved = "0".repeat(63) + "1";
    write_bundle(root.path(), "org.test.rewritten", &approved, "/greeting");

    let registry = Registry::load(root.path());

    let entry = registry
        .get("org.test.rewritten")
        .expect("the plugin is listed");
    assert!(entry.loaded().is_none(), "a rewritten module must not load");
    let reason = entry.enabled.as_ref().expect_err("a reason is required");
    assert!(reason.contains("rewritten"), "got: {reason}");

    let error = entry.usable().expect_err("must refuse to run");
    assert_eq!(error.kind, ErrorKind::Forbidden);
    assert!(error.message.contains("disabled"), "got: {}", error.message);
}

#[test]
fn a_directory_without_a_manifest_is_listed_as_disabled() {
    let root = Bundle::new("empty");
    std::fs::create_dir_all(root.path().join("org.test.empty")).expect("creating");

    let registry = Registry::load(root.path());

    let entry = registry
        .get("org.test.empty")
        .expect("the plugin is listed");
    let reason = entry.enabled.as_ref().expect_err("a reason is required");
    assert!(reason.contains("plugin.toml"), "got: {reason}");
}

#[test]
fn an_anchor_resolves_to_its_plugin() {
    let root = Bundle::new("anchor");
    let module = wasm();
    write_bundle(root.path(), "org.test.anchor", &digest(&module), "/hello");
    let registry = Registry::load(root.path());

    let (entry, _loaded) = registry
        .anchor_owner("GET", "/hello")
        .expect("looking up")
        .expect("the anchor must resolve");

    assert_eq!(entry.id, "org.test.anchor");
    assert!(
        registry
            .anchor_owner("GET", "/nope")
            .expect("looking up")
            .is_none()
    );
}

#[test]
fn two_plugins_claiming_one_anchor_is_a_conflict() {
    let root = Bundle::new("conflict");
    let module = wasm();
    write_bundle(root.path(), "org.test.one", &digest(&module), "/same");
    write_bundle(root.path(), "org.test.two", &digest(&module), "/same");
    let registry = Registry::load(root.path());

    let error = registry
        .anchor_owner("GET", "/same")
        .expect_err("must refuse");

    assert_eq!(error.kind, ErrorKind::Conflict);
    assert!(error.message.contains("both"), "got: {}", error.message);
}

/// A missing plugin directory is not an error: a daemon started without plugins
/// must still serve everything else.
#[test]
fn a_missing_root_yields_an_empty_registry() {
    let registry = Registry::load(Path::new("/nonexistent/anth/plugins"));

    assert!(registry.is_empty());
    assert_eq!(registry.entries().count(), 0);
}
