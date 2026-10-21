use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_panel::config::Config;
use anth_panel::terminal::Sessions;
use anth_proto::{AnthClient, Request, Response};
use clap::Parser;
use tokio::net::{UnixListener, UnixStream};

fn unique_socket(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "anth-panel-{name}-{}-{nanos}.sock",
        std::process::id()
    ))
}

#[test]
fn the_served_config_file_is_a_protected_path() {
    let config = Config {
        socket_path: "/var/tmp/anth.sock".to_string(),
        files: anth_panel::config::FilesConfig {
            root: "/srv".to_string(),
            ..Default::default()
        },
        ..Config::default()
    };
    // No ANTH_CONFIG is involved: the daemon is started with `--config`, which
    // is how the shipped container runs, so this path has to come from the flag.
    let policy =
        anth_panel::daemon::policy_for(&config, std::path::Path::new("/etc/anth/custom.toml"));

    assert!(
        policy
            .protected_paths
            .iter()
            .any(|path| path == std::path::Path::new("/etc/anth/custom.toml")),
        "the loaded config must be protected: {:?}",
        policy.protected_paths
    );
}

#[test]
fn handle_request_echoes_the_message() {
    let response = anth_panel::daemon::handle_request(Request::Echo {
        message: "hello".to_string(),
    });

    match response {
        Response::Ok { message } => assert_eq!(message, "hello"),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn handle_request_reports_host_info() {
    match anth_panel::daemon::handle_request(Request::HostInfo) {
        Response::HostInfo(info) => {
            // Assertions stay host-agnostic so the test passes in any container.
            assert!(!info.hostname.is_empty(), "hostname should be collected");
            assert!(!info.os.arch.is_empty(), "arch should be collected");
            assert!(info.logical_cores >= 1, "expected at least one core");
            assert!(
                info.memory.total_kib > 0,
                "expected a non-zero total memory"
            );
            assert!(
                info.memory.available_kib <= info.memory.total_kib,
                "available memory cannot exceed total memory"
            );
        }
        other => panic!("expected HostInfo, got {other:?}"),
    }
}

#[test]
fn handle_request_reports_network_interfaces() {
    match anth_panel::daemon::handle_request(Request::NetworkList) {
        Response::NetworkSnapshot(snapshot) => {
            assert!(snapshot.timestamp_ms > 0, "timestamp should be collected");
            assert!(!snapshot.control_enabled);
        }
        other => panic!("expected NetworkSnapshot, got {other:?}"),
    }
}

#[test]
fn network_state_is_refused_when_control_is_disabled() {
    match anth_panel::daemon::handle_request(Request::NetworkSetState {
        name: "lo".to_string(),
        up: true,
    }) {
        Response::NetworkError(error) => {
            assert_eq!(error.kind, anth_proto::NetworkErrorKind::Disabled);
        }
        other => panic!("expected NetworkError, got {other:?}"),
    }
}

#[test]
fn firewall_changes_are_refused_when_control_is_disabled() {
    match anth_panel::daemon::handle_request(Request::FirewallAddRule {
        action: anth_proto::FirewallAction::Allow,
        port: 22,
        protocol: anth_proto::FirewallProtocol::Tcp,
        comment: None,
    }) {
        Response::FirewallError(error) => {
            assert_eq!(error.kind, anth_proto::FirewallErrorKind::Disabled);
        }
        other => panic!("expected FirewallError, got {other:?}"),
    }

    match anth_panel::daemon::handle_request(Request::FirewallFlush) {
        Response::FirewallError(error) => {
            assert_eq!(error.kind, anth_proto::FirewallErrorKind::Disabled);
        }
        other => panic!("expected FirewallError, got {other:?}"),
    }
}

#[test]
fn handle_line_decodes_and_replies() {
    let line = Request::Echo {
        message: "from-line".to_string(),
    }
    .to_json()
    .unwrap();

    match anth_panel::daemon::handle_line(&line) {
        Response::Ok { message } => assert_eq!(message, "from-line"),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn handle_line_reports_malformed_input_as_an_error() {
    match anth_panel::daemon::handle_line("not json at all") {
        Response::Error { detail } => {
            assert!(detail.contains("Error parsing request"), "got: {detail}");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn clear_socket_path_removes_the_file_and_tolerates_a_missing_one() {
    let path = unique_socket("clear");
    std::fs::write(&path, b"stale").unwrap();
    assert!(path.exists());

    anth_panel::daemon::clear_socket_path(&path).unwrap();
    assert!(!path.exists());

    // Removing it a second time is fine: the path simply does not exist.
    anth_panel::daemon::clear_socket_path(&path).unwrap();
}

#[test]
fn cli_parses_without_a_config_flag() {
    let cli = anth_panel::cli::Cli::try_parse_from(["anth-panel", "daemon"]).unwrap();
    assert_eq!(cli.config_path, None);
}

#[test]
fn cli_parses_a_config_flag() {
    let cli = anth_panel::cli::Cli::try_parse_from([
        "anth-panel",
        "--config",
        "/tmp/anth.toml",
        "daemon",
    ])
    .unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/anth.toml"));

    let cli =
        anth_panel::cli::Cli::try_parse_from(["anth-panel", "-c", "/tmp/short.toml", "daemon"])
            .unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/short.toml"));
}

#[tokio::test]
async fn serve_connection_answers_an_echo_request() {
    let path = unique_socket("connection");
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let sessions = Sessions::new(4);
        anth_panel::daemon::serve_connection(socket, sessions, "/bin/sh".into())
            .await
            .unwrap();
    });

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client
        .send_request(&Request::Echo {
            message: "hi".to_string(),
        })
        .await
        .unwrap();

    match client.receive_response().await.unwrap().unwrap() {
        Response::Ok { message } => assert_eq!(message, "hi"),
        other => panic!("expected Ok, got {other:?}"),
    }

    drop(client);
    server.await.unwrap();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn serve_binds_the_configured_socket_and_echoes() {
    let path = unique_socket("serve");
    let _ = std::fs::remove_file(&path);

    let config = Config {
        socket_path: path.to_string_lossy().into_owned(),
        ..Config::default()
    };
    let config_path = path.with_extension("config.toml");
    let server =
        tokio::spawn(async move { anth_panel::daemon::serve(&config, &config_path).await });

    // Wait briefly for the listener to bind before connecting.
    for _ in 0..50 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client
        .send_request(&Request::Echo {
            message: "served".to_string(),
        })
        .await
        .unwrap();

    match client.receive_response().await.unwrap().unwrap() {
        Response::Ok { message } => assert_eq!(message, "served"),
        other => panic!("expected Ok, got {other:?}"),
    }

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn serve_binds_the_socket_and_answers_host_info() {
    let path = unique_socket("hostinfo-serve");
    let _ = std::fs::remove_file(&path);

    let config = Config {
        socket_path: path.to_string_lossy().into_owned(),
        ..Config::default()
    };
    let config_path = path.with_extension("config.toml");
    let server =
        tokio::spawn(async move { anth_panel::daemon::serve(&config, &config_path).await });

    for _ in 0..50 {
        if path.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let stream = UnixStream::connect(&path).await.unwrap();
    let mut client = AnthClient::new(stream);
    client.send_request(&Request::HostInfo).await.unwrap();

    match client.receive_response().await.unwrap().unwrap() {
        Response::HostInfo(info) => {
            assert!(!info.hostname.is_empty());
            assert!(info.memory.total_kib > 0);
        }
        other => panic!("expected HostInfo, got {other:?}"),
    }

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn handle_request_with_files_lists_the_configured_root() {
    use anth_panel::config::FilesConfig;
    use anth_panel::files::FileService;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("anth-daemon-files-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("hello.txt"), b"hello").unwrap();

    let files = FileService::new(&FilesConfig {
        root: root.to_string_lossy().into_owned(),
        write: false,
        max_entries: 100,
        max_preview_bytes: 1024,
        max_upload_bytes: 1024,
        ..FilesConfig::default()
    })
    .unwrap();

    match anth_panel::daemon::handle_request_with(
        Request::FileList {
            path: "/".to_string(),
        },
        &anth_panel::daemon::Policy::default(),
        &files,
    ) {
        Response::FileListing(listing) => {
            assert_eq!(listing.entries[0].name, "hello.txt");
            assert!(!listing.write_enabled);
        }
        other => panic!("expected FileListing, got {other:?}"),
    }

    match anth_panel::daemon::handle_request_with(
        Request::FileWriteStart {
            path: "/x".to_string(),
            overwrite: false,
        },
        &anth_panel::daemon::Policy::default(),
        &files,
    ) {
        Response::Error { detail } => assert!(detail.contains("open connection")),
        other => panic!("expected Error, got {other:?}"),
    }
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn handle_request_returns_a_volume_snapshot() {
    let response = anth_panel::daemon::handle_request(Request::VolumeList);
    match response {
        Response::VolumeSnapshot(snapshot) => {
            assert!(snapshot.timestamp_ms > 0);
            assert!(!snapshot.control_enabled, "control is off by default");
        }
        other => panic!("expected VolumeSnapshot, got {other:?}"),
    }
}

#[test]
fn volume_mutations_are_refused_by_the_default_policy() {
    let response = anth_panel::daemon::handle_request(Request::VolumeMount {
        device: "/dev/sdz1".to_string(),
        target: "/mnt/data".to_string(),
        fs_type: Some("ext4".to_string()),
        read_only: false,
        allow_removable: false,
    });
    match response {
        Response::VolumeError(error) => {
            assert_eq!(error.kind, anth_proto::VolumeErrorKind::Disabled)
        }
        other => panic!("expected VolumeError, got {other:?}"),
    }

    let response = anth_panel::daemon::handle_request(Request::VolumeUnmount {
        target: "/mnt/data".to_string(),
    });
    match response {
        Response::VolumeError(error) => {
            assert_eq!(error.kind, anth_proto::VolumeErrorKind::Disabled)
        }
        other => panic!("expected VolumeError, got {other:?}"),
    }
}

#[test]
fn volume_requests_survive_the_line_protocol() {
    let response = anth_panel::daemon::handle_line(r#"{"type":"VolumeList"}"#);
    assert!(matches!(response, Response::VolumeSnapshot(_)));
}
