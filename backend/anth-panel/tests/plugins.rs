//! Plugin management over HTTP with a temporary bundle root.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config, PluginsConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::plugins::{PluginService, signature};
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
manifest_version = 1
[plugin]
id = "org.anth.hello"
alias = "hello"
version = "0.3.0"
name = "Hello"
abi = 3
[wasm]
path = "plugin.wasm"
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
    let wasm = b"\0asm\x01\0\0\0";
    let manifest = manifest(wasm, capability);
    let signature =
        signed.then(|| signature::encode_hex(&key().sign(manifest.as_bytes()).to_bytes()));
    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut bytes);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("plugin.toml", options).expect("manifest");
        zip.write_all(manifest.as_bytes()).expect("write");
        zip.start_file("plugin.wasm", options).expect("wasm");
        zip.write_all(wasm).expect("write");
        if let Some(signature) = &signature {
            zip.start_file("plugin.sig", options).expect("signature");
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
    config.plugins = PluginsConfig {
        enabled: true,
        root: root.join("plugins").display().to_string(),
        install_from_web,
        require_signature,
        public_key: signature::encode_hex(key().verifying_key().as_bytes()),
        ..PluginsConfig::default()
    };
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let plugins = PluginService::new(
        config.plugins.clone(),
        root.join("plugins"),
        root.join("plugins.d"),
    );
    let daemon =
        Arc::new(Daemon::new(config, paths.clone(), Arc::clone(&logger)).with_plugins(plugins));
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
    let (router, root) = app("plugins-api", true, true);

    let response = router
        .clone()
        .oneshot(post(
            "/api/plugins/install",
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
                .uri("/api/plugins")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("list");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["plugins"][0]["state"], "enabled");
    assert_eq!(body["plugins"][0]["alias"], "hello");

    let response = router
        .clone()
        .oneshot(post("/api/plugins/org.anth.hello/disable", json!({})))
        .await
        .expect("disable");
    assert_eq!(response.status(), StatusCode::OK);
    let response = router
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri("/api/plugins")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("list");
    assert_eq!(body_json(response).await["plugins"][0]["state"], "disabled");

    let response = router
        .clone()
        .oneshot(post("/api/plugins/org.anth.hello/enable", json!({})))
        .await
        .expect("enable");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .oneshot(post("/api/plugins/org.anth.hello/remove", json!({})))
        .await
        .expect("remove");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!root.join("plugins/org.anth.hello").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn unsigned_installs_are_refused_when_the_signature_is_required() {
    let (router, root) = app("plugins-api-unsigned", true, true);
    let response = router
        .oneshot(post(
            "/api/plugins/install",
            install_body("host.hostname", false),
        ))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!root.join("plugins/org.anth.hello").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn web_install_is_refused_when_the_switch_is_off() {
    let (router, root) = app("plugins-api-web-off", true, false);
    let response = router
        .oneshot(post(
            "/api/plugins/install",
            install_body("host.hostname", true),
        ))
        .await
        .expect("install");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn a_changed_surface_needs_an_explicit_approval_route() {
    let (router, root) = app("plugins-api-approve", true, true);
    let response = router
        .clone()
        .oneshot(post(
            "/api/plugins/install",
            install_body("host.hostname", true),
        ))
        .await
        .expect("first");
    assert_eq!(response.status(), StatusCode::OK);

    let mut body = install_body("host.uname", true);
    body["accept_capabilities"] = json!(true);
    let response = router
        .clone()
        .oneshot(post("/api/plugins/install", body))
        .await
        .expect("accepted");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["capabilities"][0], "host.uname");

    let response = router
        .oneshot(post(
            "/api/plugins/org.anth.hello/approve",
            json!({ "accept_capabilities": true }),
        ))
        .await
        .expect("approve");
    assert_eq!(response.status(), StatusCode::OK);
    fs::remove_dir_all(root).expect("cleanup");
}
