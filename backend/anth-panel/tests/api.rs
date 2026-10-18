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
async fn post_command_forwards_the_request_to_the_daemon() {
    let path = unique_socket("command");
    let _ = std::fs::remove_file(&path);
    let server = spawn_echo_server(&path);

    let state = AppState::new(config_at(&path));
    let result = anth_panel::post_command(
        axum::extract::State(state),
        axum::Json(Request::Echo {
            message: "proxy".to_string(),
        }),
    )
    .await;

    match result.expect("the daemon should answer").0 {
        Response::Ok { message } => assert_eq!(message, "proxy"),
        other => panic!("expected Ok, got {other:?}"),
    }

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn command_route_proxies_and_returns_the_response() {
    let path = unique_socket("route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_echo_server(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let body = serde_json::to_vec(&Request::Echo {
        message: "routed".to_string(),
    })
    .unwrap();

    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/command")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["type"], "Ok");
    assert_eq!(value["data"]["message"], "routed");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn post_command_returns_service_unavailable_when_the_daemon_is_absent() {
    let path = unique_socket("unavailable");
    let _ = std::fs::remove_file(&path);
    let state = AppState::new(config_at(&path));

    let err = anth_panel::post_command(
        axum::extract::State(state),
        axum::Json(Request::Echo {
            message: "nobody".to_string(),
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(err, StatusCode::SERVICE_UNAVAILABLE);
}
