use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_conf::Config;
use anth_proto::{AnthClient, Request, Response};
use clap::Parser;
use tokio::net::{UnixListener, UnixStream};

fn unique_socket(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("anthd-{name}-{}-{nanos}.sock", std::process::id()))
}

#[test]
fn handle_request_echoes_the_message() {
    let response = anthd::handle_request(Request::Echo {
        message: "hello".to_string(),
    });

    match response {
        Response::Ok { message } => assert_eq!(message, "hello"),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn handle_line_decodes_and_replies() {
    let line = Request::Echo {
        message: "from-line".to_string(),
    }
    .to_json()
    .unwrap();

    match anthd::handle_line(&line) {
        Response::Ok { message } => assert_eq!(message, "from-line"),
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn handle_line_reports_malformed_input_as_an_error() {
    match anthd::handle_line("not json at all") {
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

    anthd::clear_socket_path(&path).unwrap();
    assert!(!path.exists());

    // Removing it a second time is fine: the path simply does not exist.
    anthd::clear_socket_path(&path).unwrap();
}

#[test]
fn cli_parses_without_a_config_flag() {
    let cli = anthd::cli::Cli::try_parse_from(["anthd"]).unwrap();
    assert_eq!(cli.config_path, None);
}

#[test]
fn cli_parses_a_config_flag() {
    let cli = anthd::cli::Cli::try_parse_from(["anthd", "--config", "/tmp/anth.toml"]).unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/anth.toml"));

    let cli = anthd::cli::Cli::try_parse_from(["anthd", "-c", "/tmp/short.toml"]).unwrap();
    assert_eq!(cli.config_path.as_deref(), Some("/tmp/short.toml"));
}

#[tokio::test]
async fn serve_connection_answers_an_echo_request() {
    let path = unique_socket("connection");
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        anthd::serve_connection(socket).await.unwrap();
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
    };
    let server = tokio::spawn(async move { anthd::serve(&config).await });

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
