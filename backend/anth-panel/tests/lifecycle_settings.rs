//! End-to-end checks for the Phase 2 lifecycle and settings surface.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config};
use anth_panel::dispatch::Daemon;
use anth_panel::lifecycle::{Actions, Lifecycle};
use anth_panel::log::{Logger, Settings};
use anth_wire::WireError;
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

/// Captures the actions instead of replacing the test binary or the machine.
#[derive(Default)]
struct Recorder {
    restarts: Mutex<usize>,
    reboots: Mutex<Vec<u32>>,
}

impl Actions for Recorder {
    fn restart(&self, _argv: &[OsString]) -> io::Error {
        *self.restarts.lock().unwrap() += 1;
        io::Error::other("recorded")
    }

    fn reboot(&self, delay_s: u32) -> Result<(), WireError> {
        self.reboots.lock().unwrap().push(delay_s);
        Ok(())
    }
}

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

fn app(name: &str, allow_reboot: bool) -> (axum::Router, Arc<Recorder>, PathBuf) {
    let (root, config_path) = scratch(name);
    let config = Config::dev_default();
    let paths = config.paths(&config_path).expect("paths");
    let recorder = Arc::new(Recorder::default());
    let lifecycle = Arc::new(Lifecycle::new(Arc::clone(&recorder), allow_reboot));
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Arc::new(Daemon::with_lifecycle(
        config,
        paths.clone(),
        Arc::clone(&logger),
        lifecycle,
    ));
    let auth_config = AuthConfig {
        enabled: false,
        ..AuthConfig::default()
    };
    let (auth, _) = AuthService::load(&auth_config, &paths).expect("auth");
    let router = api::router(AppState::new(daemon, auth, logger));
    (router, recorder, root)
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

#[tokio::test]
async fn settings_are_readable_and_editable_through_one_key() {
    let (router, _, root) = app("settings", false);

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("settings");
    assert_eq!(response.status(), StatusCode::OK);
    let config = body_json(response).await;
    assert_eq!(config["version"], 1);
    assert_eq!(config["auth"]["username"], "admin");
    assert!(config["socket_path"].is_string());

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("PATCH")
                .uri("/api/settings")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"key":"auth.username","value":"operator"}"#))
                .unwrap(),
        )
        .await
        .expect("patch");
    assert_eq!(response.status(), StatusCode::OK);
    let answer = body_json(response).await;
    assert_eq!(answer["restart_required"], true);

    let text = fs::read_to_string(root.join("config.toml")).expect("config written");
    assert!(text.contains("username = \"operator\""), "{text}");

    let response = router
        .oneshot(
            HttpRequest::builder()
                .method("PATCH")
                .uri("/api/settings")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"key":"nonsense","value":true}"#))
                .unwrap(),
        )
        .await
        .expect("bad key");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn restart_is_answered_before_the_action() {
    let (router, recorder, root) = app("restart", false);
    let response = router
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/restart")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("restart");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let answer = body_json(response).await;
    assert_eq!(answer["restarting"], true);
    assert_eq!(*recorder.restarts.lock().unwrap(), 0);

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(*recorder.restarts.lock().unwrap(), 1);

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn reboot_is_refused_without_the_switch() {
    let (router, recorder, root) = app("reboot-refused", false);
    let response = router
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/reboot")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .expect("reboot");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error = body_json(response).await;
    assert_eq!(error["error"]["kind"], "forbidden");

    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(recorder.reboots.lock().unwrap().is_empty());

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn an_allowed_reboot_answers_before_acting() {
    let (router, recorder, root) = app("reboot-allowed", true);
    let response = router
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/reboot")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"delay_s":5}"#))
                .unwrap(),
        )
        .await
        .expect("reboot");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let answer = body_json(response).await;
    assert_eq!(answer["rebooting"], true);
    assert_eq!(answer["delay_s"], 5);
    assert!(recorder.reboots.lock().unwrap().is_empty());

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(recorder.reboots.lock().unwrap().as_slice(), &[5]);

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn sessions_are_listed_with_the_cap() {
    let (router, _, root) = app("sessions", false);
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/auth/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("sessions");
    assert_eq!(response.status(), StatusCode::OK);
    let answer = body_json(response).await;
    assert_eq!(answer["max"], 10);
    assert_eq!(answer["sessions"].as_array().expect("list").len(), 0);

    fs::remove_dir_all(root).expect("cleanup");
}
