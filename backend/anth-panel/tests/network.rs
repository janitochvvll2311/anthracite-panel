//! Network inventory over the service, the socket and the HTTP API.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config, NetworkConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::network::SystemNetwork;
use anth_panel::socket;
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod common;

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("create scratch");
    (root.clone(), root.join("config.toml"))
}

fn daemon(name: &str) -> (Arc<Daemon>, anth_panel::config::Paths) {
    let (_root, config_path) = scratch(name);
    let config = Config::dev_default();
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    (Arc::new(Daemon::new(config, paths.clone(), logger)), paths)
}

fn state(daemon: Arc<Daemon>) -> AppState {
    let (_root, config_path) = scratch("network-state");
    let config = Config::dev_default();
    let paths = config.paths(&config_path).expect("paths");
    let auth = AuthConfig {
        enabled: false,
        ..AuthConfig::default()
    };
    let (auth, _) = AuthService::load(&auth, &paths).expect("auth");
    AppState::new(
        daemon,
        auth,
        Arc::new(Logger::buffer(Settings::default()).0),
    )
}

fn check_snapshot(snapshot: &anth_wire::NetworkSnapshot) {
    assert!(snapshot.timestamp_ms > 0);
    assert!(!snapshot.interfaces.is_empty());

    let mut indexes: Vec<u32> = snapshot
        .interfaces
        .iter()
        .map(|interface| interface.index)
        .collect();
    indexes.sort_unstable();
    indexes.dedup();
    assert_eq!(
        indexes.len(),
        snapshot.interfaces.len(),
        "indexes are unique"
    );

    let loopback = snapshot
        .interfaces
        .iter()
        .find(|interface| interface.name == "lo")
        .expect("the loopback interface is listed");
    assert!(loopback.loopback);
    assert!(loopback.up);
    assert_eq!(loopback.kind, "loopback");
    assert!(loopback.mac.is_empty());
    assert!(
        loopback
            .addresses
            .iter()
            .any(|address| address.address == "127.0.0.1" || address.address == "::1"),
        "the loopback interface has a loopback address"
    );

    for interface in &snapshot.interfaces {
        assert!(!interface.name.is_empty());
        assert!(!interface.state.is_empty());
        assert!(interface.mtu > 0, "{} has an MTU", interface.name);
        for address in &interface.addresses {
            assert!(matches!(address.family.as_str(), "inet" | "inet6"));
            assert!(matches!(address.scope.as_str(), "global" | "link" | "host"));
            let limit = if address.family == "inet" { 32 } else { 128 };
            assert!(address.prefix <= limit);
        }
    }

    for route in &snapshot.routes {
        assert!(matches!(route.family.as_str(), "inet" | "inet6"));
        assert!(!route.gateway.is_empty());
        assert!(
            snapshot
                .interfaces
                .iter()
                .any(|interface| interface.name == route.interface),
            "route {} uses a listed interface",
            route.interface
        );
    }
}

#[test]
fn a_snapshot_reports_interfaces_addresses_counters_and_policy() {
    let service = SystemNetwork::new(true, false);
    let snapshot = service.snapshot().expect("snapshot");
    check_snapshot(&snapshot);
    assert!(snapshot.control_enabled);
    assert!(!snapshot.manage_enabled);

    let later = service.snapshot().expect("second snapshot");
    for interface in &snapshot.interfaces {
        let Some(next) = later
            .interfaces
            .iter()
            .find(|candidate| candidate.name == interface.name)
        else {
            continue;
        };
        assert!(next.rx_bytes >= interface.rx_bytes);
        assert!(next.tx_bytes >= interface.tx_bytes);
    }
}

#[tokio::test]
async fn the_http_route_returns_the_snapshot() {
    let (daemon, _paths) = daemon("network-http");
    let router = api::router(state(daemon));
    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("network");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert!(
        body["interfaces"]
            .as_array()
            .is_some_and(|list| !list.is_empty())
    );
    assert_eq!(body["control_enabled"], true);
    assert_eq!(body["manage_enabled"], true);
    assert!(body["timestamp_ms"].as_u64().unwrap_or(0) > 0);
}

#[tokio::test]
async fn the_watch_route_streams_a_network_event() {
    let (daemon, _paths) = daemon("network-watch");
    let router = api::router(state(daemon));
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/network/watch?interval=1s")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("watch");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .starts_with("text/event-stream")
    );

    let mut body = response.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
        .await
        .expect("a frame arrives")
        .expect("the body has a frame")
        .expect("the frame is not an error");
    let text = String::from_utf8_lossy(&frame.into_data().unwrap_or_default()).into_owned();
    assert!(text.contains("event: network"), "{text}");
    assert!(text.contains("\"interfaces\""), "{text}");
}

#[tokio::test]
async fn the_socket_answers_a_network_request() {
    let (daemon, paths) = daemon("network-socket");
    let listener = socket::bind(&paths.socket).expect("bind");
    let router = common::local_router(Arc::clone(&daemon), &paths);
    let server = tokio::spawn(socket::serve(listener, router));

    let client = anth_client::Client::unix(&paths.socket);
    let snapshot = client.network().await.expect("network");
    check_snapshot(&snapshot);
    server.abort();
}

#[test]
fn the_policy_flags_mirror_the_configuration() {
    let service = SystemNetwork::new(false, true);
    let snapshot = service.snapshot().expect("snapshot");
    assert!(!snapshot.control_enabled);
    assert!(snapshot.manage_enabled);

    let config = Config {
        network: NetworkConfig {
            control: false,
            manage: false,
        },
        ..Config::dev_default()
    };
    let daemon = Daemon::new(
        config,
        Config::dev_default()
            .paths("/tmp/anthracite-panel/tests/network-policy/config.toml")
            .expect("paths"),
        Arc::new(Logger::buffer(Settings::default()).0),
    );
    assert!(!daemon.config().network.control);
    assert!(!daemon.config().network.manage);
}
