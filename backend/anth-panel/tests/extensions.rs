//! Extension management over HTTP with a temporary bundle root.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_extension::signature;
use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config, ExtensionsConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::extensions::ExtensionService;
use anth_panel::log::{Logger, Settings};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("scratch");
    root
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

fn manifest(wasm: &[u8], capability: &str) -> String {
    format!(
        r#"
manifest_version = 2
[extension]
id = "org.anth.hello"
alias = "hello"
version = "0.3.0"
name = "Hello"
abi = 3
[runtime]
kind = "wasm"

[wasm]
path = "extension.wasm"
sha256 = "{}"

[[capability]]
id = "{capability}"
program = "hostname"

[[api]]
method = "GET"
path = "/status"
"#,
        signature::encode_hex(&sha2_bytes(wasm))
    )
}

fn sha2_bytes(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).into()
}

fn bundle(capability: &str, signed: bool) -> Vec<u8> {
    bundle_with_wasm(b"\0asm\x01\0\0\0", capability, signed)
}

fn bundle_with_wasm(wasm: &[u8], capability: &str, signed: bool) -> Vec<u8> {
    let manifest = manifest(wasm, capability);
    let signature =
        signed.then(|| signature::encode_hex(&key().sign(manifest.as_bytes()).to_bytes()));
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("extension.toml", options).expect("manifest");
        zip.write_all(manifest.as_bytes()).expect("write");
        zip.start_file("extension.wasm", options).expect("wasm");
        zip.write_all(wasm).expect("write");
        if let Some(signature) = &signature {
            zip.start_file("extension.sig", options).expect("signature");
            zip.write_all(signature.as_bytes()).expect("write");
        }
        zip.finish().expect("finish");
    }
    bytes.into_inner()
}

fn app(name: &str, require_signature: bool, install_from_web: bool) -> (axum::Router, PathBuf) {
    let (root, config_path) = {
        let root = scratch(name);
        let config_path = root.join("config.toml");
        (root, config_path)
    };
    let mut config = Config::dev_default();
    config.extensions = ExtensionsConfig {
        enabled: true,
        root: root.join("extensions").display().to_string(),
        install_from_web,
        require_signature,
        public_key: signature::encode_hex(key().verifying_key().as_bytes()),
        ..ExtensionsConfig::default()
    };
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let extensions = ExtensionService::new(
        config.extensions.clone(),
        root.join("extensions"),
        root.join("extensions.d"),
    );
    let daemon =
        Arc::new(Daemon::new(config, paths.clone(), Arc::clone(&logger)).with_plugins(extensions));
    let auth_config = AuthConfig {
        enabled: false,
        ..AuthConfig::default()
    };
    let (auth, _) = AuthService::load(&auth_config, &paths).expect("auth");
    (api::router(AppState::new(daemon, auth, logger)), root)
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
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

fn install_body(capability: &str, signed: bool) -> serde_json::Value {
    json!({
        "bundle_base64": STANDARD.encode(bundle(capability, signed)),
        "accept_capabilities": false,
    })
}

#[tokio::test]
async fn installs_lists_toggles_and_removes_a_signed_bundle() {
    let (router, root) = app("extensions-api", true, true);

    let response = router
        .clone()
        .oneshot(post(
            "/api/extensions/install",
            install_body("host.hostname", true),
        ))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["id"], "org.anth.hello");
    assert_eq!(body["capabilities"][0], "host.hostname");

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/extensions")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["extensions"][0]["state"], "enabled");
    assert_eq!(body["extensions"][0]["alias"], "hello");

    let response = router
        .clone()
        .oneshot(post("/api/extensions/org.anth.hello/disable", json!({})))
        .await
        .expect("disable");
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/extensions")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("list");
    assert_eq!(
        body_json(response).await["extensions"][0]["state"],
        "disabled"
    );

    let response = router
        .clone()
        .oneshot(post("/api/extensions/org.anth.hello/enable", json!({})))
        .await
        .expect("enable");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .oneshot(post("/api/extensions/org.anth.hello/remove", json!({})))
        .await
        .expect("remove");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!root.join("extensions/org.anth.hello").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

fn bundle_with_asset(asset: &[u8]) -> Vec<u8> {
    let wasm = b"\0asm\x01\0\0\0";
    let mut manifest = manifest(wasm, "host.hostname");
    manifest.push_str(&format!(
        r#"
[[ui]]
slot = "settings.extension"
tag = "anth-hello"
module = "web/hello.js"
sha256 = "{}"
"#,
        signature::encode_hex(&sha2_bytes(asset))
    ));
    let signature = signature::encode_hex(&key().sign(manifest.as_bytes()).to_bytes());
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("extension.toml", options).expect("manifest");
        zip.write_all(manifest.as_bytes()).expect("write");
        zip.start_file("extension.wasm", options).expect("wasm");
        zip.write_all(wasm).expect("write");
        zip.start_file("extension.sig", options).expect("signature");
        zip.write_all(signature.as_bytes()).expect("write");
        zip.start_file("web/hello.js", options).expect("asset");
        zip.write_all(asset).expect("write");
        zip.finish().expect("finish");
    }
    bytes.into_inner()
}

#[tokio::test]
async fn a_declared_ui_asset_is_served_and_an_undeclared_one_is_not() {
    let (router, root) = app("extensions-api-asset", true, true);
    let body = json!({
        "bundle_base64": STANDARD.encode(bundle_with_asset(b"export const hello = 1\n")),
        "accept_capabilities": false,
    });
    let response = router
        .clone()
        .oneshot(post("/api/extensions/install", body))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/extensions/org.anth.hello/web/hello.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("asset");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/javascript; charset=utf-8"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    assert_eq!(bytes.as_ref(), b"export const hello = 1\n");

    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/extensions/org.anth.hello/web/other.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("asset");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn the_frame_bootstrap_inlines_a_declared_module() {
    let (router, root) = app("extensions-api-frame", true, true);
    let body = json!({
        "bundle_base64": STANDARD.encode(bundle_with_asset(
            b"export function mount(root) { root.textContent = 'hello' }\n",
        )),
        "accept_capabilities": false,
    });
    let response = router
        .clone()
        .oneshot(post("/api/extensions/install", body))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/e/org.anth.hello/hello.js")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("frame");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/html; charset=utf-8"
    );
    let csp = response
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(csp.contains("script-src 'nonce-"), "{csp}");
    assert!(csp.contains("frame-ancestors 'self'"), "{csp}");
    let html = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let html = String::from_utf8(html.to_vec()).expect("utf8");
    assert!(html.contains("export function mount"), "{html}");
    assert!(html.contains("sandbox") || html.contains("URL.createObjectURL"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn an_anchor_runs_the_installed_guest() {
    const GUEST: &str = r#"
(module
  (memory (export "memory") 1)
  (data (i32.const 256) "{\"status\":200,\"body\":{\"hello\":\"world\"}}")
  (func (export "anth_abi_version") (result i32) i32.const 3)
  (func (export "anth_alloc") (param i32) (result i32) i32.const 4096)
  (func (export "anth_http") (param i32 i32) (result i64)
    i64.const 1099511627815)
  (func (export "anth_cli") (param i32 i32) (result i64)
    i64.const 1099511627815)
  (func (export "anth_direct") (param i32 i32) (result i64)
    i64.const 1099511627815)
)
"#;
    let wasm = wat::parse_str(GUEST).expect("wat");
    let (router, root) = app("extensions-api-anchor", true, true);
    let body = json!({
        "bundle_base64": STANDARD.encode(bundle_with_wasm(&wasm, "host.hostname", true)),
        "accept_capabilities": false,
    });
    let response = router
        .clone()
        .oneshot(post("/api/extensions/install", body))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/extensions/org.anth.hello/status?x=1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("anchor");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["hello"], "world");

    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/extensions/org.anth.hello/other")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("anchor");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn unsigned_installs_are_refused_when_the_signature_is_required() {
    let (router, root) = app("extensions-api-unsigned", true, true);
    let response = router
        .oneshot(post(
            "/api/extensions/install",
            install_body("host.hostname", false),
        ))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!root.join("extensions/org.anth.hello").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn web_install_is_refused_when_the_switch_is_off() {
    let (router, root) = app("extensions-api-web-off", true, false);
    let response = router
        .oneshot(post(
            "/api/extensions/install",
            install_body("host.hostname", true),
        ))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn a_changed_surface_needs_an_explicit_approval_route() {
    let (router, root) = app("extensions-api-approve", true, true);
    let response = router
        .clone()
        .oneshot(post(
            "/api/extensions/install",
            install_body("host.hostname", true),
        ))
        .await
        .expect("first");
    assert_eq!(response.status(), StatusCode::OK);

    let mut body = install_body("host.uname", true);
    body["accept_capabilities"] = json!(true);
    let response = router
        .clone()
        .oneshot(post("/api/extensions/install", body))
        .await
        .expect("accepted");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["capabilities"][0], "host.uname");

    let response = router
        .oneshot(post(
            "/api/extensions/org.anth.hello/approve",
            json!({ "accept_capabilities": true }),
        ))
        .await
        .expect("approve");
    assert_eq!(response.status(), StatusCode::OK);
    fs::remove_dir_all(root).expect("cleanup");
}
