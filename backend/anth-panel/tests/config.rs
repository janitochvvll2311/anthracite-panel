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

    let config = Config::load_or_create(&path);

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

    let config = Config::load_or_create(&path);

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

    let first = Config::load_or_create(&path);
    let second = Config::load_or_create(&path);

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

    let config = Config::load_or_create(&path);

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

    let config = Config::load_or_create(&path);

    assert_eq!(without_auth(&config), custom);
    assert_eq!(
        Config::load(&path).unwrap().auth.password_hash,
        config.auth.password_hash
    );
    assert_eq!(Config::load(&path).unwrap().socket_path, "/tmp/custom.sock");

    let _ = std::fs::remove_dir_all(&dir);
}

/// An unreadable or malformed file is replaced by a freshly written default,
/// matching what the pre-refactor `anthd` did: the error branch always saved.
#[test]
fn load_or_create_replaces_a_malformed_file_with_defaults() {
    let dir = unique_dir("malformed");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "not = = toml\n").unwrap();

    let config = Config::load_or_create(&path);

    assert_eq!(
        without_auth(&config),
        without_auth(&Config::server_default())
    );
    let written = Config::load(&path).unwrap();
    assert_eq!(
        without_auth(&written),
        without_auth(&Config::server_default())
    );
    assert_eq!(
        written.auth.password_hash, config.auth.password_hash,
        "the file must hold the account the server is using"
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
