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
    let server = tokio::spawn(async move { anth_panel::daemon::serve(&config).await });

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
    let server = tokio::spawn(async move { anth_panel::daemon::serve(&config).await });

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
