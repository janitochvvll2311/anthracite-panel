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
