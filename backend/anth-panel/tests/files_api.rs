//! End-to-end checks for the read-only files API.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, BootstrapMode, Config, FilesConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_wire::{FileChunk, FileEntry, Page};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

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

fn app(name: &str, auth_on: bool) -> (axum::Router, PathBuf) {
    let (root, config_path) = scratch(name);
    fs::create_dir(root.join("folder")).expect("folder");
    fs::write(root.join("note.txt"), b"hello files").expect("file");
    let config = Config {
        files: FilesConfig {
            root: root.display().to_string(),
            write: true,
            ..FilesConfig::default()
        },
        ..Config::dev_default()
    };
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Arc::new(Daemon::new(config, paths.clone(), Arc::clone(&logger)));
    let auth_config = if auth_on {
        AuthConfig {
            enabled: true,
            bootstrap: BootstrapMode::Generated,
            ..AuthConfig::default()
        }
    } else {
        AuthConfig {
            enabled: false,
            ..AuthConfig::default()
        }
    };
    let (auth, _) = AuthService::load(&auth_config, &paths).expect("auth");
    let router = api::router(AppState::new(daemon, auth, logger));
    (router, root)
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
async fn lists_the_root_with_directories_and_files() {
    let (router, root) = app("files-list", false);
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files?path=%2F&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::OK);
    let page: Page<FileEntry> = serde_json::from_value(body_json(response).await).expect("page");
    assert!(page.items.iter().any(|entry| entry.name == "folder"));
    assert!(page.items.iter().any(|entry| entry.name == "note.txt"));
    assert!(page.next.is_none());

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn reads_a_text_file_and_refuses_traversal() {
    let (router, root) = app("files-read", false);

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files/read?path=note.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("read");
    assert_eq!(response.status(), StatusCode::OK);
    let chunk: FileChunk = serde_json::from_value(body_json(response).await).expect("chunk");
    assert_eq!(chunk.text, "hello files");
    assert!(chunk.eof);

    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files?path=..%2F")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("traversal");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error = body_json(response).await;
    assert_eq!(error["error"]["code"], "path_escape");

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn deletes_a_file_and_refuses_a_directory() {
    let (router, root) = app("files-delete", false);

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("DELETE")
                .uri("/api/files?path=note.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!root.join("note.txt").exists());

    let response = router
        .oneshot(
            HttpRequest::builder()
                .method("DELETE")
                .uri("/api/files?path=folder")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("delete a directory");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(root.join("folder").exists());

    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn files_require_a_session_when_auth_is_on() {
    let (router, root) = app("files-auth", true);
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/files")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("files");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    fs::remove_dir_all(root).expect("cleanup");
}
