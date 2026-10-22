use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::config::{Config, DEFAULT_CONFIG_PATH, DEFAULT_SOCKET_PATH};

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("anth-panel-{name}-{}-{nanos}", std::process::id()))
}

#[test]
fn constants_point_at_the_expected_defaults() {
    assert_eq!(DEFAULT_CONFIG_PATH, "/etc/anth/config.toml");
    assert_eq!(DEFAULT_SOCKET_PATH, "/var/tmp/anth.sock");
}

#[test]
fn default_config_uses_the_default_socket_path() {
    let config = Config::default();
    assert_eq!(config.socket_path, DEFAULT_SOCKET_PATH);
}

#[test]
fn toml_roundtrip_preserves_values() {
    let config = Config {
        socket_path: "/tmp/custom.sock".to_string(),
        ..Config::default()
    };

    let text = config.to_toml().expect("serialization should work");
    assert_eq!(Config::from_toml(&text).unwrap(), config);
}

#[test]
fn partial_or_empty_toml_falls_back_to_defaults() {
    assert_eq!(Config::from_toml("").unwrap(), Config::default());

    let config = Config::from_toml("socket_path = \"/tmp/only.sock\"\n").unwrap();
    assert_eq!(config.socket_path, "/tmp/only.sock");
}

#[test]
fn unknown_keys_are_rejected() {
    let err = Config::from_toml("sockket_path = \"/tmp/typo.sock\"\n").unwrap_err();
    assert!(
        err.to_string().contains("sockket_path"),
        "error should name the unknown key: {err}"
    );
}

#[test]
fn mistyped_values_are_rejected() {
    assert!(Config::from_toml("socket_path = 42\n").is_err());
    assert!(Config::from_toml("this is not = = toml").is_err());
}

#[test]
fn load_and_save_roundtrip_creates_missing_parent_directories() {
    let dir = unique_dir("save");
    let path = dir.join("a").join("b").join("config.toml");

    let config = Config {
        socket_path: "/tmp/roundtrip.sock".to_string(),
        ..Config::default()
    };
    config.save(&path).expect("save should create parents");

    assert!(path.exists());
    assert_eq!(Config::load(&path).unwrap(), config);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_missing_file_reports_the_path() {
    let path = unique_dir("missing").join("config.toml");
    let err = Config::load(&path).unwrap_err();
    assert!(
        err.contains(&path.display().to_string()),
        "error should mention the missing path: {err}"
    );
}

#[test]
fn load_invalid_file_reports_a_parse_error() {
    let dir = unique_dir("invalid");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "not = = toml\n").unwrap();

    let err = Config::load(&path).unwrap_err();
    assert!(
        err.contains("Error parsing config"),
        "expected a parse error, got: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_or_default_falls_back_without_touching_the_filesystem() {
    let dir = unique_dir("no-write");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    let config = Config::load_or_default(&path);

    assert_eq!(config, Config::default());
    assert!(
        !path.exists(),
        "load_or_default must not create the config file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Everything a first start keeps from `server_default`; the `[auth]` section is
/// generated per run and is compared separately.
fn without_auth(config: &Config) -> Config {
    let mut copy = config.clone();
    copy.auth = Default::default();
    copy
}

#[test]
fn load_or_create_writes_the_default_when_the_file_is_missing() {
    let dir = unique_dir("create");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    assert!(!path.exists());

    let config = Config::load_or_create(&path).unwrap();

    assert_eq!(
        without_auth(&config),
        without_auth(&Config::server_default())
    );
    assert!(path.exists(), "a default config should have been written");
    assert_eq!(
        without_auth(&Config::load(&path).unwrap()),
        without_auth(&config)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_start_creates_an_admin_account() {
    let dir = unique_dir("bootstrap");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    let config = Config::load_or_create(&path).unwrap();

    assert!(
        config.auth.enabled,
        "the generated config must require a login"
    );
    assert!(config.auth.is_configured());
    assert_eq!(config.auth.username, "admin");
    assert!(
        config.auth.password_hash.starts_with("pbkdf2-sha256$"),
        "only a digest may reach the file"
    );

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("bootstrap_password"),
        "the one-time password must never be serialized"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_bootstrap_password_is_printed_once_and_never_stored() {
    let mut config = Config::default();
    assert!(config.bootstrap_auth());
    let password = config
        .take_bootstrap_password()
        .expect("a password was generated");
    assert_eq!(password.chars().count(), 20);
    assert!(
        config.take_bootstrap_password().is_none(),
        "it is shown once"
    );
    assert!(anth_panel::auth::password::verify_password(
        &password,
        &config.auth.password_hash
    ));
}

#[test]
fn a_second_start_does_not_replace_the_account() {
    let dir = unique_dir("bootstrap-once");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    let first = Config::load_or_create(&path).unwrap();
    let second = Config::load_or_create(&path).unwrap();

    assert_eq!(first.auth.password_hash, second.auth.password_hash);
    assert_eq!(first.auth.jwt_secret, second.auth.jwt_secret);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_config_written_before_auth_existed_gets_a_secret() {
    let mut config = Config::default();
    assert!(config.bootstrap_auth());
    let hash = config.auth.password_hash.clone();
    let secret = config.auth.jwt_secret.clone();

    // Simulate a reload: the one-time password is gone but the credential stays.
    let mut reloaded = Config::default();
    reloaded.auth.password_hash = hash.clone();
    assert!(reloaded.bootstrap_auth());
    assert_eq!(reloaded.auth.password_hash, hash);
    assert_ne!(reloaded.auth.jwt_secret, secret);
}

#[test]
fn load_or_create_creates_missing_parent_directories() {
    let dir = unique_dir("create-nested");
    let path = dir.join("etc").join("anth").join("config.toml");

    let config = Config::load_or_create(&path).unwrap();

    assert_eq!(
        without_auth(&config),
        without_auth(&Config::server_default())
    );
    assert!(path.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_or_create_keeps_an_existing_file_untouched() {
    let dir = unique_dir("keep");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    let custom = Config {
        socket_path: "/tmp/custom.sock".to_string(),
        ..Config::default()
    };
    custom.save(&path).unwrap();

    let config = Config::load_or_create(&path).unwrap();

    assert_eq!(without_auth(&config), custom);
    assert_eq!(
        Config::load(&path).unwrap().auth.password_hash,
        config.auth.password_hash
    );
    assert_eq!(Config::load(&path).unwrap().socket_path, "/tmp/custom.sock");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A malformed file is refused, not replaced.
///
/// The previous behaviour wrote defaults over it. That turns any schema change
/// into data loss: a plugin grant written by an older version failed to parse
/// and the operator's `files.root`, `auth.username` and `jwt_secret` were
/// silently gone. "No config" bootstraps; "config that will not parse" stops.
#[test]
fn load_or_create_refuses_a_malformed_file_and_leaves_it_alone() {
    let dir = unique_dir("malformed");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let original = "not = = toml\n";
    std::fs::write(&path, original).unwrap();

    let error = Config::load_or_create(&path).unwrap_err();
    assert!(
        error.contains("parsing") || error.contains("parse"),
        "the error must say the file did not parse: {error}"
    );

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "the file is the operator's and must survive a failed load byte for byte"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The case that motivated the split: a config that parses under no current
/// schema but holds settings nobody can reconstruct.
#[test]
fn a_stale_plugin_grant_does_not_take_the_rest_of_the_config_with_it() {
    let dir = unique_dir("stale-grant");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    // Written by hand rather than by `save`, because the point is a file the
    // current schema rejects: the settings below must survive regardless.
    let text = "socket_path = \"/tmp/custom.sock\"\n\
                \n[files]\nroot = \"/srv/data\"\n\
                \n[auth]\nusername = \"operator\"\n\
                \n[[plugins.grants]]\nid = \"org.anth.test\"\nversion = \"0.1.0\"\nwasm_sha256 = \"aa\"\ncapabilities = [\"wg.status\"]\nenabled = true\n";
    std::fs::write(&path, text).unwrap();

    let error = Config::load_or_create(&path).unwrap_err();
    assert!(error.contains("capabilities"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "the operator's settings, including the stale grant, stay on disk"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn process_control_is_off_by_default() {
    assert!(!Config::default().process.control);
    assert!(!Config::from_toml("").unwrap().process.control);
}

#[test]
fn process_control_can_be_enabled_from_toml() {
    let config = Config::from_toml("[process]\ncontrol = true\n").unwrap();
    assert!(config.process.control);
}

#[test]
fn unknown_process_keys_are_rejected() {
    assert!(Config::from_toml("[process]\ncontroll = true\n").is_err());
}

#[test]
fn file_writes_are_off_by_default_and_can_be_enabled() {
    assert!(!Config::default().files.write);
    assert!(!Config::from_toml("").unwrap().files.write);
    assert_eq!(Config::default().files.root, "/");

    let config = Config::from_toml(
        "[files]\nroot = \"/srv\"\nwrite = true\nmax_entries = 7\nmax_preview_bytes = 8\nmax_upload_bytes = 9\n",
    )
    .unwrap();
    assert_eq!(config.files.root, "/srv");
    assert!(config.files.write);
    assert_eq!(config.files.max_entries, 7);
    assert_eq!(config.files.max_preview_bytes, 8);
    assert_eq!(config.files.max_upload_bytes, 9);
}

#[test]
fn unknown_files_keys_are_rejected() {
    assert!(Config::from_toml("[files]\nroot = \"/\"\nwrit = true\n").is_err());
}

#[test]
fn dev_defaults_enable_process_control_and_file_writes() {
    let config = Config::dev_default();
    assert!(config.process.control);
    assert!(config.files.write);

    let toml = config.to_toml().unwrap();
    assert!(toml.contains("control = true"), "{toml}");
    assert!(toml.contains("write = true"), "{toml}");
}

#[test]
fn server_defaults_follow_the_build_profile() {
    if cfg!(debug_assertions) {
        assert_eq!(Config::server_default(), Config::dev_default());
    } else {
        assert_eq!(Config::server_default(), Config::default());
    }
}

#[test]
fn legacy_grants_migrate_to_one_file_per_plugin() {
    let dir = unique_dir("grant-migration");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let text = r#"
socket_path = "/tmp/migrate.sock"

[plugins]
enabled = true

[[plugins.grants]]
id = "org.anth.test"
version = "0.1.0"
wasm_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
enabled = true
recipes = []
"#;
    std::fs::write(&path, text).unwrap();

    let config = Config::load_or_create(&path).expect("migration should succeed");
    assert_eq!(config.plugins.grants.len(), 1);
    assert_eq!(config.plugins.grants[0].id, "org.anth.test");

    let main = std::fs::read_to_string(&path).unwrap();
    assert!(
        !main.contains("[[plugins.grants]]"),
        "the legacy block must be removed: {main}"
    );
    let grant_path = dir.join("plugins.d").join("org.anth.test.toml");
    let grant = std::fs::read_to_string(&grant_path).expect("the grant file exists");
    assert!(grant.contains("org.anth.test"), "{grant}");

    let reloaded = Config::load(&path).expect("the files load again");
    assert_eq!(reloaded.plugins.grants.len(), 1);
    assert_eq!(reloaded.plugins.grants[0].id, "org.anth.test");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_malformed_grant_file_is_a_load_error() {
    let dir = unique_dir("grant-malformed");
    std::fs::create_dir_all(dir.join("plugins.d")).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "socket_path = \"/tmp/grant.sock\"\n").unwrap();
    std::fs::write(
        dir.join("plugins.d").join("org.anth.bad.toml"),
        "not = toml = at all",
    )
    .unwrap();

    let error = Config::load(&path).unwrap_err();
    assert!(error.contains("org.anth.bad.toml"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_grant_file_name_must_match_its_id() {
    let dir = unique_dir("grant-mismatch");
    std::fs::create_dir_all(dir.join("plugins.d")).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "socket_path = \"/tmp/grant.sock\"\n").unwrap();
    std::fs::write(
        dir.join("plugins.d").join("org.anth.one.toml"),
        r#"
id = "org.anth.two"
version = "0.1.0"
wasm_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
recipes = []
enabled = true
"#,
    )
    .unwrap();

    let error = Config::load(&path).unwrap_err();
    assert!(error.contains("must match"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn grant_files_are_written_with_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = unique_dir("grant-mode");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let mut config = Config::default();
    config
        .plugins
        .grants
        .push(anth_panel::plugins::PluginGrant {
            id: "org.anth.test".to_string(),
            version: "0.1.0".to_string(),
            wasm_sha256: "a".repeat(64),
            recipes: Vec::new(),
            enabled: true,
        });
    config.save(&path).unwrap();

    let grant = dir.join("plugins.d").join("org.anth.test.toml");
    let mode = std::fs::metadata(&grant).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let dir_mode = std::fs::metadata(dir.join("plugins.d"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);

    let _ = std::fs::remove_dir_all(&dir);
}
