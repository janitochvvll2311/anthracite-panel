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
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 27)))
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

    /// A bundle with a web module on disk, so the allowlist has a real file to
    /// name. The module is written where the manifest says it is.
    fn write_ui_bundle(root: &Path, id: &str, module: &str) -> PathBuf {
        let dir = write_bundle(root, id, &digest(&wasm()), "/greeting");
        let file = dir.join(module);
        std::fs::create_dir_all(file.parent().expect("a module has a parent"))
            .expect("creating the web directory");
        std::fs::write(&file, "export const hello = 1;").expect("writing the module");
        let manifest = std::fs::read_to_string(dir.join("plugin.toml")).expect("reading");
        std::fs::write(
            dir.join("plugin.toml"),
            format!(
                "{manifest}\n[[ui]]\nslot = \"dashboard.widget\"\ntag = \"anth-test\"\nmodule = \"{module}\"\n"
            ),
        )
        .expect("writing the manifest");
        dir
    }

    #[test]
    fn a_declared_ui_module_is_reachable() {
        let root = Bundle::new("ui-declared");
        write_ui_bundle(root.path(), "org.test.ui", "web/widget.js");
        let registry = Registry::load(root.path());

        let entry = registry
            .ui_module("org.test.ui", "web/widget.js")
            .expect("looking up")
            .expect("a declared module must resolve");

        assert_eq!(entry.id, "org.test.ui");
        assert_eq!(entry.ui.len(), 1);
        assert_eq!(entry.ui[0].module, "web/widget.js");
        assert_eq!(entry.ui[0].slot, "dashboard.widget");
    }

    /// The reason the module is looked up by declaration instead of by path:
    /// a bundle's own wasm and manifest sit beside its web assets, and neither
    /// is something a request may read.
    #[test]
    fn an_undeclared_file_in_a_bundle_is_not_reachable() {
        let root = Bundle::new("ui-undeclared");
        write_ui_bundle(root.path(), "org.test.ui", "web/widget.js");
        let registry = Registry::load(root.path());

        for path in ["plugin.wasm", "plugin.toml", "web/other.js"] {
            assert!(
                registry
                    .ui_module("org.test.ui", path)
                    .expect("looking up")
                    .is_none(),
                "`{path}` must not be reachable"
            );
        }
    }

    /// A traversal never reaches the allowlist at all: `normalise` refuses `..`
    /// rather than resolving it, so these paths are `None` before any
    /// comparison. That is a different guarantee from the allowlist, and this
    /// test pins the one it names — emptying the allowlist does not change the
    /// answer here, and rewriting `normalise` to resolve `..` would.
    #[test]
    fn a_traversal_is_refused_before_the_allowlist_is_consulted() {
        let root = Bundle::new("ui-traversal");
        write_ui_bundle(root.path(), "org.test.ui", "web/widget.js");
        let registry = Registry::load(root.path());

        for path in [
            "web/../plugin.wasm",
            "../plugin.toml",
            "web/../../org.test.ui/plugin.wasm",
            "/etc/passwd",
            "web/.env",
        ] {
            assert!(
                registry
                    .ui_module("org.test.ui", path)
                    .expect("looking up")
                    .is_none(),
                "`{path}` must not be reachable"
            );
        }
    }

    /// Normalisation is applied to the request, not used as a filter: a
    /// redundant but harmless spelling still names the declared module. This
    /// is the case that fails if `normalise` starts rejecting anything it does
    /// not spell identically, which would make a correct URL stop working.
    #[test]
    fn a_redundant_spelling_of_a_declared_module_still_resolves() {
        let root = Bundle::new("ui-spelling");
        write_ui_bundle(root.path(), "org.test.ui", "web/widget.js");
        let registry = Registry::load(root.path());

        for path in ["./web/widget.js", "web//widget.js", "web/./widget.js"] {
            assert!(
                registry
                    .ui_module("org.test.ui", path)
                    .expect("looking up")
                    .is_some(),
                "`{path}` names a declared module"
            );
        }
    }

    /// A declared module whose file is a symlink out of the bundle is refused
    /// by the read, even though the manifest names it.
    #[cfg(unix)]
    #[test]
    fn a_declared_module_that_links_out_of_the_bundle_is_not_read() {
        let root = Bundle::new("ui-symlink");
        let outside = root.path().join("outside.js");
        std::fs::write(&outside, "secret").expect("writing the target");
        let dir = write_ui_bundle(root.path(), "org.test.ui", "web/widget.js");
        std::fs::remove_file(dir.join("web/widget.js")).expect("removing the module");
        std::os::unix::fs::symlink(&outside, dir.join("web/widget.js")).expect("linking");

        let registry = Registry::load(root.path());
        let entry = registry
            .ui_module("org.test.ui", "web/widget.js")
            .expect("looking up")
            .expect("the manifest still declares it");

        assert!(
            read_ui_module(entry, "web/widget.js").is_none(),
            "a link out of the bundle must not be read"
        );
    }

    #[test]
    fn a_declared_module_the_bundle_lacks_is_a_missing_read() {
        let root = Bundle::new("ui-absent");
        let dir = write_ui_bundle(root.path(), "org.test.ui", "web/widget.js");
        std::fs::remove_file(dir.join("web/widget.js")).expect("removing the module");
        let registry = Registry::load(root.path());
        let entry = registry
            .ui_module("org.test.ui", "web/widget.js")
            .expect("looking up")
            .expect("declared");

        assert!(read_ui_module(entry, "web/widget.js").is_none());
    }

    #[test]
    fn a_real_module_is_read_back() {
        let root = Bundle::new("ui-read");
        write_ui_bundle(root.path(), "org.test.ui", "web/widget.js");
        let registry = Registry::load(root.path());
        let entry = registry
            .ui_module("org.test.ui", "web/widget.js")
            .expect("looking up")
            .expect("declared");

        assert_eq!(
            read_ui_module(entry, "web/widget.js").expect("the bytes"),
            b"export const hello = 1;"
        );
    }

    #[test]
    fn an_alias_finds_the_ui_module_too() {
        let root = Bundle::new("ui-alias");
        let dir = write_bundle_with_alias(
            root.path(),
            "org.test.aliased",
            Some("short"),
            &digest(&wasm()),
            "/greeting",
        );
        std::fs::create_dir_all(dir.join("web")).expect("creating");
        std::fs::write(dir.join("web/w.js"), "x").expect("writing");
        let manifest = std::fs::read_to_string(dir.join("plugin.toml")).expect("reading");
        std::fs::write(
            dir.join("plugin.toml"),
            format!("{manifest}\n[[ui]]\nslot = \"dashboard.widget\"\ntag = \"anth-test\"\nmodule = \"web/w.js\"\n"),
        )
        .expect("writing");

        let registry = Registry::load(root.path());

        assert!(registry.ui_module("short", "web/w.js").unwrap().is_some());
    }
}

/// Installing a bundle: a real zip written to a real directory.
///
/// The failures worth testing are all about *what ends up on disk*, so the
/// archive is built entry by entry rather than mocked: a refusal that still left
/// a file behind would pass a test that only inspected the return value.
mod install {
    use crate::plugins::install::{MAX_ENTRIES, install};
    use anth_wire::ErrorKind;
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::path::{Path, PathBuf};

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
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const 27)))
)
"#;

    fn wasm() -> Vec<u8> {
        wat::parse_str(GUEST).expect("the test guest must compile")
    }

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn manifest(id: &str, sha256: &str) -> String {
        format!(
            r#"[plugin]
id = "{id}"
version = "0.1.0"
name = "Test"
abi = 2

[wasm]
path = "plugin.wasm"
sha256 = "{sha256}"

[[api]]
method = "GET"
path = "/greeting"

[[ui]]
slot = "dashboard.widget"
tag = "anth-test"
module = "web/widget.js"
"#
        )
    }

    /// A scratch directory that cleans itself up.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!(
                "anth-install-{label}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("creating the scratch root");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Writes a zip holding `entries`, in order, and returns its path.
    fn zip_at(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).expect("creating the zip");
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            writer
                .start_file(*name, options)
                .expect("starting an entry");
            writer.write_all(bytes).expect("writing an entry");
        }
        writer.finish().expect("finishing the zip");
    }

    /// A zip holding a bundle that should install cleanly.
    fn good_zip(path: &Path, id: &str) {
        let wasm = wasm();
        let sha = digest(&wasm);
        let manifest = manifest(id, &sha);
        zip_at(
            path,
            &[
                ("plugin.toml", manifest.as_bytes()),
                ("plugin.wasm", &wasm),
                ("web/widget.js", b"export const x = 1;"),
            ],
        );
    }

    fn error(result: Result<impl std::fmt::Debug, anth_wire::WireError>) -> anth_wire::WireError {
        match result {
            Ok(value) => panic!("expected a refusal, got {value:?}"),
            Err(error) => error,
        }
    }

    #[test]
    fn a_good_bundle_is_installed_under_its_id() {
        let scratch = Scratch::new("good");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        good_zip(&bundle, "org.test.installed");

        let installed = install(&bundle, &root, false).expect("installing");

        assert_eq!(installed.id, "org.test.installed");
        assert_eq!(installed.anchors, 1);
        assert_eq!(installed.slots, 1);
        assert_eq!(installed.dir, root.join("org.test.installed"));
        assert!(installed.dir.join("plugin.wasm").is_file());
        assert!(installed.dir.join("web/widget.js").is_file());
    }

    /// The registry then finds it through the same load path the daemon uses,
    /// which is the only proof that installing produced a *usable* bundle rather
    /// than a directory that merely exists.
    #[test]
    fn an_installed_bundle_loads_in_a_registry() {
        let scratch = Scratch::new("loads");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        good_zip(&bundle, "org.test.loadable");
        install(&bundle, &root, false).expect("installing");

        let registry = crate::plugins::registry::Registry::load(&root);

        let entry = registry
            .resolve("org.test.loadable")
            .expect("resolving")
            .expect("the installed plugin");
        assert!(
            entry.enabled.is_ok(),
            "{}",
            entry.enabled.as_ref().unwrap_err()
        );
        assert_eq!(entry.ui.len(), 1);
    }

    /// Installed under an id, not under the file name: the directory is the id
    /// because that is what the registry reads, so a bundle called `latest.zip`
    /// must not become a plugin called `latest`.
    #[test]
    fn the_directory_is_the_id_and_not_the_file_name() {
        let scratch = Scratch::new("id");
        let bundle = scratch.path().join("latest.zip");
        let root = scratch.path().join("registry");
        good_zip(&bundle, "org.test.named");

        install(&bundle, &root, false).expect("installing");

        assert!(root.join("org.test.named").is_dir());
        assert!(!root.join("latest").exists());
    }

    #[test]
    fn a_second_install_of_the_same_id_is_a_conflict() {
        let scratch = Scratch::new("conflict");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        good_zip(&bundle, "org.test.twice");
        install(&bundle, &root, false).expect("the first install");

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Conflict);
        assert!(refused.message.contains("--force"), "{}", refused.message);
    }

    #[test]
    fn force_replaces_the_previous_bundle() {
        let scratch = Scratch::new("force");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        good_zip(&bundle, "org.test.forced");
        let first = install(&bundle, &root, false).expect("the first install");
        std::fs::remove_file(first.dir.join("web/widget.js")).expect("removing a file");
        assert!(!first.dir.join("web/widget.js").exists());

        install(&bundle, &root, true).expect("replacing");

        assert!(
            root.join("org.test.forced").join("web/widget.js").is_file(),
            "the replacement must be the new bundle, not the old one"
        );
    }

    /// The refusal is tested on `safe_member`, not only on the archive: the
    /// integration test below passes even when this function accepts `..`,
    /// because the member allowlist then refuses the name. Two mechanisms
    /// answering one question is fine; a test that cannot tell them apart is not,
    /// so each rule is pinned where it lives.
    #[test]
    fn a_relative_path_that_escapes_is_refused() {
        for name in [
            "../secret",
            "web/../../secret",
            "web/../plugin.toml",
            "/etc/passwd",
            "./web/a.js",
        ] {
            assert!(
                crate::plugins::install::safe_member(name).is_none(),
                "`{name}` must be refused"
            );
        }
    }

    /// A doubled separator is accepted because it resolves to the path that is
    /// then written and checked: the name the allowlist approves and the file on
    /// disk stay the same. `./` is not, because accepting it would mean the
    /// archive name and the written path differ.
    #[test]
    fn a_doubled_separator_resolves_to_the_same_path() {
        assert_eq!(
            crate::plugins::install::safe_member("web//a.js").as_deref(),
            Some(std::path::Path::new("web/a.js"))
        );
    }

    #[test]
    fn a_backslash_is_not_a_separator() {
        assert!(crate::plugins::install::safe_member("web\\a.js").is_none());
    }

    /// `../` in an entry name is the whole reason extraction is hand-written.
    #[test]
    fn an_entry_that_escapes_the_bundle_is_refused() {
        let scratch = Scratch::new("escape");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        let wasm = wasm();
        let manifest = manifest("org.test.escape", &digest(&wasm));
        zip_at(
            &bundle,
            &[
                ("plugin.toml", manifest.as_bytes()),
                ("plugin.wasm", &wasm),
                ("../escaped.txt", b"outside"),
            ],
        );

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Forbidden);
        assert!(
            !scratch.path().join("escaped.txt").exists(),
            "the file must not be written outside the staging directory"
        );
    }

    #[test]
    fn an_absolute_entry_is_refused() {
        let scratch = Scratch::new("absolute");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        zip_at(&bundle, &[("/etc/passwd", b"nope")]);

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Forbidden);
    }

    /// A bundle holds three kinds of file. Anything else is code the host never
    /// looked at, arriving in a directory it will load from.
    #[test]
    fn a_file_a_bundle_does_not_hold_is_refused() {
        let scratch = Scratch::new("stray");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        zip_at(&bundle, &[("plugin.toml", b"x"), ("run.sh", b"#!/bin/sh")]);

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Forbidden);
        assert!(refused.message.contains("run.sh"), "{}", refused.message);
    }

    /// A digest that does not name the bytes would install a plugin the registry
    /// disables on the next start, so it is refused here instead.
    #[test]
    fn a_digest_that_does_not_match_is_refused() {
        let scratch = Scratch::new("digest");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        let wasm = wasm();
        let manifest = manifest("org.test.baddigest", &"a".repeat(64));
        zip_at(
            &bundle,
            &[("plugin.toml", manifest.as_bytes()), ("plugin.wasm", &wasm)],
        );

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Invalid);
        assert!(refused.message.contains("digest"), "{}", refused.message);
        assert!(!root.join("org.test.baddigest").exists());
    }

    /// A digest proves the bytes are the approved ones; it does not prove they
    /// parse. Installing a module the host cannot instantiate would leave a
    /// plugin that lists, hashes correctly and fails on every call.
    #[test]
    fn a_module_that_does_not_compile_is_refused() {
        let scratch = Scratch::new("badwasm");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        let bytes = b"\0asm definitely not wasm";
        let manifest = manifest("org.test.badwasm", &digest(bytes));
        zip_at(
            &bundle,
            &[("plugin.toml", manifest.as_bytes()), ("plugin.wasm", bytes)],
        );

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Invalid);
        assert!(
            refused.message.contains("not a valid module"),
            "{}",
            refused.message
        );
    }

    #[test]
    fn a_bundle_with_no_manifest_is_refused() {
        let scratch = Scratch::new("nomanifest");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        zip_at(&bundle, &[("plugin.wasm", &wasm())]);

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Invalid);
        assert!(
            refused.message.contains("plugin.toml"),
            "{}",
            refused.message
        );
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_refused() {
        let scratch = Scratch::new("notzip");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        std::fs::write(&bundle, b"not a zip at all").expect("writing");

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Invalid);
        assert!(refused.message.contains("not a zip"), "{}", refused.message);
    }

    #[test]
    fn a_missing_bundle_names_the_packaging_step() {
        let scratch = Scratch::new("missing");
        let root = scratch.path().join("registry");

        let refused = error(install(&scratch.path().join("nope.zip"), &root, false));

        assert_eq!(refused.kind, ErrorKind::NotFound);
        assert!(
            refused.message.contains("package.sh"),
            "{}",
            refused.message
        );
    }

    /// The staging directory is the promise that a refusal leaves nothing
    /// behind. Checked by listing the root, not by trusting the `Drop`.
    #[test]
    fn a_refusal_late_in_the_install_leaves_the_root_empty() {
        let scratch = Scratch::new("cleanup");
        let root = scratch.path().join("registry");
        let bundle = scratch.path().join("bundle.zip");
        let wasm = wasm();
        // Everything extracts, then the digest refuses: the case where files
        // have already been written and the cleanup is what matters.
        let manifest = manifest("org.test.clean", &"b".repeat(64));
        zip_at(
            &bundle,
            &[("plugin.toml", manifest.as_bytes()), ("plugin.wasm", &wasm)],
        );

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::Invalid);
        let left: Vec<_> = std::fs::read_dir(&root)
            .map(|read| read.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "left {left:?} behind in the registry root");
    }

    /// Duplicate entries make the contents depend on read order, and the second
    /// write would target a file that already exists.
    ///
    /// Tested on the validator rather than through an archive: `zip`'s reader
    /// collapses repeated names, so an archive cannot present the case. The
    /// validator is the code that has to refuse it anyway — it is what stands
    /// between a reader that keeps duplicates and a bundle whose contents depend
    /// on iteration order.
    #[test]
    fn a_name_that_appears_twice_is_refused() {
        let mut seen = std::collections::HashSet::new();

        crate::plugins::install::plan_member("web/a.js", &mut seen).expect("the first mention");

        let refused = error(crate::plugins::install::plan_member("web/a.js", &mut seen));
        assert_eq!(refused.kind, ErrorKind::Invalid);
        assert!(refused.message.contains("twice"), "{}", refused.message);
    }

    /// An archive with more entries than the limit is refused before anything is
    /// written, so the bound is on work as well as on size.
    #[test]
    fn too_many_entries_are_refused() {
        let scratch = Scratch::new("entries");
        let bundle = scratch.path().join("bundle.zip");
        let root = scratch.path().join("registry");
        let names: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("web/{i}.js")).collect();
        let entries: Vec<(&str, &[u8])> = names
            .iter()
            .map(|name| (name.as_str(), b"x".as_slice()))
            .collect();
        zip_at(&bundle, &entries);

        let refused = error(install(&bundle, &root, false));

        assert_eq!(refused.kind, ErrorKind::TooLarge);
        assert!(
            refused.message.contains(&MAX_ENTRIES.to_string()),
            "{}",
            refused.message
        );
    }
}

/// Removing a bundle: the one operation that deletes a directory tree.
///
/// The id arrives from an argument, so every test here is about what it can
/// reach rather than about what it returns.
mod remove {
    use crate::plugins::install::remove;
    use anth_wire::ErrorKind;
    use std::path::{Path, PathBuf};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!(
                "anth-remove-{label}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("creating the scratch root");
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A directory that looks like an installed bundle.
    fn plugin_root(root: &Path, id: &str) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(dir.join("web")).expect("creating the bundle");
        std::fs::write(dir.join("plugin.toml"), "[plugin]\nid = \"x\"\n").expect("writing");
        std::fs::write(dir.join("plugin.wasm"), b"wasm").expect("writing");
        dir
    }

    #[test]
    fn an_installed_bundle_is_removed() {
        let scratch = Scratch::new("plain");
        let dir = plugin_root(scratch.path(), "org.test.gone");

        let removed = remove(scratch.path(), "org.test.gone").expect("removing");

        assert_eq!(removed, dir);
        assert!(!dir.exists());
    }

    /// The id is validated before a path is built, or `..` would name a sibling
    /// of the root and `remove_dir_all` would delete it.
    ///
    /// The victim is a fully valid plugin, so a test that leaves it in place is
    /// proving the validation ran, not the later "is this a plugin" check.
    #[test]
    fn an_id_that_escapes_the_root_is_refused() {
        let scratch = Scratch::new("escape");
        // A real registry root, so `..` names a directory that exists and is a
        // sibling rather than the scratch directory's parent.
        let root = scratch.path().join("registry");
        std::fs::create_dir_all(&root).expect("creating the root");
        let victim = scratch.path().join("victim");
        std::fs::create_dir_all(&victim).expect("creating the victim");
        std::fs::write(victim.join("keep.txt"), b"precious").expect("writing");
        // The victim holds a `plugin.toml`, so the "not a plugin" guard cannot
        // be what refuses these ids: only the id validation stands between
        // `../victim` and a recursive delete of a sibling of the registry.
        std::fs::write(victim.join("plugin.toml"), b"[plugin]\n").expect("writing");

        for id in ["../victim", "..", ".", "/", "a/b", "", "org.test.Upper"] {
            let refused = remove(&root, id);
            assert!(
                refused.is_err(),
                "`{id}` must not name anything removable, got {refused:?}"
            );
            assert!(
                victim.join("plugin.toml").is_file(),
                "`{id}` reached the victim directory"
            );
        }
    }

    #[test]
    fn a_plugin_that_is_not_installed_is_not_found() {
        let scratch = Scratch::new("missing");

        let refused = remove(scratch.path(), "org.test.absent").expect_err("removing");

        assert_eq!(refused.kind, ErrorKind::NotFound);
        assert!(
            refused.message.contains("not installed"),
            "{}",
            refused.message
        );
    }

    /// A registry root may hold more than bundles. Deleting a directory that is
    /// not a plugin would be a surprise the operator never asked for, so a
    /// directory without a manifest is refused.
    #[test]
    fn a_directory_that_is_not_a_plugin_is_refused() {
        let scratch = Scratch::new("notaplugin");
        let not_a_plugin = scratch.path().join("org.test.junk");
        std::fs::create_dir_all(&not_a_plugin).expect("creating");
        std::fs::write(not_a_plugin.join("notes.txt"), b"mine").expect("writing");

        let refused = remove(scratch.path(), "org.test.junk").expect_err("removing");

        assert_eq!(refused.kind, ErrorKind::Conflict);
        assert!(not_a_plugin.join("notes.txt").is_file());
    }

    /// A symlink to a directory is not a plugin either, and following it would
    /// delete the target rather than the link.
    #[test]
    fn a_symlinked_directory_is_refused() {
        let scratch = Scratch::new("symlink");
        let elsewhere = scratch.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("creating");
        std::fs::write(elsewhere.join("plugin.toml"), b"x").expect("writing");
        let link = scratch.path().join("org.test.link");
        std::os::unix::fs::symlink(&elsewhere, &link).expect("creating the link");

        let refused = remove(scratch.path(), "org.test.link").expect_err("removing");

        assert_eq!(refused.kind, ErrorKind::Conflict);
        assert!(
            elsewhere.join("plugin.toml").is_file(),
            "the symlink target must survive"
        );
    }
}
