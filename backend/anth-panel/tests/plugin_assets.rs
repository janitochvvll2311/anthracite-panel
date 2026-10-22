//! Plugin web assets: confinement, MIME, and the grant as the gate.
//!
//! These go through the real router, so the route ordering, the auth
//! middleware and the response headers are all exercised rather than a handler
//! called directly. The bundle is written by hand instead of installed from a
//! zip: the interesting cases are all *after* install — a symlink planted in
//! `web/`, a grant that is missing or disabled — and building a zip for each
//! would only add a layer between the test and what it proves.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::app::AppState;
use anth_panel::config::Config;
use anth_panel::plugins::{PluginGrant, manifest::Manifest};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode, header};
use tower::ServiceExt;

fn unique_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "anth-webassets-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A bundle on disk plus the grant the admin would have approved at install.
///
/// The grant is built from a parsed manifest rather than by hand, so a change to
/// `PluginGrant::from_manifest` keeps this fixture honest.
fn bundle(root: &Path, id: &str, web_files: &[(&str, &str)]) -> PluginGrant {
    let dir = root.join(id);
    fs::create_dir_all(dir.join("web/js")).unwrap();
    fs::write(dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").unwrap();

    let manifest = format!(
        r#"
[plugin]
id = "{id}"
version = "0.1.0"
name = "Widget"
abi = 1

[wasm]
path = "plugin.wasm"
sha256 = "{}"

[[ui]]
slot = "dashboard.widget"
tag = "anth-widget"
module = "web/js/widget.js"
"#,
        "0".repeat(64)
    );
    fs::write(dir.join("plugin.toml"), &manifest).unwrap();

    for (name, body) in web_files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
    }

    PluginGrant::from_manifest(&Manifest::parse(&manifest).unwrap())
}

fn state(root: &Path, id: &str, grants: Vec<PluginGrant>) -> AppState {
    let mut config = Config::default();
    config.plugins.enabled = true;
    config.plugins.root = root.to_string_lossy().into_owned();
    config.plugins.grants = grants;
    let _ = id;
    AppState::new(config)
}

/// A configured state for a bundle that is already on disk.
fn state_for(root: &Path) -> AppState {
    let grants = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let text = fs::read_to_string(entry.path().join("plugin.toml")).ok()?;
            Some(PluginGrant::from_manifest(&Manifest::parse(&text).ok()?))
        })
        .collect::<Vec<_>>();
    state(root, "", grants)
}

async fn fetch(state: AppState, uri: &str) -> (StatusCode, Option<String>, String) {
    let response = anth_panel::api::router(state)
        .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap_or_default().to_string());
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 22)
        .await
        .unwrap();
    (
        status,
        content_type,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test]
async fn an_asset_is_served_with_a_fixed_type_and_nosniff() {
    let root = unique_dir("served");
    let grant = bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    let state = state(&root, "org.anth.widget", vec![grant]);

    let (status, content_type, body) =
        fetch(state, "/api/plugins/org.anth.widget/web/js/widget.js").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type.as_deref(),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(body, "export {}");
    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_planted_symlink_cannot_reach_the_manifest() {
    // The install-time member guard refuses a symlink in the zip, so this is
    // about what a local writer can do afterwards: the bundle directory is
    // content on disk, not a sealed archive.
    let root = unique_dir("symlink");
    let grant = bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    std::os::unix::fs::symlink(
        root.join("org.anth.widget/plugin.toml"),
        root.join("org.anth.widget/web/js/leak.toml"),
    )
    .unwrap();
    let state = state(&root, "org.anth.widget", vec![grant]);

    let (status, _, _) = fetch(state, "/api/plugins/org.anth.widget/web/js/leak.toml").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_traversal_in_the_path_is_a_bad_request() {
    let root = unique_dir("traversal");
    let grant = bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    let state = state(&root, "org.anth.widget", vec![grant]);

    // Percent-encoded so the request line keeps the shape a client would send.
    let (status, _, body) = fetch(
        state,
        "/api/plugins/org.anth.widget/web/%2e%2e%2fplugin.toml",
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        !body.contains("[plugin]"),
        "the manifest must not be echoed"
    );
    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn an_encoded_traversal_without_an_encoded_slash_is_still_refused() {
    // The defence is specifically that axum hands the handler a *decoded*
    // capture, so `%2e%2e` arrives as `..` before `relative_asset` sees it.
    // This case isolates that: no `%2f`, only encoded dots, so a router that
    // decoded late would pass `%2e%2e/plugin.toml` through as a plain name.
    let root = unique_dir("encoded");
    let grant = bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    let state = state(&root, "org.anth.widget", vec![grant]);

    let (status, _, body) =
        fetch(state, "/api/plugins/org.anth.widget/web/%2e%2e/plugin.toml").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        !body.contains("[plugin]"),
        "the manifest must not be echoed"
    );

    // The same path with a literal `..` behaves identically.
    let state = state_for(&root);
    let (status, _, _) = fetch(state, "/api/plugins/org.anth.widget/web/../plugin.toml").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn an_unapproved_bundle_serves_nothing() {
    // A directory without a grant is a plugin the admin never approved. Its
    // assets are code for this origin, so the gate is the grant, not the path.
    let root = unique_dir("ungranted");
    bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    let state = state(&root, "org.anth.widget", Vec::new());

    let (status, _, _) = fetch(state, "/api/plugins/org.anth.widget/web/js/widget.js").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_disabled_plugin_serves_no_assets() {
    let root = unique_dir("disabled");
    let mut grant = bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    grant.enabled = false;
    let state = state(&root, "org.anth.widget", vec![grant]);

    let (status, _, _) = fetch(state, "/api/plugins/org.anth.widget/web/js/widget.js").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn the_plugins_switch_turns_assets_off() {
    let root = unique_dir("switch");
    let grant = bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    let state = state(&root, "org.anth.widget", vec![grant]);
    drop(state);
    // The switch is read per request, so it is set before the router is built
    // rather than flipped on a live state.
    let mut config = Config::default();
    config.plugins.enabled = false;
    config.plugins.root = root.to_string_lossy().into_owned();
    config.plugins.grants = Vec::new();
    let state = AppState::new(config);

    let (status, _, _) = fetch(state, "/api/plugins/org.anth.widget/web/js/widget.js").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn an_unknown_asset_is_not_a_directory_listing() {
    let root = unique_dir("missing");
    let grant = bundle(
        &root,
        "org.anth.widget",
        &[("web/js/widget.js", "export {}")],
    );
    let state = state(&root, "org.anth.widget", vec![grant]);

    let (status, _, body) = fetch(state.clone(), "/api/plugins/org.anth.widget/web/js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("widget.js"), "a directory must not list");

    // And the plugin's own id cannot be a directory escape either.
    let (status, _, _) = fetch(state, "/api/plugins/%2e%2e/web/x").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    fs::remove_dir_all(&root).ok();
}
