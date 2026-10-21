//! Bundle tests. Every case builds a real zip in a temporary directory and
//! installs it, so the path under test is the one the daemon runs.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::Sha256;

use super::*;

/// A minimal recipe, for the grant tests that only care about the id.
fn capability(id: &str, program: &str, argv: &[&str]) -> Capability {
    Capability {
        id: id.to_string(),
        job: None,
        program: Some(program.to_string()),
        argv: argv.iter().map(|part| (*part).to_string()).collect(),
        args: Default::default(),
        accepts_text: false,
    }
}
use crate::plugins::manifest::{Capability, GrantedCapability};
use crate::plugins::{PluginRegistry, PluginsConfig, install_bundle};

struct Member {
    name: &'static str,
    bytes: Vec<u8>,
}

fn wasm_bytes() -> Vec<u8> {
    // An empty but well-formed module: the installer hashes it and then parses
    // it, so the fixture has to survive both.
    b"\0asm\x01\0\0\0".to_vec()
}

fn sha_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn manifest_text(sha: &str, extra: &str) -> String {
    format!(
        r#"
[plugin]
id = "org.anth.test"
version = "0.1.0"
name = "Test"
abi = 1

[wasm]
path = "plugin.wasm"
sha256 = "{sha}"
{extra}
"#
    )
}

fn write_bundle(dir: &Path, name: &str, members: &[Member]) -> PathBuf {
    let path = dir.join(format!("{name}.zip"));
    let file = fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for member in members {
        zip.start_file(member.name, options).unwrap();
        zip.write_all(&member.bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn valid_bundle(dir: &Path, extra: &str) -> PathBuf {
    let wasm = wasm_bytes();
    let manifest = manifest_text(&sha_of(&wasm), extra);
    write_bundle(
        dir,
        "valid",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm,
            },
        ],
    )
}

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("anth-plugin-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn config_at(root: &Path) -> PluginsConfig {
    PluginsConfig {
        enabled: true,
        root: root.to_string_lossy().into_owned(),
        ..PluginsConfig::default()
    }
}

#[test]
fn a_valid_bundle_installs_and_records_a_grant() {
    let dir = scratch("install");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));

    let (installed, grant) = install_bundle(&bundle, &config).unwrap();

    assert_eq!(installed.plugin.id, "org.anth.test");
    assert_eq!(grant.id, "org.anth.test");
    assert_eq!(grant.wasm_sha256, sha_of(&wasm_bytes()));
    assert!(installed.root.join("plugin.wasm").exists());
    assert!(installed.root.join("plugin.toml").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_bundle_with_a_wrong_hash_is_refused_and_leaves_nothing() {
    let dir = scratch("hash");
    let manifest = manifest_text(&"0".repeat(64), "");
    let bundle = write_bundle(
        &dir,
        "bad-hash",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm_bytes(),
            },
        ],
    );
    let root = dir.join("plugins");
    let config = config_at(&root);

    assert_eq!(
        install_bundle(&bundle, &config).unwrap_err().kind,
        PluginErrorKind::Tampered
    );

    // No plugin directory and no staging leftovers: "installed minus a file" is
    // worse than "not installed".
    assert!(!root.join("org.anth.test").exists());
    let leftovers: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging"))
        .collect();
    assert!(leftovers.is_empty(), "staging was not cleaned up");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_member_outside_the_bundle_shape_is_refused() {
    let dir = scratch("member");
    let wasm = wasm_bytes();
    let manifest = manifest_text(&sha_of(&wasm), "");
    let bundle = write_bundle(
        &dir,
        "extra-member",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm,
            },
            Member {
                name: "README.md",
                bytes: b"hello".to_vec(),
            },
        ],
    );
    let config = config_at(&dir.join("plugins"));

    let error = install_bundle(&bundle, &config).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("README.md"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_member_escaping_the_bundle_is_refused() {
    let dir = scratch("slip");
    let wasm = wasm_bytes();
    let manifest = manifest_text(&sha_of(&wasm), "");
    let bundle = write_bundle(
        &dir,
        "slip",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm,
            },
            Member {
                name: "../escaped.js",
                bytes: b"alert(1)".to_vec(),
            },
        ],
    );
    let config = config_at(&dir.join("plugins"));

    assert_eq!(
        install_bundle(&bundle, &config).unwrap_err().kind,
        PluginErrorKind::Denied
    );
    assert!(!dir.join("escaped.js").exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_bundle_without_a_manifest_is_refused() {
    let dir = scratch("no-manifest");
    let bundle = write_bundle(
        &dir,
        "no-manifest",
        &[Member {
            name: "plugin.wasm",
            bytes: wasm_bytes(),
        }],
    );
    let config = config_at(&dir.join("plugins"));

    let error = install_bundle(&bundle, &config).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Io);
    assert!(error.detail.contains("plugin.toml"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_second_install_of_the_same_id_is_a_conflict() {
    let dir = scratch("conflict");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));

    install_bundle(&bundle, &config).unwrap();
    assert_eq!(
        install_bundle(&bundle, &config).unwrap_err().kind,
        PluginErrorKind::Conflict
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn installing_is_refused_while_plugins_are_disabled() {
    let dir = scratch("disabled");
    let bundle = valid_bundle(&dir, "");
    let config = PluginsConfig {
        enabled: false,
        root: dir.join("plugins").to_string_lossy().into_owned(),
        ..PluginsConfig::default()
    };

    let error = install_bundle(&bundle, &config).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("disabled"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn web_assets_install_beside_the_module() {
    let dir = scratch("web");
    let wasm = wasm_bytes();
    let manifest = manifest_text(
        &sha_of(&wasm),
        "\n[[ui]]\nslot = \"page\"\ntag = \"anth-test\"\nmodule = \"web/card.js\"\n",
    );
    let bundle = write_bundle(
        &dir,
        "web",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm,
            },
            Member {
                name: "web/card.js",
                bytes: b"export {};".to_vec(),
            },
        ],
    );
    let config = config_at(&dir.join("plugins"));

    let (installed, _grant) = install_bundle(&bundle, &config).unwrap();
    assert!(installed.root.join("web/card.js").exists());
    assert_eq!(installed.plugin.ui[0].tag, "anth-test");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_manifest_pointing_at_another_module_is_refused() {
    let dir = scratch("repoint");
    let wasm = wasm_bytes();
    let manifest = manifest_text(&sha_of(&wasm), "")
        .replace("path = \"plugin.wasm\"", "path = \"other.wasm\"");
    let bundle = write_bundle(
        &dir,
        "repoint",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm,
            },
        ],
    );
    let config = config_at(&dir.join("plugins"));

    let error = install_bundle(&bundle, &config).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Invalid);
    assert!(error.detail.contains("plugin.wasm"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rewritten_module_is_caught_on_load() {
    let dir = scratch("tamper");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));
    let (installed, grant) = install_bundle(&bundle, &config).unwrap();

    fs::write(installed.root.join("plugin.wasm"), b"\0asm\x01\0\0\0EVIL").unwrap();

    assert_eq!(
        load(&installed.root, &grant).unwrap_err().kind,
        PluginErrorKind::Tampered
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rewritten_manifest_version_is_caught_on_load() {
    let dir = scratch("version");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));
    let (installed, grant) = install_bundle(&bundle, &config).unwrap();

    let text = fs::read_to_string(installed.root.join("plugin.toml")).unwrap();
    fs::write(
        installed.root.join("plugin.toml"),
        text.replace("version = \"0.1.0\"", "version = \"9.9.9\""),
    )
    .unwrap();

    let error = load(&installed.root, &grant).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Tampered);
    assert!(error.detail.contains("9.9.9"));
    fs::remove_dir_all(&dir).unwrap();
}

/// The case the previous version of `load` missed: rewriting the module *and*
/// updating the manifest's own hash keeps the pair consistent, so a check that
/// trusts the manifest passes. Only the grant's digest can catch it.
#[test]
fn a_rewritten_module_with_a_matching_manifest_hash_is_caught() {
    let dir = scratch("rewrite-both");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));
    let (installed, grant) = install_bundle(&bundle, &config).unwrap();

    let evil = b"\0asm\x01\0\0\0EVIL".to_vec();
    fs::write(installed.root.join("plugin.wasm"), &evil).unwrap();
    let text = manifest_text(&sha_of(&evil), "");
    fs::write(installed.root.join("plugin.toml"), text).unwrap();

    let error = load(&installed.root, &grant).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Tampered);
    assert!(error.detail.contains("the grant says"), "{}", error.detail);
    fs::remove_dir_all(&dir).unwrap();
}

/// A plugin may rewrite its manifest to claim capabilities. The advertised set
/// comes from the grant, so the panel can never display authority that does not
/// exist.
#[test]
fn a_loaded_plugin_advertises_the_granted_capabilities_not_the_manifests() {
    let dir = scratch("claim");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));
    let (installed, grant) = install_bundle(&bundle, &config).unwrap();

    // The grant froze no capability; the manifest now claims one.
    assert!(grant.allowed_ids().is_empty());
    let wasm = wasm_bytes();
    fs::write(
        installed.root.join("plugin.toml"),
        manifest_text(
            &sha_of(&wasm),
            "\n[[capability]]\nid = \"wg.delete\"\nprogram = \"wg\"\nargv = [\"show\"]\n",
        ),
    )
    .unwrap();

    let reloaded = load(&installed.root, &grant).unwrap();
    assert!(
        reloaded.plugin.capabilities.is_empty(),
        "advertised {:?}",
        reloaded.plugin.capabilities
    );
    assert!(!reloaded.plugin.enabled || grant.enabled);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_advertised_state_follows_the_grant() {
    let dir = scratch("state");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));
    let (installed, grant) = install_bundle(&bundle, &config).unwrap();

    let mut disabled = grant;
    disabled.enabled = false;
    assert!(!load(&installed.root, &disabled).unwrap().plugin.enabled);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_id_with_a_path_separator_cannot_reach_outside_the_root() {
    let dir = scratch("traversal");
    let root = dir.join("plugins");
    fs::create_dir_all(&root).unwrap();

    let victim = dir.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("keep"), b"important").unwrap();

    let error = remove(&root, "../victim").unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Invalid);
    assert!(victim.join("keep").exists(), "the sibling was deleted");

    assert!(plugin_dir(&root, "../victim").is_err());
    assert!(plugin_dir(&root, "/etc").is_err());
    assert!(plugin_dir(&root, "org.anth.test").is_ok());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn loading_an_unchanged_bundle_succeeds() {
    let dir = scratch("reload");
    let bundle = valid_bundle(&dir, "");
    let config = config_at(&dir.join("plugins"));
    let (installed, grant) = install_bundle(&bundle, &config).unwrap();

    assert_eq!(
        load(&installed.root, &grant).unwrap().plugin.id,
        "org.anth.test"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn listing_reports_an_installed_plugin_and_an_orphan_directory() {
    let dir = scratch("list");
    let bundle = valid_bundle(&dir, "");
    let root = dir.join("plugins");
    let config = config_at(&root);
    let (_installed, grant) = install_bundle(&bundle, &config).unwrap();

    fs::create_dir_all(root.join("org.anth.orphan")).unwrap();

    let (plugins, problems) = list(&root, &[grant]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "org.anth.test");
    assert!(
        problems.iter().any(|p| p.contains("org.anth.orphan")),
        "{problems:?}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn listing_a_missing_root_is_empty_not_an_error() {
    let dir = scratch("missing-root");
    let (plugins, problems) = list(&dir.join("nothing"), &[]);
    assert!(plugins.is_empty());
    assert!(problems.is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn removing_an_installed_plugin_deletes_its_directory() {
    let dir = scratch("remove");
    let bundle = valid_bundle(&dir, "");
    let root = dir.join("plugins");
    let config = config_at(&root);
    let (installed, _grant) = install_bundle(&bundle, &config).unwrap();

    remove(&root, "org.anth.test").unwrap();
    assert!(!installed.root.exists());
    assert_eq!(
        remove(&root, "org.anth.test").unwrap_err().kind,
        PluginErrorKind::Missing
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_registry_hides_everything_when_plugins_are_disabled() {
    let dir = scratch("registry-off");
    let bundle = valid_bundle(&dir, "");
    let root = dir.join("plugins");
    let config = config_at(&root);
    let (_installed, grant) = install_bundle(&bundle, &config).unwrap();

    let mut installed = config.clone();
    installed.upsert_grant(grant);
    assert_eq!(PluginRegistry::load(&installed).plugins().len(), 1);

    let mut off = installed.clone();
    off.enabled = false;
    let registry = PluginRegistry::load(&off);
    assert!(registry.plugins().is_empty());
    assert!(registry.problems().is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_capability_is_allowed_only_through_its_grant() {
    let mut config = PluginsConfig {
        enabled: true,
        ..PluginsConfig::default()
    };
    config.upsert_grant(PluginGrant {
        id: "org.anth.test".to_string(),
        version: "0.1.0".to_string(),
        wasm_sha256: "0".repeat(64),
        recipes: vec![GrantedCapability::from(&capability(
            "wg.status",
            "wg",
            &["show"],
        ))],
        enabled: true,
    });

    assert!(config.allows("org.anth.test", "wg.status"));
    assert!(!config.allows("org.anth.test", "wg.write"));
    assert!(!config.allows("org.anth.other", "wg.status"));

    config.enabled = false;
    assert!(!config.allows("org.anth.test", "wg.status"));
}

#[test]
fn a_grant_is_replaced_rather_than_duplicated() {
    let mut config = PluginsConfig::default();
    let base = PluginGrant {
        id: "org.anth.test".to_string(),
        version: "0.1.0".to_string(),
        wasm_sha256: "0".repeat(64),
        recipes: vec![GrantedCapability::from(&capability("a", "wg", &["show"]))],
        enabled: true,
    };
    config.upsert_grant(base.clone());

    let mut approved = base;
    approved.version = "0.2.0".to_string();
    approved
        .recipes
        .push(GrantedCapability::from(&capability("b", "wg", &["show"])));
    config.upsert_grant(approved);

    assert_eq!(config.grants.len(), 1);
    assert_eq!(config.grants[0].version, "0.2.0");
    assert_eq!(config.grants[0].allowed_ids(), vec!["a", "b"]);
}

#[test]
fn an_install_refuses_a_module_the_host_cannot_load() {
    let dir = scratch("notwasm");
    let bytes = b"this is not wasm".to_vec();
    let bundle = write_bundle(
        &dir,
        "notwasm",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest_text(&sha_of(&bytes), "").into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes,
            },
        ],
    );
    let config = config_at(&dir.join("plugins"));

    let error = install_bundle(&bundle, &config).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Invalid, "{}", error.detail);
    assert!(
        error.detail.contains("cannot be loaded"),
        "{}",
        error.detail
    );
    assert!(
        !dir.join("plugins").join("org.anth.test").exists(),
        "a refused bundle must leave nothing behind"
    );
}

#[test]
fn a_grant_freezes_the_program_and_the_template_not_only_the_id() {
    // Phase 8a froze ids. Phase 8b needs the recipe itself, because `invoke`
    // renders argv from the grant: if only the id were stored, the program and
    // template would come from the manifest, which an attacker can rewrite.
    let dir = scratch("recipe");
    let bundle = valid_bundle(
        &dir,
        "\n[[capability]]\nid = \"wg.status\"\nprogram = \"wg\"\n\
         argv = [\"show\", \"{interface}\", \"dump\"]\n\
         [capability.args.interface]\ntype = \"ifname\"\n",
    );
    let config = config_at(&dir.join("plugins"));
    let (_, grant) = install_bundle(&bundle, &config).unwrap();

    let recipe = grant.recipe("wg.status").expect("the recipe is frozen");
    assert_eq!(recipe.program.as_deref(), Some("wg"));
    assert_eq!(recipe.argv, vec!["show", "{interface}", "dump"]);
    assert!(
        recipe.args.contains_key("interface"),
        "the validator must be frozen with the recipe, not re-read from the manifest"
    );

    // And it survives a config round trip, which is what the daemon reads on the
    // next start.
    let mut config = config;
    config.upsert_grant(grant);
    let text = toml::to_string(&config).unwrap();
    let reloaded: PluginsConfig = toml::from_str(&text).unwrap();
    let recipe = reloaded.grants[0].recipe("wg.status").unwrap();
    assert_eq!(recipe.argv, vec!["show", "{interface}", "dump"]);
    assert!(recipe.args.contains_key("interface"));
}

#[test]
fn grants_round_trip_through_toml_inside_the_config() {
    let mut config = PluginsConfig::default();
    config.upsert_grant(PluginGrant {
        id: "org.anth.test".to_string(),
        version: "0.1.0".to_string(),
        wasm_sha256: "a".repeat(64),
        recipes: vec![GrantedCapability::from(&capability(
            "wg.status",
            "wg",
            &["show"],
        ))],
        enabled: true,
    });

    let text = toml::to_string(&config).unwrap();
    let parsed: PluginsConfig = toml::from_str(&text).unwrap();
    assert_eq!(parsed, config);
}

#[test]
fn grants_by_id_indexes_every_grant() {
    let grants = vec![
        PluginGrant {
            id: "org.anth.one".to_string(),
            version: "0.1.0".to_string(),
            wasm_sha256: "0".repeat(64),
            recipes: Vec::new(),
            enabled: true,
        },
        PluginGrant {
            id: "org.anth.two".to_string(),
            version: "0.1.0".to_string(),
            wasm_sha256: "1".repeat(64),
            recipes: Vec::new(),
            enabled: true,
        },
    ];
    let index = grants_by_id(&grants);
    assert_eq!(index.len(), 2);
    assert!(index.contains_key("org.anth.two"));
}

#[test]
fn an_installed_plugin_advertises_its_anchors() {
    let dir = scratch("anchors");
    let wasm = wasm_bytes();
    let manifest = manifest_text(
        &sha_of(&wasm),
        "\n[[api]]\nmethod = \"GET\"\npath = \"/status\"\n\n[[ui]]\nslot = \"dashboard.widget\"\ntag = \"anth-test\"\nmodule = \"web/card.js\"\n",
    );
    let bundle = write_bundle(
        &dir,
        "anchors",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm,
            },
            Member {
                name: "web/card.js",
                bytes: b"export {};".to_vec(),
            },
        ],
    );
    let root = dir.join("plugins");
    let config = config_at(&root);
    let (_installed, grant) = install_bundle(&bundle, &config).unwrap();

    let mut config = config;
    config.upsert_grant(grant);
    let registry = PluginRegistry::load(&config);

    let api = registry.api_anchors();
    assert_eq!(api.len(), 1);
    assert_eq!(api[0].0, "org.anth.test");
    assert_eq!(api[0].1.path, "/status");

    let ui = registry.ui_anchors();
    assert_eq!(ui.len(), 1);
    assert_eq!(ui[0].1.slot, "dashboard.widget");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_capability_job_is_accepted_without_a_program() {
    let dir = scratch("job");
    let wasm = wasm_bytes();
    let manifest = manifest_text(
        &sha_of(&wasm),
        "\n[[capability]]\nid = \"host.read\"\njob = \"host.info\"\n",
    );
    let bundle = write_bundle(
        &dir,
        "job",
        &[
            Member {
                name: "plugin.toml",
                bytes: manifest.into_bytes(),
            },
            Member {
                name: "plugin.wasm",
                bytes: wasm,
            },
        ],
    );
    let config = config_at(&dir.join("plugins"));
    let (_installed, grant) = install_bundle(&bundle, &config).unwrap();
    assert_eq!(grant.allowed_ids(), vec!["host.read".to_string()]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_broken_plugin_does_not_stop_the_others_from_loading() {
    let dir = scratch("partial");
    let bundle = valid_bundle(&dir, "");
    let root = dir.join("plugins");
    let config = config_at(&root);
    let (_installed, grant) = install_bundle(&bundle, &config).unwrap();

    let broken_root = root.join("org.anth.broken");
    fs::create_dir_all(&broken_root).unwrap();
    let wasm = wasm_bytes();
    fs::write(
        broken_root.join("plugin.toml"),
        manifest_text(&sha_of(&wasm), "").replace("org.anth.test", "org.anth.broken"),
    )
    .unwrap();
    fs::write(broken_root.join("plugin.wasm"), b"tampered").unwrap();

    let broken_grant = PluginGrant {
        id: "org.anth.broken".to_string(),
        version: "0.1.0".to_string(),
        wasm_sha256: sha_of(&wasm),
        recipes: Vec::new(),
        enabled: true,
    };

    let (plugins, problems) = list(&root, &[grant, broken_grant]);
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, "org.anth.test");
    assert!(problems.iter().any(|p| p.contains("org.anth.broken")));
    fs::remove_dir_all(&dir).unwrap();
}
