//! Web terminal WebSocket tests against a real daemon and a real browser client.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anth_panel::app::AppState;
use anth_panel::config::{Config, TerminalConfig};
use anth_proto::MAX_TERMINAL_INPUT;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type WsClient = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn unique_socket(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "anth-ws-{name}-{}-{nanos}.sock",
        std::process::id()
    ))
}

fn config_at(path: &Path) -> Config {
    Config {
        socket_path: path.to_string_lossy().into_owned(),
        terminal: TerminalConfig {
            shell: "/bin/sh".to_string(),
            max_sessions: 2,
        },
        ..Config::default()
    }
}

async fn start_daemon(config: Config, path: &Path) -> tokio::task::JoinHandle<()> {
    let _ = std::fs::remove_file(path);
    let config_path = path.with_extension("config.toml");
    let handle = tokio::spawn(async move {
        let _ = anth_panel::daemon::serve(&config, &config_path).await;
    });

    for _ in 0..200 {
        if path.exists() {
            return handle;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon did not bind {}", path.display());
}

async fn start_api(config: Config) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = anth_panel::router(AppState::new(config));
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (address, handle)
}

async fn connect(address: SocketAddr, query: &str) -> WsClient {
    let (socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/terminal{query}"))
            .await
            .unwrap();
    socket
}

async fn next_message(socket: &mut WsClient) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out waiting for a WebSocket message")
            .expect("WebSocket closed")
            .expect("WebSocket error");

        match message {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected WebSocket message: {other:?}"),
        }
    }
}

async fn send_json(socket: &mut WsClient, value: &Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn next_error(socket: &mut WsClient) -> Value {
    loop {
        let message = next_message(socket).await;
        match message["type"].as_str() {
            Some("error") => return message,
            Some("output") => continue,
            other => panic!("unexpected frame while waiting for an error: {other:?}"),
        }
    }
}

async fn read_until(socket: &mut WsClient, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seen = String::new();

    while Instant::now() < deadline {
        let message = next_message(socket).await;
        match message["type"].as_str() {
            Some("output") => {
                let bytes = anth_proto::b64::decode(message["data"].as_str().unwrap()).unwrap();
                seen.push_str(&String::from_utf8_lossy(&bytes));
                if seen.contains(needle) {
                    return seen;
                }
            }
            Some("error") => panic!("terminal error: {}", message["detail"]),
            Some("exit") => panic!("terminal exited while waiting for {needle:?}"),
            Some("pong") => {}
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    panic!("never received {needle:?}; output so far: {seen:?}");
}

#[tokio::test]
async fn a_websocket_runs_a_shell_and_resizes_it() {
    let path = unique_socket("session");
    let config = config_at(&path);
    let daemon = start_daemon(config.clone(), &path).await;
    let (address, api) = start_api(config).await;

    let mut socket = connect(address, "?cols=100&rows=30").await;

    let ready = next_message(&mut socket).await;
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["shell"], "/bin/sh");
    assert_eq!(ready["cols"], 100);
    assert_eq!(ready["rows"], 30);
    assert!(ready["line_editing"].is_boolean());

    send_json(
        &mut socket,
        &serde_json::json!({
            "type": "input",
            "data": anth_proto::b64::encode(b"echo WS-MARKER-42\n"),
        }),
    )
    .await;
    read_until(&mut socket, "WS-MARKER-42").await;

    send_json(
        &mut socket,
        &serde_json::json!({
            "type": "resize",
            "cols": 120,
            "rows": 40,
        }),
    )
    .await;
    send_json(
        &mut socket,
        &serde_json::json!({
            "type": "input",
            "data": anth_proto::b64::encode(b"stty size\n"),
        }),
    )
    .await;
    read_until(&mut socket, "40 120").await;

    send_json(&mut socket, &serde_json::json!({ "type": "ping" })).await;
    loop {
        let message = next_message(&mut socket).await;
        match message["type"].as_str() {
            Some("pong") => break,
            Some("output") => {}
            other => panic!("unexpected frame while waiting for pong: {other:?}"),
        }
    }

    let _ = socket.close(None).await;
    api.abort();
    daemon.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn the_default_size_is_80x24() {
    let path = unique_socket("default-size");
    let config = config_at(&path);
    let daemon = start_daemon(config.clone(), &path).await;
    let (address, api) = start_api(config).await;

    let mut socket = connect(address, "").await;
    let ready = next_message(&mut socket).await;

    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["cols"], 80);
    assert_eq!(ready["rows"], 24);
    assert!(ready["line_editing"].is_boolean());

    let _ = socket.close(None).await;
    api.abort();
    daemon.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_daemon_that_is_not_listening_produces_an_error_frame() {
    let path = unique_socket("missing");
    let _ = std::fs::remove_file(&path);
    let (address, api) = start_api(config_at(&path)).await;

    let mut socket = connect(address, "").await;
    let message = next_message(&mut socket).await;

    assert_eq!(message["type"], "error");
    assert!(
        message["detail"]
            .as_str()
            .unwrap()
            .contains("daemon unavailable"),
        "unexpected detail: {message}"
    );

    api.abort();
}

#[tokio::test]
async fn an_invalid_size_is_refused_before_the_upgrade() {
    let path = unique_socket("bad-size");
    let _ = std::fs::remove_file(&path);
    let (address, api) = start_api(config_at(&path)).await;

    let error =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/terminal?cols=0&rows=24"))
            .await
            .unwrap_err();

    match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), 400);
        }
        other => panic!("expected an HTTP rejection, got {other:?}"),
    }

    api.abort();
}

#[tokio::test]
async fn malformed_and_oversized_input_are_refused() {
    let path = unique_socket("limits");
    let config = config_at(&path);
    let daemon = start_daemon(config.clone(), &path).await;
    let (address, api) = start_api(config).await;

    let mut socket = connect(address, "").await;
    assert_eq!(next_message(&mut socket).await["type"], "ready");

    socket
        .send(Message::Text(r#"{"type":"nope"}"#.into()))
        .await
        .unwrap();
    let error = next_error(&mut socket).await;
    assert!(
        error["detail"].as_str().unwrap().contains("invalid client"),
        "unexpected detail: {error}"
    );

    // The bridge closes after a protocol error, so use a fresh socket for the
    // payload limit.
    let mut socket = connect(address, "").await;
    assert_eq!(next_message(&mut socket).await["type"], "ready");

    let oversized = vec![b'a'; MAX_TERMINAL_INPUT + 1];
    send_json(
        &mut socket,
        &serde_json::json!({
            "type": "input",
            "data": anth_proto::b64::encode(&oversized),
        }),
    )
    .await;
    let error = next_error(&mut socket).await;
    assert!(
        error["detail"].as_str().unwrap().contains("over the"),
        "unexpected detail: {error}"
    );

    api.abort();
    daemon.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_second_websocket_gets_its_own_session() {
    let path = unique_socket("two");
    let config = config_at(&path);
    let daemon = start_daemon(config.clone(), &path).await;
    let (address, api) = start_api(config).await;

    let mut first = connect(address, "?cols=80&rows=24").await;
    let mut second = connect(address, "?cols=90&rows=25").await;

    assert_eq!(next_message(&mut first).await["cols"], 80);
    assert_eq!(next_message(&mut second).await["cols"], 90);

    send_json(
        &mut first,
        &serde_json::json!({
            "type": "input",
            "data": anth_proto::b64::encode(b"echo FIRST\n"),
        }),
    )
    .await;
    read_until(&mut first, "FIRST").await;

    send_json(
        &mut second,
        &serde_json::json!({
            "type": "input",
            "data": anth_proto::b64::encode(b"echo SECOND\n"),
        }),
    )
    .await;
    read_until(&mut second, "SECOND").await;

    let _ = first.close(None).await;
    let _ = second.close(None).await;
    api.abort();
    daemon.abort();
    let _ = std::fs::remove_file(&path);
}
