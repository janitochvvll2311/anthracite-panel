//! Tests for the plugin modules at this level.
//!
//! `plugins/runtime/` is its own level and keeps its tests next to it.

/// What a bundle declares, and what the host refuses to accept.
mod manifest {
    use crate::plugins::manifest::*;
    use anth_wire::WireError;

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
    fn a_ui_slot_must_be_a_dotted_lowercase_name() {
        let text = GOOD.replace(
            r#"slot = "dashboard.widget""#,
            r#"slot = "Dashboard Widget""#,
        );

        assert!(refused(&text).message.contains("dotted lowercase"));
    }

    #[test]
    fn a_blank_ui_label_is_refused() {
        let text = GOOD.replace(
            r#"tag = "anth-hello""#,
            "tag = \"anth-hello\"\nlabel = \"   \"",
        );

        assert!(refused(&text).message.contains("cannot be blank"));
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

    /// The alias is the short handle a caller types, so it is kept as written.
    #[test]
    fn an_alias_is_parsed_next_to_the_id() {
        let text = GOOD.replace(
            r#"id = "org.anth.hello""#,
            "id = \"org.anth.hello\"\nalias = \"hello\"",
        );

        let manifest = Manifest::parse(&text).expect("parsing");
        assert_eq!(manifest.plugin.alias.as_deref(), Some("hello"));
    }

    /// Most bundles need no alias, and demanding one would be a pointless tax.
    #[test]
    fn an_alias_is_optional() {
        let manifest = Manifest::parse(GOOD).expect("parsing");

        assert!(manifest.plugin.alias.is_none());
    }

    /// An alias is a name a caller types, so anything that is not a plain lowercase
    /// word is a typo. Dots are excluded on purpose: a dotted value would be
    /// indistinguishable from an id at the point of use.
    #[test]
    fn a_malformed_alias_is_refused() {
        for bad in ["Hello", "org.anth.hello", "hello world", ""] {
            let text = GOOD.replace(
                r#"id = "org.anth.hello""#,
                &format!("id = \"org.anth.hello\"\nalias = \"{bad}\""),
            );

            let error = refused(&text);
            assert!(
                error.message.contains("alias"),
                "`{bad}` must be refused by name, got: {}",
                error.message
            );
        }
    }
}

/// The bundles the daemon found, and the digest that keeps them honest.
mod registry {
    use crate::plugins::registry::*;
    use anth_wire::ErrorKind;
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

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
        write_bundle_with_alias(root, id, None, digest, anchor)
    }

    /// The same, with the short handle the manifest may declare.
    fn write_bundle_with_alias(
        root: &Path,
        id: &str,
        alias: Option<&str>,
        digest: &str,
        anchor: &str,
    ) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).expect("creating the plugin directory");
        std::fs::write(dir.join("plugin.wasm"), wasm()).expect("writing the module");
        let alias_line = alias
            .map(|alias| format!("\nalias = \"{alias}\""))
            .unwrap_or_default();
        std::fs::write(
            dir.join("plugin.toml"),
            format!(
                r#"
[plugin]
id = "{id}"{alias_line}
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
            .resolve("org.test.good")
            .expect("looking up")
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
            .resolve("org.test.rewritten")
            .expect("looking up")
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
            .resolve("org.test.empty")
            .expect("looking up")
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

    /// The point of the alias: a caller types a short handle instead of the id.
    #[test]
    fn an_alias_resolves_to_its_plugin() {
        let root = Bundle::new("alias");
        let module = wasm();
        write_bundle_with_alias(
            root.path(),
            "org.test.aliased",
            Some("short"),
            &digest(&module),
            "/greeting",
        );
        let registry = Registry::load(root.path());

        let entry = registry
            .resolve("short")
            .expect("looking up")
            .expect("the alias must resolve");
        assert_eq!(entry.id, "org.test.aliased");

        // The id still works, and is what the entry reports either way.
        assert_eq!(
            registry.resolve("org.test.aliased").unwrap().unwrap().id,
            "org.test.aliased"
        );
        assert!(registry.resolve("nope").expect("looking up").is_none());
    }

    /// Two bundles claiming one alias is the same hazard as two claiming one anchor:
    /// answering by directory order would make the result depend on the filesystem.
    #[test]
    fn two_plugins_claiming_one_alias_is_a_conflict() {
        let root = Bundle::new("alias-conflict");
        let module = wasm();
        write_bundle_with_alias(
            root.path(),
            "org.test.a",
            Some("dup"),
            &digest(&module),
            "/a",
        );
        write_bundle_with_alias(
            root.path(),
            "org.test.b",
            Some("dup"),
            &digest(&module),
            "/b",
        );
        let registry = Registry::load(root.path());

        let error = registry.resolve("dup").expect_err("must refuse");

        assert_eq!(error.kind, ErrorKind::Conflict);
        assert!(error.message.contains("both"), "got: {}", error.message);
    }

    /// An id that happens to equal another plugin's alias still finds its own
    /// plugin: an exact id is never ambiguous, so it wins before any scan.
    #[test]
    fn an_exact_id_wins_over_an_alias() {
        let root = Bundle::new("alias-shadow");
        let module = wasm();
        write_bundle_with_alias(
            root.path(),
            "org.test.first",
            Some("second"),
            &digest(&module),
            "/a",
        );
        // The directory name is the id, so this bundle is literally named `second`.
        write_bundle_with_alias(root.path(), "second", None, &digest(&module), "/b");
        let registry = Registry::load(root.path());

        let entry = registry.resolve("second").expect("looking up").unwrap();

        assert_eq!(entry.id, "second");
    }
}
