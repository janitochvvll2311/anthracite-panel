use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::app::AppState;
use anth_panel::config::{Config, FilesConfig};
use anth_panel::files::{FileService, PendingUpload};
use anth_proto::{
    AnthServer, FileError, FileErrorKind, FirewallAction, FirewallError, FirewallErrorKind,
    FirewallProtocol, FirewallRule, FirewallSnapshot, NetworkAddress, NetworkAddressFamily,
    NetworkError, NetworkErrorKind, NetworkInterface, NetworkSnapshot, Request, Response,
};
use axum::body::{Body, Bytes, to_bytes};
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
        ..Config::default()
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

#[tokio::test]
async fn app_state_stores_the_configuration() {
    let config = Config {
        socket_path: "/tmp/state.sock".to_string(),
        ..Config::default()
    };
    let state = AppState::new(config.clone());
    assert_eq!(state.config().await, config);
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value["error"].as_str().unwrap().contains("unit suffix"),
        "{value}"
    );
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

#[tokio::test]
async fn every_failure_uses_the_same_json_shape() {
    // One shape for errors raised by the handler itself...
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info/watch?interval=nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 1, "{value}");
    assert!(value["error"].is_string(), "{value}");

    // ...and one for bodies axum rejects before the handler runs.
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/echo")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"nope":1}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 1, "{value}");
    assert!(value["error"].is_string(), "{value}");
}

#[tokio::test]
async fn a_daemon_error_response_becomes_a_json_gateway_error() {
    let path = unique_socket("daemon-error");
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(_)) = server.receive_request().await {
                let response = Response::Error {
                    detail: "nope".to_string(),
                };
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    });

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

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"], "nope");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_rejected_query_uses_the_unified_error_shape() {
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/host-info/watch?interval=1s&interval=2s")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers()["content-type"],
        "application/json",
        "a query rejected by the extractor must not fall back to text/plain"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["error"].is_string(), "{value}");
}

fn firewall_snapshot() -> FirewallSnapshot {
    FirewallSnapshot {
        initialized: true,
        control_enabled: true,
        rules: vec![FirewallRule {
            handle: 3,
            chain: "input".to_string(),
            action: "allow".to_string(),
            protocol: Some(FirewallProtocol::Tcp),
            port: Some(22),
            comment: Some("ssh".to_string()),
        }],
        ruleset: "table inet anthracite { chain input { ... } }".to_string(),
        total: 1,
        truncated: false,
        warning: None,
    }
}

fn config_with_firewall_control(path: &std::path::Path, control: bool) -> Config {
    let mut config = config_at(path);
    config.firewall.control = control;
    config
}

fn spawn_firewall_server(
    path: &std::path::Path,
    snapshot: FirewallSnapshot,
) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let snapshot = snapshot.clone();
            tokio::spawn(async move {
                let mut server = AnthServer::new(socket);
                while let Some(Ok(request)) = server.receive_request().await {
                    let response = match request {
                        Request::FirewallStatus => Response::FirewallSnapshot(snapshot.clone()),
                        Request::FirewallAddRule {
                            action,
                            port,
                            protocol,
                            ..
                        } => {
                            let verb = if action == FirewallAction::Allow {
                                "allowed"
                            } else {
                                "denied"
                            };
                            Response::FirewallChanged {
                                message: format!("{verb} {protocol}/{port}"),
                            }
                        }
                        Request::FirewallDeleteRule { handle } => Response::FirewallChanged {
                            message: format!("deleted firewall rule {handle}"),
                        },
                        Request::FirewallFlush => Response::FirewallChanged {
                            message: "flushed 1 firewall rule".to_string(),
                        },
                        other => Response::Error {
                            detail: format!("unexpected request: {other:?}"),
                        },
                    };
                    if server.send_response(&response).await.is_err() {
                        break;
                    }
                }
            });
        }
    })
}

fn config_with_process_control(path: &std::path::Path, control: bool) -> Config {
    let mut config = config_at(path);
    config.process.control = control;
    config
}

fn network_snapshot() -> NetworkSnapshot {
    NetworkSnapshot {
        interfaces: vec![NetworkInterface {
            name: "eth0".to_string(),
            index: 2,
            state: "up".to_string(),
            up: true,
            running: true,
            loopback: false,
            mac: "aa:bb:cc:dd:ee:ff".to_string(),
            mtu: 1500,
            speed_mbps: Some(1000),
            duplex: Some("full".to_string()),
            addresses: vec![NetworkAddress {
                family: NetworkAddressFamily::Ipv4,
                address: "192.0.2.10".to_string(),
                prefix_len: 24,
            }],
            rx_bytes: 2048,
            tx_bytes: 1024,
            rx_packets: 10,
            tx_packets: 5,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: 0,
            tx_dropped: 0,
        }],
        timestamp_ms: 1_700_000_000_000,
        control_enabled: true,
        total: 1,
        truncated: false,
        warning: None,
    }
}

fn config_with_network_control(path: &std::path::Path, control: bool) -> Config {
    let mut config = config_at(path);
    config.network.control = control;
    config
}

fn spawn_network_server(
    path: &std::path::Path,
    snapshot: NetworkSnapshot,
) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(request)) = server.receive_request().await {
                let response = match request {
                    Request::NetworkList => Response::NetworkSnapshot(snapshot.clone()),
                    Request::NetworkSetState { name, up } => {
                        Response::NetworkStateChanged { name, up }
                    }
                    other => Response::Error {
                        detail: format!("unexpected request: {other:?}"),
                    },
                };
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    })
}

fn process_snapshot() -> anth_proto::ProcessSnapshot {
    anth_proto::ProcessSnapshot {
        processes: vec![anth_proto::ProcessInfo {
            pid: 42,
            ppid: 1,
            uid: 1000,
            user: "alice".to_string(),
            name: "sleep".to_string(),
            command: "sleep 30".to_string(),
            state: "S".to_string(),
            cpu_ticks: 10,
            rss_kib: 2048,
            threads: 1,
            start_ticks: 12345,
        }],
        clock_ticks: 100,
        timestamp_ms: 1_700_000_000_000,
        control_enabled: true,
        spawn_enabled: true,
        spawn_allow: vec!["/bin/echo".to_string()],
        total: 1,
        truncated: false,
        warning: None,
    }
}

fn spawn_process_server(
    path: &std::path::Path,
    snapshot: anth_proto::ProcessSnapshot,
) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(request)) = server.receive_request().await {
                let response = match request {
                    Request::ProcessList => Response::ProcessSnapshot(snapshot.clone()),
                    Request::ProcessSignal { pid, signal } => {
                        Response::ProcessSignalled { pid, signal }
                    }
                    other => Response::Error {
                        detail: format!("unexpected request: {other:?}"),
                    },
                };
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    })
}

#[tokio::test]
async fn process_route_returns_the_bare_snapshot() {
    let path = unique_socket("process-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_process_server(&path, process_snapshot());

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/processes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["total"], 1);
    assert_eq!(value["processes"][0]["pid"], 42);
    assert_eq!(value["processes"][0]["user"], "alice");
    assert!(value.get("type").is_none(), "no tag envelope: {value}");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn process_route_returns_service_unavailable_without_a_daemon() {
    let path = unique_socket("process-down");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/processes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn process_signal_requires_control_to_be_enabled() {
    let path = unique_socket("process-control-off");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/processes/42/signal")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"signal":"term"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn process_signal_route_maps_the_signal() {
    let path = unique_socket("process-signal-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_process_server(&path, process_snapshot());

    let app = anth_panel::router(AppState::new(config_with_process_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/processes/42/signal")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"signal":"KILL"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value, serde_json::json!({"pid": 42, "signal": "kill"}));

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn process_signal_route_rejects_an_unknown_signal() {
    let path = unique_socket("process-bad-signal");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_with_process_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/processes/42/signal")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"signal":"nope"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn process_signal_rejects_a_non_numeric_pid_as_json() {
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/processes/nope/signal")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"signal":"term"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()["content-type"], "application/json");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["error"].is_string(), "{value}");
}

#[tokio::test]
async fn network_route_returns_the_bare_snapshot() {
    let path = unique_socket("network-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_network_server(&path, network_snapshot());

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["timestamp_ms"], 1_700_000_000_000u64);
    assert_eq!(value["control_enabled"], true);
    assert_eq!(value["total"], 1);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["interfaces"][0]["name"], "eth0");
    assert_eq!(value["interfaces"][0]["addresses"][0]["family"], "ipv4");
    assert!(value.get("type").is_none(), "no tag envelope: {value}");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn network_route_returns_service_unavailable_without_a_daemon() {
    let path = unique_socket("network-down");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/network")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn network_watch_route_rejects_an_invalid_interval() {
    let app = anth_panel::router(AppState::new(Config::default()));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/network/watch?interval=abc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn network_state_requires_control_to_be_enabled() {
    let path = unique_socket("network-control-off");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/network/eth0/state")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"up":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn network_state_route_maps_the_request() {
    let path = unique_socket("network-state-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_network_server(&path, network_snapshot());

    let app = anth_panel::router(AppState::new(config_with_network_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/network/eth0/state")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"up":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value, serde_json::json!({"name":"eth0","up":true}));

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn network_state_route_maps_daemon_errors() {
    let path = unique_socket("network-state-error");
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(_)) = server.receive_request().await {
                let response = Response::NetworkError(NetworkError::new(
                    NetworkErrorKind::Protected,
                    "refusing to bring the loopback interface down",
                ));
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    });

    let app = anth_panel::router(AppState::new(config_with_network_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/network/lo/state")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"up":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["error"].as_str().unwrap().contains("loopback"));

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn firewall_route_returns_the_bare_snapshot() {
    let path = unique_socket("firewall-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_firewall_server(&path, firewall_snapshot());

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/firewall")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["initialized"], true);
    assert_eq!(value["control_enabled"], true);
    assert_eq!(value["rules"][0]["handle"], 3);
    assert_eq!(value["rules"][0]["protocol"], "tcp");
    assert!(value.get("type").is_none(), "no tag envelope: {value}");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn firewall_routes_require_control_to_be_enabled() {
    let path = unique_socket("firewall-control-off");
    let _ = std::fs::remove_file(&path);
    let app = anth_panel::router(AppState::new(config_at(&path)));

    for (uri, body) in [
        ("/api/firewall/rules", r#"{"action":"allow","port":22}"#),
        ("/api/firewall/rules/3/delete", ""),
        ("/api/firewall/flush", ""),
    ] {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
    }
}

#[tokio::test]
async fn firewall_rule_route_maps_the_request() {
    let path = unique_socket("firewall-rule-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_firewall_server(&path, firewall_snapshot());

    let app = anth_panel::router(AppState::new(config_with_firewall_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/firewall/rules")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"action":"deny","port":53,"protocol":"udp","comment":"dns"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["message"], "denied udp/53");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn firewall_delete_and_flush_routes_map_the_request() {
    let path = unique_socket("firewall-delete-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_firewall_server(&path, firewall_snapshot());
    let app = anth_panel::router(AppState::new(config_with_firewall_control(&path, true)));

    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/firewall/rules/7/delete")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/firewall/flush")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn firewall_route_maps_daemon_errors() {
    let path = unique_socket("firewall-error");
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(_)) = server.receive_request().await {
                let response = Response::FirewallError(FirewallError::new(
                    FirewallErrorKind::Unavailable,
                    "nft is not installed",
                ));
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    });

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/firewall")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"], "nft is not installed");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "anth-api-files-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

struct FileFixture {
    root: PathBuf,
    socket: PathBuf,
    config: Config,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for FileFixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = fs::remove_file(&self.socket);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn file_fixture(name: &str, write: bool, max_preview_bytes: u64) -> FileFixture {
    let root = unique_dir(name);
    let socket = unique_socket(name);
    let _ = fs::remove_file(&socket);

    let mut config = config_at(&socket);
    config.files = FilesConfig {
        root: root.to_string_lossy().into_owned(),
        write,
        max_entries: 5000,
        max_preview_bytes,
        max_upload_bytes: 1024 * 1024,
        ..FilesConfig::default()
    };
    let service = FileService::new(&config.files).unwrap();
    let server = spawn_file_server(&socket, service);

    FileFixture {
        root,
        socket,
        config,
        server,
    }
}

fn spawn_file_server(path: &Path, service: FileService) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let service = service.clone();
            tokio::spawn(async move {
                let mut server = AnthServer::new(socket);
                let mut uploads: HashMap<u32, PendingUpload> = HashMap::new();
                let mut next_upload = 1u32;

                while let Some(Ok(request)) = server.receive_request().await {
                    let response = match request {
                        Request::FileWriteStart {
                            path,
                            overwrite,
                            require_existing,
                            expected_modified_ms,
                        } => {
                            match service.start_upload(
                                &path,
                                overwrite,
                                require_existing,
                                expected_modified_ms,
                            ) {
                                Ok(upload) => {
                                    let id = next_upload;
                                    next_upload += 1;
                                    uploads.insert(id, upload);
                                    Response::FileWriteStarted { id }
                                }
                                Err(error) => Response::FileError(error),
                            }
                        }
                        Request::FileWriteChunk { id, bytes } => match uploads.get_mut(&id) {
                            Some(upload) => match upload.write_chunk(&bytes) {
                                Ok(received) => Response::FileWriteAccepted { id, received },
                                Err(error) => {
                                    uploads.remove(&id);
                                    Response::FileError(error)
                                }
                            },
                            None => Response::FileError(FileError::new(
                                FileErrorKind::NotFound,
                                format!("upload {id} is not open"),
                            )),
                        },
                        Request::FileWriteFinish { id } => match uploads.remove(&id) {
                            Some(upload) => match upload.finish() {
                                Ok(written) => Response::FileWritten(written),
                                Err(error) => Response::FileError(error),
                            },
                            None => Response::FileError(FileError::new(
                                FileErrorKind::NotFound,
                                format!("upload {id} is not open"),
                            )),
                        },
                        Request::FileWriteAbort { id } => {
                            uploads.remove(&id);
                            Response::FileWriteAborted { id }
                        }
                        other => anth_panel::daemon::handle_request_with(
                            other,
                            &anth_panel::daemon::Policy::default(),
                            &service,
                        ),
                    };

                    if server.send_response(&response).await.is_err() {
                        break;
                    }
                }
            });
        }
    })
}

fn file_app(fixture: &FileFixture) -> axum::Router {
    anth_panel::router(AppState::new(fixture.config.clone()))
}

#[tokio::test]
async fn files_route_returns_a_directory_listing() {
    let fixture = file_fixture("list-route", false, 1024);
    fs::write(fixture.root.join("hello.txt"), b"hello").unwrap();
    fs::create_dir(fixture.root.join("nested")).unwrap();

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files?path=/")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["path"], "/");
    assert_eq!(value["write_enabled"], false);
    assert_eq!(value["entries"][0]["name"], "hello.txt");
    assert_eq!(value["entries"][1]["kind"], "directory");
}

#[tokio::test]
async fn content_route_truncates_at_max_preview_bytes() {
    let fixture = file_fixture("content-route", false, 4);
    fs::write(fixture.root.join("hello.txt"), b"abcdef").unwrap();

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files/content?path=/hello.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-anth-size"], "6");
    assert_eq!(response.headers()["x-anth-truncated"], "true");
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"abcd");
}

#[tokio::test]
async fn content_route_downgrades_active_documents_to_text() {
    let fixture = file_fixture("content-active", false, 1024);
    fs::write(
        fixture.root.join("evil.html"),
        b"<script>alert('origin')</script>",
    )
    .unwrap();
    fs::write(
        fixture.root.join("evil.svg"),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
    )
    .unwrap();

    for path in ["/evil.html", "/evil.svg"] {
        let response = file_app(&fixture)
            .oneshot(
                HttpRequest::builder()
                    .uri(format!("/api/files/content?path={path}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "text/plain; charset=utf-8"
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            response.headers()["content-disposition"],
            format!("inline; filename=\"{}\"", &path[1..])
        );
    }
}

#[tokio::test]
async fn content_route_keeps_safe_images_inline() {
    let fixture = file_fixture("content-image", false, 1024);
    fs::write(fixture.root.join("pixel.png"), b"not-a-real-png").unwrap();

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files/content?path=/pixel.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[tokio::test]
async fn download_route_streams_the_whole_file() {
    let fixture = file_fixture("download-route", false, 1024);
    fs::write(fixture.root.join("hello.txt"), b"abcdef").unwrap();

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files/download?path=/hello.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"hello.txt\""
    );
    assert_eq!(response.headers()["x-anth-size"], "6");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..], b"abcdef");
}

#[tokio::test]
async fn missing_files_return_not_found() {
    let fixture = file_fixture("missing-route", false, 1024);

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files/content?path=/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["error"].is_string());
}

/// A typo in a route must be a JSON 404, not the SPA's index page: a client
/// that receives HTML with status 200 only finds out when parsing fails.
#[tokio::test]
async fn an_unknown_api_route_returns_a_json_not_found() {
    let app = anth_panel::router(AppState::new(Config::default()));
    for uri in ["/api/does-not-exist", "/api"] {
        let response = app
            .clone()
            .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND, "for {uri}");
        assert_eq!(
            response.headers()["content-type"],
            "application/json",
            "for {uri}"
        );
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value["error"].is_string(), "for {uri}");
    }
}

#[tokio::test]
async fn mkdir_rename_and_delete_routes_mutate_the_root() {
    let fixture = file_fixture("mutate-routes", true, 1024);
    fs::write(fixture.root.join("file.txt"), b"hello").unwrap();
    let app = file_app(&fixture);

    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/files/mkdir")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/nested"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(fixture.root.join("nested").is_dir());

    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/files/rename")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"from":"/file.txt","to":"/nested/moved.txt"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(fixture.root.join("nested/moved.txt").is_file());

    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/files/delete")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/nested","recursive":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!fixture.root.join("nested").exists());
}

#[tokio::test]
async fn write_routes_are_forbidden_when_disabled() {
    let fixture = file_fixture("read-only-routes", false, 1024);
    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/files/mkdir")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"path":"/nested"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn upload_route_writes_the_raw_body() {
    let fixture = file_fixture("upload-route", true, 1024);

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/files/upload?path=/uploaded.bin")
                .header("content-type", "application/octet-stream")
                .body(Body::from(vec![1u8, 2, 3, 4, 5]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"path": "/uploaded.bin", "size": 5})
    );
    assert_eq!(
        fs::read(fixture.root.join("uploaded.bin")).unwrap(),
        [1, 2, 3, 4, 5]
    );
}

#[tokio::test]
async fn path_traversal_is_rejected_by_the_daemon() {
    let fixture = file_fixture("traversal-route", false, 1024);
    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files/content?path=/../etc/passwd")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_route_forwards_multiple_body_frames() {
    let fixture = file_fixture("upload-stream-route", true, 1024);
    let body = Body::from_stream(futures::stream::iter(vec![
        Ok::<_, std::io::Error>(Bytes::from_static(b"hello ")),
        Ok::<_, std::io::Error>(Bytes::from_static(b"world")),
    ]));

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/files/upload?path=/streamed.txt")
                .header("content-type", "application/octet-stream")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        fs::read(fixture.root.join("streamed.txt")).unwrap(),
        b"hello world"
    );
}

#[tokio::test]
async fn upload_route_rejects_a_body_above_the_configured_limit() {
    let mut fixture = file_fixture("upload-limit-route", true, 1024);
    fixture.config.files.max_upload_bytes = 4;

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/files/upload?path=/too-big")
                .header("content-type", "application/octet-stream")
                .body(Body::from(vec![1u8, 2, 3, 4, 5]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn download_route_streams_multiple_chunks() {
    let fixture = file_fixture("download-chunks", false, 1024);
    let content = vec![7u8; 600 * 1024];
    fs::write(fixture.root.join("big.bin"), &content).unwrap();

    let response = file_app(&fixture)
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files/download?path=/big.bin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.len(), content.len());
    assert!(bytes.iter().all(|byte| *byte == 7));
}

fn volume_snapshot() -> anth_proto::VolumeSnapshot {
    anth_proto::VolumeSnapshot {
        devices: vec![anth_proto::VolumeDevice {
            name: "sdb1".to_string(),
            path: "/dev/sdb1".to_string(),
            kind: anth_proto::VolumeKind::Part,
            parent: Some("sdb".to_string()),
            size_bytes: 16_000_000_000,
            fstype: Some("ext4".to_string()),
            label: Some("data".to_string()),
            uuid: None,
            mountpoints: Vec::new(),
            read_only: false,
            removable: false,
            model: None,
            transport: Some("usb".to_string()),
        }],
        mounts: vec![anth_proto::VolumeMount {
            mountpoint: "/".to_string(),
            source: "overlay".to_string(),
            fstype: "overlay".to_string(),
            options: "rw".to_string(),
            read_only: false,
            virtual_fs: true,
            usage: None,
        }],
        control_enabled: true,
        timestamp_ms: 1_700_000_000_000,
        total: 1,
        truncated: false,
        warning: None,
    }
}

fn config_with_volume_control(path: &std::path::Path, control: bool) -> Config {
    let mut config = config_at(path);
    config.volumes.control = control;
    config
}

fn spawn_volume_server(path: &std::path::Path) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(request)) = server.receive_request().await {
                let response = match request {
                    Request::VolumeList => Response::VolumeSnapshot(volume_snapshot()),
                    Request::VolumeMount { device, target, .. } => Response::VolumeChanged {
                        message: format!("mounted {device} on {target}"),
                    },
                    Request::VolumeUnmount { target } => {
                        Response::VolumeError(anth_proto::VolumeError::new(
                            anth_proto::VolumeErrorKind::Busy,
                            format!("{target} is busy"),
                        ))
                    }
                    other => Response::Error {
                        detail: format!("unexpected request: {other:?}"),
                    },
                };
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    })
}

#[tokio::test]
async fn volume_route_returns_the_bare_snapshot() {
    let path = unique_socket("volume-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_volume_server(&path);

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .uri("/api/volumes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["total"], 1);
    assert_eq!(value["devices"][0]["name"], "sdb1");
    assert_eq!(value["devices"][0]["kind"], "part");
    assert_eq!(value["mounts"][0]["fstype"], "overlay");
    assert_eq!(value["mounts"][0]["virtual_fs"], true);
    assert!(value.get("type").is_none(), "no tag envelope: {value}");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn volume_mount_requires_control_to_be_enabled() {
    let path = unique_socket("volume-disabled");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_with_volume_control(&path, false)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/volumes/mount")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"device":"/dev/sdb1","target":"/mnt/data"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"], "volume control is disabled");
}

#[tokio::test]
async fn volume_mount_returns_the_daemon_message() {
    let path = unique_socket("volume-mount");
    let _ = std::fs::remove_file(&path);
    let server = spawn_volume_server(&path);

    let app = anth_panel::router(AppState::new(config_with_volume_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/volumes/mount")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"device":"/dev/sdb1","target":"/mnt/data","read_only":true}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["message"], "mounted /dev/sdb1 on /mnt/data");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn volume_unmount_maps_busy_to_conflict() {
    let path = unique_socket("volume-busy");
    let _ = std::fs::remove_file(&path);
    let server = spawn_volume_server(&path);

    let app = anth_panel::router(AppState::new(config_with_volume_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/volumes/unmount")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"target":"/mnt/data"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"], "/mnt/data is busy");

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn volume_mount_rejects_a_malformed_body() {
    let path = unique_socket("volume-body");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_with_volume_control(&path, true)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/volumes/mount")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"target":"/mnt/data"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

/// Answers every account request with a fixed response, so the router's
/// gating, body parsing and status mapping can be tested without a daemon.
fn spawn_account_server(path: &std::path::Path, response: Response) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(_)) = server.receive_request().await {
                if server.send_response(&response).await.is_err() {
                    break;
                }
            }
        }
    })
}

fn account_config(path: &std::path::Path, control: bool, manage_passwords: bool) -> Config {
    let mut config = config_at(path);
    config.accounts.control = control;
    config.accounts.manage_passwords = manage_passwords;
    config
}

async fn post(app: axum::Router, uri: &str, body: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn account_mutations_are_forbidden_while_control_is_off() {
    let path = unique_socket("accounts-off");
    let _ = std::fs::remove_file(&path);
    let state = AppState::new(account_config(&path, false, false));

    // No daemon is running: a 403 proves the gate answered before the socket.
    let (status, body) = post(
        anth_panel::router(state),
        "/api/accounts/users",
        r#"{"name":"alice"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"].as_str().unwrap().contains("disabled"));
}

#[tokio::test]
async fn account_password_is_gated_by_its_own_switch() {
    let path = unique_socket("accounts-passwd-off");
    let _ = std::fs::remove_file(&path);
    // Control on, passwords off.
    let state = AppState::new(account_config(&path, true, false));

    let (status, body) = post(
        anth_panel::router(state),
        "/api/accounts/users/alice/password",
        r#"{"password":"correct-horse"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["error"].as_str().unwrap().contains("password"));
}

#[tokio::test]
async fn account_create_returns_the_daemon_message() {
    let path = unique_socket("accounts-create");
    let _ = std::fs::remove_file(&path);
    let server = spawn_account_server(
        &path,
        Response::AccountChanged {
            message: "created user alice".to_string(),
        },
    );
    let state = AppState::new(account_config(&path, true, false));

    let (status, body) = post(
        anth_panel::router(state),
        "/api/accounts/users",
        r#"{"name":"alice","gecos":"Alice","system":false}"#,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "created user alice");
    server.abort();
}

#[tokio::test]
async fn account_errors_map_onto_status_codes() {
    for (kind, expected) in [
        (
            anth_proto::AccountErrorKind::Invalid,
            StatusCode::BAD_REQUEST,
        ),
        (
            anth_proto::AccountErrorKind::Protected,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::AccountErrorKind::Disabled,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::AccountErrorKind::PermissionDenied,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::AccountErrorKind::NotFound,
            StatusCode::NOT_FOUND,
        ),
        (anth_proto::AccountErrorKind::Conflict, StatusCode::CONFLICT),
        (
            anth_proto::AccountErrorKind::Unsupported,
            StatusCode::NOT_IMPLEMENTED,
        ),
        (
            anth_proto::AccountErrorKind::Failed,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ] {
        let path = unique_socket("accounts-err");
        let _ = std::fs::remove_file(&path);
        let server = spawn_account_server(
            &path,
            Response::AccountError(anth_proto::AccountError::new(kind, "refused")),
        );
        let state = AppState::new(account_config(&path, true, true));

        let (status, body) = post(
            anth_panel::router(state),
            "/api/accounts/groups",
            r#"{"name":"developers"}"#,
        )
        .await;

        assert_eq!(status, expected, "kind {kind:?}");
        assert_eq!(body["error"], "refused");
        server.abort();
    }
}

#[tokio::test]
async fn account_delete_accepts_an_empty_body() {
    let path = unique_socket("accounts-delete-empty");
    let _ = std::fs::remove_file(&path);
    let server = spawn_account_server(
        &path,
        Response::AccountChanged {
            message: "deleted user alice".to_string(),
        },
    );
    let state = AppState::new(account_config(&path, true, false));

    // No content-type and no body: `userdel` is the common case and a client
    // should not have to send `{}`.
    let response = anth_panel::router(state)
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/accounts/users/alice/delete")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    server.abort();
}

#[tokio::test]
async fn a_malformed_account_body_is_a_bad_request() {
    let path = unique_socket("accounts-bad-body");
    let _ = std::fs::remove_file(&path);
    let state = AppState::new(account_config(&path, true, false));

    let (status, _) = post(anth_panel::router(state), "/api/accounts/users", "not json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

fn config_with_process_spawn(path: &std::path::Path, allow: &[&str]) -> Config {
    let mut config = config_at(path);
    config.process.spawn = true;
    config.process.spawn_allow = allow.iter().map(|entry| entry.to_string()).collect();
    config
}

/// A one-connection server that answers `ProcessSpawn` with `response`.
fn spawn_process_spawn_server<F>(path: &std::path::Path, answer: F) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Response + Send + 'static,
{
    let listener = tokio::net::UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            let mut server = AnthServer::new(socket);
            while let Some(Ok(request)) = server.receive_request().await {
                let answer = match request {
                    Request::ProcessSpawn { .. } => answer(),
                    other => Response::Error {
                        detail: format!("unexpected request: {other:?}"),
                    },
                };
                if server.send_response(&answer).await.is_err() {
                    break;
                }
            }
        }
    })
}

#[tokio::test]
async fn process_spawn_route_returns_the_started_process() {
    let path = unique_socket("process-spawn-route");
    let _ = std::fs::remove_file(&path);
    let server = spawn_process_spawn_server(&path, || {
        Response::ProcessStarted(anth_proto::ProcessSpawned {
            pid: 4242,
            program: "/bin/echo".to_string(),
            args: vec!["hi".to_string()],
        })
    });

    let app = anth_panel::router(AppState::new(config_with_process_spawn(
        &path,
        &["/bin/echo"],
    )));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/processes/spawn")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"program":"/bin/echo","args":["hi"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"pid": 4242, "program": "/bin/echo", "args": ["hi"]})
    );

    server.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn process_spawn_maps_each_refusal_to_a_usable_status() {
    // A client has to tell "you may not" from "there is no such program", so
    // the kind decides the status rather than every refusal collapsing to 400.
    let cases = [
        (
            anth_proto::ProcessSpawnErrorKind::Disabled,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::ProcessSpawnErrorKind::NotAllowed,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::ProcessSpawnErrorKind::NotAbsolute,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::ProcessSpawnErrorKind::NotExecutable,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::ProcessSpawnErrorKind::PermissionDenied,
            StatusCode::FORBIDDEN,
        ),
        (
            anth_proto::ProcessSpawnErrorKind::NotFound,
            StatusCode::NOT_FOUND,
        ),
        (
            anth_proto::ProcessSpawnErrorKind::Failed,
            StatusCode::BAD_REQUEST,
        ),
    ];

    for (index, (kind, expected)) in cases.into_iter().enumerate() {
        let path = unique_socket(&format!("process-spawn-status-{index}"));
        let _ = std::fs::remove_file(&path);
        let server = spawn_process_spawn_server(&path, move || {
            Response::ProcessSpawnError(anth_proto::ProcessSpawnError {
                kind,
                detail: format!("refused: {kind:?}"),
            })
        });

        let app = anth_panel::router(AppState::new(config_with_process_spawn(
            &path,
            &["/bin/echo"],
        )));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/processes/spawn")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"program":"/bin/echo"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), expected, "kind {kind:?}");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value["error"].as_str().unwrap().contains("refused"));

        server.abort();
        let _ = std::fs::remove_file(&path);
    }
}

#[tokio::test]
async fn process_spawn_rejects_a_malformed_body_as_json() {
    let path = unique_socket("process-spawn-bad-body");
    let _ = std::fs::remove_file(&path);

    let app = anth_panel::router(AppState::new(config_with_process_spawn(
        &path,
        &["/bin/echo"],
    )));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/processes/spawn")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"args":["hi"]}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["error"].is_string());
}

#[tokio::test]
async fn process_snapshot_exposes_the_spawn_switch_and_allowlist() {
    let path = unique_socket("process-spawn-snapshot");
    let _ = std::fs::remove_file(&path);
    let server = spawn_process_server(&path, process_snapshot());

    let app = anth_panel::router(AppState::new(config_at(&path)));
    let response = app
        .oneshot(
            HttpRequest::builder()
                .method("GET")
                .uri("/api/processes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["spawn_enabled"], serde_json::json!(true));
    assert_eq!(value["spawn_allow"], serde_json::json!(["/bin/echo"]));

    server.abort();
    let _ = std::fs::remove_file(&path);
}
