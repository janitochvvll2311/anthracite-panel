//! End-to-end checks for the Phase 1 host-info slice.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod common;

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("create scratch");
    (root.clone(), root.join("config.toml"))
}

fn daemon(
    config: Config,
    config_path: &std::path::Path,
) -> (Arc<Daemon>, anth_panel::config::Paths) {
    let paths = config.paths(config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    (Arc::new(Daemon::new(config, paths.clone(), logger)), paths)
}

#[tokio::test]
async fn the_socket_handshake_and_host_info_round_trip() {
    let (root, config_path) = scratch("host-info-socket");
    let (daemon, paths) = daemon(Config::dev_default(), &config_path);
    let listener = socket::bind(&paths.socket).expect("bind");
    let router = common::local_router(Arc::clone(&daemon), &paths);
    let server = tokio::spawn(socket::serve(listener, router));

    let client = anth_client::Client::unix(&paths.socket);
    assert_eq!(client.hello().await.expect("hello").protocol_version, 4);
    let info = client.host_info().await.expect("host-info");
    assert!(!info.hostname.is_empty());
    assert!(info.collected_at_ms > 0);

    server.abort();
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn http_host_info_and_auth_state_are_reachable_with_auth_off() {
    let (root, config_path) = scratch("host-info-http");
    let config = Config::dev_default();
    let (daemon, paths) = daemon(config.clone(), &config_path);
    let auth_config = AuthConfig {
        enabled: false,
        ..AuthConfig::default()
    };
    let (auth, _) = AuthService::load(&auth_config, &paths).expect("auth");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let router = api::router(AppState::new(daemon, auth, logger));

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("health");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-anth-protocol")
            .and_then(|value| value.to_str().ok()),
        Some("4")
    );

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("host-info");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let info: anth_wire::HostInfo = serde_json::from_slice(&body).expect("host-info json");
    assert!(!info.hostname.is_empty());

    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/auth/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("state");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let state: serde_json::Value = serde_json::from_slice(&body).expect("state json");
    assert_eq!(state["required"], false);
    assert_eq!(state["registered"], false);
    assert_eq!(state["authenticated"], true);

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn host_info_requires_a_session_when_auth_is_on() {
    let (root, config_path) = scratch("host-info-protected");
    let config = Config::dev_default();
    let (daemon, paths) = daemon(config, &config_path);
    let auth_config = AuthConfig {
        enabled: true,
        bootstrap: anth_panel::config::BootstrapMode::Generated,
        ..AuthConfig::default()
    };
    let (auth, password) = AuthService::load(&auth_config, &paths).expect("auth");
    assert!(password.is_some());
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let router = api::router(AppState::new(daemon, auth, logger));

    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("host-info");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body).expect("error json");
    assert_eq!(error["error"]["code"], "auth_required");

    fs::remove_dir_all(root).expect("cleanup");
}
