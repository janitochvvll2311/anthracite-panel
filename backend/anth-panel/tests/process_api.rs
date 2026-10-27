//! End-to-end checks for the process API.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_wire::{PageRequest, Request, Response};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

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

fn app(name: &str) -> (axum::Router, Arc<Daemon>) {
    let (_root, config_path) = scratch(name);
    let config = Config::dev_default();
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Arc::new(Daemon::new(config, paths.clone(), Arc::clone(&logger)));
    let auth = AuthConfig {
        enabled: false,
        ..AuthConfig::default()
    };
    let (auth, _) = AuthService::load(&auth, &paths).expect("auth");
    let router = api::router(AppState::new(Arc::clone(&daemon), auth, logger));
    (router, daemon)
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json")
}

fn post(uri: &str, body: serde_json::Value) -> HttpRequest<Body> {
    HttpRequest::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

#[tokio::test]
async fn the_process_table_is_paginated_and_reports_the_policy() {
    let (router, _daemon) = app("process-list");
    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/processes?limit=3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::OK);
    let page = body_json(response).await;
    let items = page["items"].as_array().expect("items");
    assert!(items.len() <= 3);
    assert!(page["total"].as_u64().unwrap_or(0) > 0);
    assert_eq!(page["control_enabled"], true);
    assert_eq!(page["spawn_enabled"], true);
    assert!(page["clock_ticks"].as_u64().unwrap_or(0) > 0);
    assert!(!items.is_empty());
    assert!(items[0]["pid"].is_number());
    assert!(items[0]["start_ticks"].is_number());

    let bad = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/processes?limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("bad limit");
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_watch_route_streams_a_processes_event() {
    let (router, _daemon) = app("process-watch");
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/processes/watch?interval=1s&limit=2")
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
    assert!(text.contains("event: processes"), "{text}");
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("a data line");
    let snapshot: serde_json::Value = serde_json::from_str(data).expect("json data");
    assert!(snapshot["items"].as_array().is_some());
    assert!(snapshot["timestamp_ms"].as_u64().unwrap_or(0) > 0);
}

#[tokio::test]
async fn signals_are_checked_before_they_reach_the_kernel() {
    let (router, _daemon) = app("process-signal");
    let invalid = router
        .clone()
        .oneshot(post(
            "/api/processes/signal",
            json!({ "pid": 2, "start_ticks": 1, "signal": "explode" }),
        ))
        .await
        .expect("invalid signal");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let protected = router
        .clone()
        .oneshot(post(
            "/api/processes/signal",
            json!({ "pid": 1, "start_ticks": 1, "signal": "term" }),
        ))
        .await
        .expect("protected");
    assert_eq!(protected.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(protected).await["error"]["code"],
        "protected_process"
    );

    let own = std::process::id();
    let stale = router
        .clone()
        .oneshot(post(
            "/api/processes/signal",
            json!({ "pid": own, "start_ticks": 1, "signal": "term" }),
        ))
        .await
        .expect("own pid");
    assert_eq!(stale.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn spawn_refuses_programs_outside_the_allowlist() {
    let (router, _daemon) = app("process-spawn-denied");
    let response = router
        .clone()
        .oneshot(post(
            "/api/processes/spawn",
            json!({ "program": "/bin/sh", "args": ["-c", "id"] }),
        ))
        .await
        .expect("spawn");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "program_not_allowed"
    );
}

#[tokio::test]
async fn spawn_starts_and_signal_stops_an_allowlisted_program() {
    let (router, daemon) = app("process-spawn-signal");
    let response = router
        .clone()
        .oneshot(post(
            "/api/processes/spawn",
            json!({ "program": "/bin/sleep", "args": ["30"] }),
        ))
        .await
        .expect("spawn");
    assert_eq!(response.status(), StatusCode::OK);
    let started = body_json(response).await;
    let pid = started["pid"].as_u64().expect("pid") as u32;
    assert_eq!(started["program"], "/bin/sleep");

    let process = find_process(&daemon, pid).await;

    let response = router
        .clone()
        .oneshot(post(
            "/api/processes/signal",
            json!({ "pid": pid, "start_ticks": process.start_ticks, "signal": "kill" }),
        ))
        .await
        .expect("signal");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["pid"], pid);

    let deadline = Instant::now() + Duration::from_secs(5);
    while std::path::Path::new(&format!("/proc/{pid}")).exists() {
        assert!(Instant::now() < deadline, "the process did not exit");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // A cursor is opaque; a non-numeric one is rejected by the page contract.
    let bad = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/processes?cursor=nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("bad cursor");
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_dispatch_arm_pages_the_snapshot() {
    let (_router, daemon) = app("process-dispatch");
    let first = match daemon
        .execute(Request::Processes {
            limit: Some(1),
            cursor: None,
        })
        .await
    {
        Response::Processes(snapshot) => snapshot,
        other => panic!("unexpected snapshot: {other:?}"),
    };
    assert_eq!(first.page.items.len(), 1);
    let total = first.page.total.expect("total");
    if total > 1 {
        let next = first.page.next.expect("next");
        let second = match daemon
            .execute(Request::Processes {
                limit: Some(1),
                cursor: Some(next),
            })
            .await
        {
            Response::Processes(snapshot) => snapshot,
            other => panic!("unexpected snapshot: {other:?}"),
        };
        assert_eq!(second.page.items.len(), 1);
        assert_ne!(second.page.items[0].pid, first.page.items[0].pid);
    }
    // The page contract is shared with every other list.
    assert!(
        PageRequest {
            limit: Some(1),
            cursor: None,
        }
        .valid()
    );
}
async fn find_process(daemon: &Daemon, pid: u32) -> anth_wire::ProcessInfo {
    let mut cursor = None;
    loop {
        let snapshot = match daemon
            .execute(Request::Processes {
                limit: Some(1000),
                cursor,
            })
            .await
        {
            Response::Processes(snapshot) => snapshot,
            other => panic!("unexpected snapshot: {other:?}"),
        };
        let next = snapshot.page.next.clone();
        if let Some(process) = snapshot
            .page
            .items
            .iter()
            .find(|process| process.pid == pid)
        {
            return process.clone();
        }
        match next {
            Some(value) => cursor = Some(value),
            None => panic!("the spawned process is not in the table"),
        }
    }
}
