use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::app::AppState;
use anth_panel::config::Config;
use anth_proto::{AnthServer, Request, Response};
use axum::body::{Body, to_bytes};
use axum::http::{Request as HttpRequest, StatusCode};
use tower::ServiceExt;

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

fn config_at(path: &std::path::Path) -> Config {
    Config {
        socket_path: path.to_string_lossy().into_owned(),
    }
}

/// A one-connection echo server used to stand in for the real daemon.
fn spawn_echo_server(path: &std::path::Path) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(Request::Echo { message })) = server.receive_request().await {
                if server
                    .send_response(&Response::Ok { message })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    })
}

/// A one-connection server that answers every request with `HostInfo`.
fn spawn_host_info_server(
    path: &std::path::Path,
    info: anth_proto::HostInfo,
) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(_)) = server.receive_request().await {
                let response = Response::HostInfo(Box::new(info.clone()));
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    })
}

#[test]
fn app_state_stores_the_configuration() {
    let config = Config {
        socket_path: "/tmp/state.sock".to_string(),
    };
    let state = AppState::new(config.clone());
    assert_eq!(state.config, config);
}

#[test]
fn router_can_be_built() {
    let _app = anth_panel::router(AppState::new(Config::default()));
}

#[tokio::test]
async fn get_healthz_returns_ok() {
    assert_eq!(anth_panel::get_healthz().await, "ok");
}

#[tokio::test]
async fn healthz_route_responds_ok() {
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&body[..], b"ok");
}

#[tokio::test]
async fn create_client_fails_when_the_socket_is_missing() {
    let path = unique_socket("missing");
    let _ = std::fs::remove_file(&path);
    let state = AppState::new(config_at(&path));

    assert!(state.create_client().await.is_err());
}

#[tokio::test]
async fn create_client_connects_to_a_listening_socket() {
    let path = unique_socket("connect");
    let _ = std::fs::remove_file(&path);
    let server = spawn_echo_server(&path);
    let state = AppState::new(config_at(&path));

    let client = state.create_client().await;
    assert!(client.is_ok());

    drop(client);
    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn echo_route_maps_the_body_to_a_message() {
    let path = unique_socket("echo-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_echo_server(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/echo")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"routed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // The route implies the command, so there is no `type`/`data` envelope.
    assert_eq!(value, serde_json::json!({ "message": "routed" }));

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn echo_route_rejects_a_body_without_a_message() {
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/echo")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"type":"Echo","data":{"message":"x"}}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn host_info_route_returns_the_bare_snapshot() {
    let path = unique_socket("host-info-route");
    let _ = std::fs::remove_file(&path);
    let expected = anth_panel::collect_host_info();
    let server = spawn_host_info_server(&path, expected.clone());

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(value["hostname"], expected.hostname);
    assert_eq!(value["logical_cores"], expected.logical_cores);
    assert_eq!(value["os"]["name"], expected.os.name);
    assert_eq!(value["memory"]["total_kib"], expected.memory.total_kib);
    assert!(value.get("type").is_none(), "no tag envelope: {value}");
    assert!(value.get("data").is_none(), "no tag envelope: {value}");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn host_info_route_returns_service_unavailable_without_a_daemon() {
    let path = unique_socket("host-info-down");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn watch_route_rejects_an_invalid_interval() {
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info/watch?interval=abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("unit suffix"));
}

#[tokio::test]
async fn watch_route_rejects_a_busy_loop_interval() {
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info/watch?interval=1ms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn watch_route_returns_service_unavailable_without_a_daemon() {
    let path = unique_socket("watch-down");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info/watch?interval=20ms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
