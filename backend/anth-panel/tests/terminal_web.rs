//! Web terminal WebSocket tests against a real router and client.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, BootstrapMode, Config, TerminalConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

type WsClient = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("terminal-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn config(enabled: bool, auth_on: bool) -> Config {
    Config {
        terminal: TerminalConfig {
            enabled,
            shell: "/bin/sh".to_string(),
            max_sessions: 2,
            idle_seconds: 900,
        },
        auth: AuthConfig {
            enabled: auth_on,
            bootstrap: BootstrapMode::Generated,
            ..AuthConfig::default()
        },
        files: anth_panel::config::FilesConfig {
            root: "/tmp".to_string(),
            ..anth_panel::config::FilesConfig::default()
        },
        ..Config::dev_default()
    }
}

fn app(config: Config) -> (axum::Router, PathBuf) {
    let path = scratch("app");
    let config_path = path.join("config.toml");
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Arc::new(Daemon::new(
        config.clone(),
        paths.clone(),
        Arc::clone(&logger),
    ));
    let (auth, _) = AuthService::load(&config.auth, &paths).expect("auth");
    (api::router(AppState::new(daemon, auth, logger)), path)
}

async fn start(config: Config) -> (std::net::SocketAddr, PathBuf, tokio::task::JoinHandle<()>) {
    let (router, path) = app(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (address, path, handle)
}

async fn connect(address: std::net::SocketAddr, query: &str) -> WsClient {
    let (socket, _) =
        tokio_tungstenite::connect_async(format!("ws://{address}/api/terminal{query}"))
            .await
            .expect("connect");
    socket
}

async fn next_message(socket: &mut WsClient) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("timed out")
            .expect("closed")
            .expect("error");
        match message {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected frame: {other:?}"),
        }
    }
}

async fn send_json(socket: &mut WsClient, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn read_until(socket: &mut WsClient, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seen = String::new();
    while Instant::now() < deadline {
        let message = next_message(socket).await;
        match message["type"].as_str() {
            Some("output") => {
                let bytes = STANDARD.decode(message["data"].as_str().unwrap()).unwrap();
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
    panic!("never received {needle:?}: {seen:?}");
}

#[tokio::test]
async fn a_websocket_runs_a_shell_resizes_and_pings() {
    let (address, path, server) = start(config(true, false)).await;
    let mut socket = connect(address, "?cols=100&rows=30").await;

    let ready = next_message(&mut socket).await;
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["shell"], "/bin/sh");
    assert_eq!(ready["cols"], 100);
    assert_eq!(ready["rows"], 30);
    assert!(ready["line_editing"].is_boolean());

    send_json(
        &mut socket,
        json!({ "type": "input", "data": STANDARD.encode(b"echo WS-MARKER-42\n") }),
    )
    .await;
    read_until(&mut socket, "WS-MARKER-42").await;

    send_json(
        &mut socket,
        json!({ "type": "resize", "cols": 120, "rows": 40 }),
    )
    .await;
    send_json(
        &mut socket,
        json!({ "type": "input", "data": STANDARD.encode(b"stty size\n") }),
    )
    .await;
    read_until(&mut socket, "40 120").await;

    send_json(&mut socket, json!({ "type": "ping" })).await;
    loop {
        let message = next_message(&mut socket).await;
        match message["type"].as_str() {
            Some("pong") => break,
            Some("output") => {}
            other => panic!("unexpected frame while waiting for pong: {other:?}"),
        }
    }

    let _ = socket.close(None).await;
    server.abort();
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn the_default_size_is_80x24() {
    let (address, path, server) = start(config(true, false)).await;
    let mut socket = connect(address, "").await;
    let ready = next_message(&mut socket).await;
    assert_eq!(ready["type"], "ready");
    assert_eq!(ready["cols"], 80);
    assert_eq!(ready["rows"], 24);
    let _ = socket.close(None).await;
    server.abort();
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn a_disabled_terminal_answers_with_an_error_frame() {
    let (address, path, server) = start(config(false, false)).await;
    let mut socket = connect(address, "").await;
    let error = next_message(&mut socket).await;
    assert_eq!(error["type"], "error");
    assert!(error["detail"].as_str().unwrap().contains("disabled"));
    let _ = socket.close(None).await;
    server.abort();
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn an_unauthenticated_upgrade_is_refused() {
    let (router, path) = app(config(true, true));
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/terminal")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn a_foreign_origin_is_refused() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let (address, path, server) = start(config(true, false)).await;
    let mut request = format!("ws://{address}/api/terminal")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", "http://evil.example".parse().unwrap());
    let error = tokio_tungstenite::connect_async(request)
        .await
        .expect_err("a foreign origin must be refused");
    assert!(
        format!("{error}").contains("403"),
        "expected 403, got {error}"
    );
    server.abort();
    let _ = std::fs::remove_dir_all(path);
}
