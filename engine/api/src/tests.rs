use crate::*;
use axum::body::Body;
use axum::http::header::IF_MATCH;
use axum::http::Request;
use futures_core::Stream;
use std::task::{Context, Poll};
use tower::ServiceExt;

fn state_with(uid: u32) -> Arc<EngineState> {
    Arc::new(EngineState::new(uid))
}

fn state() -> Arc<EngineState> {
    state_with(1000)
}

fn session_state() -> (Arc<EngineState>, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
        "anth-api-session-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let auth = Auth::load_or_create(&dir, std::time::Duration::from_secs(60)).expect("credentials");
    (
        Arc::new(EngineState::new(1000).with_auth(Arc::new(auth))),
        dir,
    )
}

fn credentials(dir: &std::path::Path) -> (String, String) {
    let bytes = std::fs::read(dir.join(anth_auth::CREDENTIALS)).expect("read");
    let value: Value = serde_json::from_slice(&bytes).expect("json");
    (
        value["operator"].as_str().expect("operator").to_string(),
        value["password"].as_str().expect("password").to_string(),
    )
}

async fn session_call(
    state: Arc<EngineState>,
    method: &str,
    uri: &str,
    body: Value,
    cookie: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(serde_json::to_vec(&body).expect("json"))
    };
    let mut request = builder.body(body).expect("request");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4000))));
    let response = public_router(state)
        .oneshot(request)
        .await
        .expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, headers, value)
}

fn cookie(headers: &axum::http::HeaderMap) -> String {
    headers["set-cookie"]
        .to_str()
        .expect("cookie")
        .split(';')
        .next()
        .expect("cookie pair")
        .to_string()
}

async fn trusted_call(
    state: Arc<EngineState>,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(serde_json::to_vec(&body).expect("json"))
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .expect("request");
    let response = api_router(state)
        .layer(Extension(Trusted))
        .oneshot(request)
        .await
        .expect("response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// The uid this test process runs as. CI does not run as root, so a fixed
/// number here would pass locally and fail there.
fn current_euid() -> u32 {
    // SAFETY: `geteuid` has no preconditions.
    unsafe { libc::geteuid() }
}

fn request(uri: &str, peer: Option<u32>) -> Request<Body> {
    let mut request = Request::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("request");
    if let Some(uid) = peer {
        request.extensions_mut().insert(ConnectInfo(PeerUid(uid)));
    }
    request
}

async fn status(uri: &str, peer: Option<u32>) -> StatusCode {
    router(state())
        .oneshot(request(uri, peer))
        .await
        .expect("response")
        .status()
}

fn apps_state() -> (Arc<EngineState>, std::path::PathBuf) {
    apps_state_with(&[])
}

/// A state tree with `notes` installed and a composition naming `composed`.
fn apps_state_with(composed: &[&str]) -> (Arc<EngineState>, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
        "anth-api-apps-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let app = dir.join("apps/org.example.notes");
    let wit = app.join("wit");
    let web = app.join("web");
    std::fs::create_dir_all(&wit).expect("mkdir");
    std::fs::create_dir_all(&web).expect("mkdir");
    for path in [&dir, &dir.join("apps"), &app, &wit, &web] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    }
    let manifest = "manifest_version = 2\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n\n[wasm]\npath = \"wasm/test.wasm\"\nsha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\n[[export]]\ninterface = \"example:notes/thing@1.0.0\"\n\n[[route]]\nmethod = \"GET\"\npath = \"/notes\"\ncall = \"example:notes/thing@1.0.0#ping\"\n\n[[route]]\nmethod = \"GET\"\npath = \"/stream\"\ncall = \"example:notes/thing@1.0.0#follow\"\nstream = true\n\n[[slot]]\nid = \"panel.view\"\nlabel = \"Notes\"\norder = 1\ntag = \"notes\"\nmodule = \"web/notes.js\"\nsha256 = \"a87f13cb0f53003ce287621f67b588959fe569bcc8864527feb741b8bf5d6c71\"\n\n[[cli]]\ncall = \"example:notes/thing@1.0.0#ping\"\nverb = \"list\"\n";
    let descriptor = "version = 1\napp = \"org.example.notes\"\nenabled = true\ntier = \"third-party\"\ninstalled_at = \"2026-11-03T20:00:00Z\"\n";
    let wit_source =
            "package example:notes@1.0.0;\ninterface thing { ping: func(); follow: func() -> stream<list<u8>>; }\nworld app { export thing; }\n";
    let ui_source =
        "export function mount(root) { root.textContent = \"notes\"; return () => {}; }\n";
    for (path, text) in [
        (app.join("manifest.toml"), manifest),
        (app.join("descriptor.toml"), descriptor),
        (wit.join("notes.wit"), wit_source),
        (web.join("notes.js"), ui_source),
    ] {
        std::fs::write(&path, text).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }
    (
        Arc::new(
            EngineState::new(1000)
                .with_composition(Composition {
                    version: 1,
                    profile: vec![anth_contract::Profile {
                        id: "default".to_string(),
                        apps: composed.iter().map(|app| app.to_string()).collect(),
                    }],
                })
                .with_apps(&dir, 1 << 20, false, false),
        ),
        dir,
    )
}

/// The public half of the test signer: the verifying key for seed `[7; 32]`.
fn public_key_hex() -> String {
    "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c".to_string()
}

fn write_grant(dir: &std::path::Path, text: &str) {
    write_grant_for(dir, "org.example.notes", text);
}

fn write_grant_for(dir: &std::path::Path, app: &str, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    let grants = dir.join("grants");
    std::fs::create_dir_all(&grants).expect("grants");
    std::fs::set_permissions(&grants, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let path = grants.join(format!("{app}.toml"));
    std::fs::write(&path, text).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
}

fn grant_toml(dir: &std::path::Path) -> String {
    let app = AppId::parse("org.example.notes").expect("id");
    let material = Registry::new(dir)
        .approval_material(&app)
        .expect("material")
        .expect("installed");
    format!(
            "version = 1\napp = \"org.example.notes\"\napp_version = \"0.1.0\"\nmanifest_sha256 = \"{}\"\nwit_sha256 = \"{}\"\nrisk = \"read\"\napproved_by = \"operator-3f2a\"\napproved_at = \"2026-11-03T20:00:00Z\"\nexport = [\"example:notes/thing@1.0.0\"]\n",
            material.manifest_sha256, material.wit_sha256
        )
}

/// A second installed app that imports what `notes` exports.
fn add_dependent(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let app = dir.join("apps/org.example.consumer");
    let wit = app.join("wit");
    std::fs::create_dir_all(&wit).expect("mkdir");
    for path in [&app, &wit] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    }
    let manifest = "manifest_version = 2\n\n[app]\nid = \"org.example.consumer\"\nnamespace = \"example:consumer\"\nname = \"Consumer\"\nversion = \"0.1.0\"\n\n[[import]]\ninterface = \"example:notes/thing@1.0.0\"\n";
    let descriptor = "version = 1\napp = \"org.example.consumer\"\nenabled = true\ntier = \"third-party\"\ninstalled_at = \"2026-11-03T20:00:00Z\"\n";
    let wit_source = "package example:consumer@1.0.0;\ninterface thing { ping: func(); }\nworld app { import example:notes/thing@1.0.0; }\n";
    for (path, text) in [
        (app.join("manifest.toml"), manifest),
        (app.join("descriptor.toml"), descriptor),
        (wit.join("consumer.wit"), wit_source),
    ] {
        std::fs::write(&path, text).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }
}

/// A minimal valid bundle: one export, one world, one component.
fn bundle(id: &str, namespace: &str, version: &str, wasm: &[u8]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    let hash: String = Sha256::digest(wasm)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let manifest = format!(
            "manifest_version = 2\n\n[app]\nid = \"{id}\"\nnamespace = \"{namespace}\"\nname = \"Test\"\nversion = \"{version}\"\n\n[wasm]\npath = \"wasm/test.wasm\"\nsha256 = \"{hash}\"\n\n[[export]]\ninterface = \"{namespace}/thing@1.0.0\"\n"
        );
    let wit = format!(
            "package {namespace}@1.0.0;\ninterface thing {{ ping: func(); }}\nworld app {{ export thing; }}\n"
        );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in [
        ("manifest.toml", manifest.into_bytes()),
        ("wit/test.wit", wit.into_bytes()),
        ("wasm/test.wasm", wasm.to_vec()),
    ] {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .expect("start");
        writer.write_all(&bytes).expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

async fn call(
    state: Arc<EngineState>,
    method: &str,
    uri: &str,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    call_with(state, method, uri, body, &[]).await
}

async fn call_with(
    state: Arc<EngineState>,
    method: &str,
    uri: &str,
    body: Vec<u8>,
    extra: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    let mut request = builder.body(Body::from(body)).expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, headers, value)
}

async fn json(state: Arc<EngineState>, uri: &str) -> (StatusCode, Value) {
    let response = router(state)
        .oneshot(request(uri, Some(1000)))
        .await
        .expect("response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn the_app_catalogue_lists_installed_apps() {
    let (state, dir) = apps_state();
    let (status, value) = json(state, "/api/engine/apps").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["apps"][0]["id"], "org.example.notes");
    assert_eq!(value["apps"][0]["alias"], "notes");
    assert_eq!(value["apps"][0]["tier"], "third-party");
    assert!(
        value["apps"][0].get("grant").is_none(),
        "the list carries no grant"
    );
    assert_eq!(value["apps"][0]["grant_state"], "none");
    assert!(value["problems"].as_array().expect("problems").is_empty());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_key_is_added_listed_and_removed() {
    let (state, dir) = apps_state();
    let body = format!(
        r#"{{"public_key":"{}","label":"example signer"}}"#,
        public_key_hex()
    );
    let (status, headers, value) = call(
        state.clone(),
        "POST",
        "/api/engine/keys",
        body.clone().into_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    let fingerprint = value["fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_string();
    assert_eq!(fingerprint.len(), 64);
    assert_eq!(value["label"], "example signer");
    assert_eq!(value["added_by"], "uid:1000");
    assert_eq!(value["algorithm"], "ed25519");
    assert_eq!(
        headers
            .get("location")
            .and_then(|value| value.to_str().ok()),
        Some(format!("/api/engine/keys/{fingerprint}").as_str())
    );
    let (status, value) = json(state.clone(), "/api/engine/keys").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["keys"][0]["fingerprint"], fingerprint);
    // The same key is idempotent: it keeps the first label and adds no
    // second audit entry.
    let retry = format!(r#"{{"public_key":"{}"}}"#, public_key_hex());
    let (status, _, value) = call(
        state.clone(),
        "POST",
        "/api/engine/keys",
        retry.into_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["label"], "example signer");
    let (status, _, _) = call(
        state.clone(),
        "DELETE",
        &format!("/api/engine/keys/{fingerprint}"),
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, value) = json(state.clone(), "/api/engine/keys").await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["keys"].as_array().expect("keys").is_empty());
    let (status, _, value) = call(
        state,
        "DELETE",
        &format!("/api/engine/keys/{fingerprint}"),
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not-found");
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.len(), 2, "a retried add is not a second action");
    assert_eq!(entries[0].action, AuditAction::KeyAdd);
    assert_eq!(entries.last().unwrap().action, AuditAction::KeyRemove);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_key_that_is_not_32_bytes_is_refused() {
    let (state, dir) = apps_state();
    let (status, _, value) = call(
        state.clone(),
        "POST",
        "/api/engine/keys",
        br#"{"public_key":"nope"}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid");
    let (status, value) = json(state, "/api/engine/keys").await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["keys"].as_array().expect("keys").is_empty());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_manifest_route_calls_the_app() {
    struct Echo;
    impl AppCaller for Echo {
        fn call<'a>(
            &'a self,
            app: &'a str,
            interface: &'a str,
            function: &'a str,
            input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move {
                Ok(json!({
                    "app": app,
                    "interface": interface,
                    "function": function,
                    "input": input,
                }))
            })
        }
    }

    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Echo)),
    );
    let (status, headers, value) = call(
        state.clone(),
        "GET",
        "/api/apps/org.example.notes/notes?q=a&q=b&single=1&list[]=only",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["app"], "org.example.notes");
    assert_eq!(value["interface"], "example:notes/thing@1.0.0");
    assert_eq!(value["function"], "ping");
    assert_eq!(value["input"]["q"], json!(["a", "b"]));
    assert_eq!(value["input"]["single"], "1");
    assert_eq!(value["input"]["list"], json!(["only"]));
    let _ = headers;
    let (status, headers, _) = call(
        state.clone(),
        "OPTIONS",
        "/api/apps/org.example.notes/notes",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let allow = headers
        .get("allow")
        .and_then(|value| value.to_str().ok())
        .expect("allow");
    assert!(allow.contains("GET"), "{allow}");
    assert!(allow.contains("OPTIONS"), "{allow}");
    let (status, _, _) = call(
        state.clone(),
        "POST",
        "/api/apps/org.example.notes/notes",
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_stream_route_returns_chunked_bytes() {
    struct Streamer;
    impl AppCaller for Streamer {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Ok(Value::Null) })
        }

        fn stream<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> StreamFuture<'a> {
            Box::pin(async move {
                let chunks = vec![
                    Ok::<Chunk, Error>(b"one".to_vec()),
                    Ok(b"-two".to_vec()),
                    Ok(b"-three".to_vec()),
                ];
                Ok(Box::pin(futures_util::stream::iter(chunks)) as ChunkStream)
            })
        }
    }

    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Streamer)),
    );
    let mut request = Request::builder()
        .method("GET")
        .uri("/api/apps/org.example.notes/stream")
        .body(Body::empty())
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/octet-stream")
    );
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    assert_eq!(&body[..], b"one-two-three");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_stream_error_before_the_first_byte_is_the_json_envelope() {
    struct Refusing;
    impl AppCaller for Refusing {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Ok(Value::Null) })
        }

        fn stream<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> StreamFuture<'a> {
            Box::pin(async move { Err(Error::denied("not for you")) })
        }
    }

    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Refusing)),
    );
    let (status, _, value) = call(
        state,
        "GET",
        "/api/apps/org.example.notes/stream",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(value["error"]["code"], "denied");
    assert_eq!(value["error"]["message"], "not for you");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_stream_error_after_the_first_chunk_truncates_the_body() {
    struct Breaking;
    impl AppCaller for Breaking {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Ok(Value::Null) })
        }

        fn stream<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> StreamFuture<'a> {
            Box::pin(async move {
                let chunks = vec![
                    Ok::<Chunk, Error>(b"head".to_vec()),
                    Err(Error::internal("the producer died")),
                ];
                Ok(Box::pin(futures_util::stream::iter(chunks)) as ChunkStream)
            })
        }
    }

    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Breaking)),
    );
    let mut request = Request::builder()
        .method("GET")
        .uri("/api/apps/org.example.notes/stream")
        .body(Body::empty())
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let error = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect_err("the body is truncated");
    assert!(error.to_string().contains("producer died"), "{error}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_caller_that_cannot_stream_answers_unavailable() {
    struct Buffered;
    impl AppCaller for Buffered {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Ok(Value::Null) })
        }
    }

    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Buffered)),
    );
    let (status, _, value) = call(
        state,
        "GET",
        "/api/apps/org.example.notes/stream",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(value["error"]["code"], "unavailable");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

fn set_limits(dir: &std::path::Path, limits: &str) {
    let path = dir.join("apps/org.example.notes/manifest.toml");
    let manifest = std::fs::read_to_string(&path).expect("manifest");
    std::fs::write(&path, format!("{manifest}\n[limits]\n{limits}\n")).expect("write");
}

struct Emit {
    chunks: Vec<Vec<u8>>,
}

impl AppCaller for Emit {
    fn call<'a>(
        &'a self,
        _app: &'a str,
        _interface: &'a str,
        _function: &'a str,
        _input: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
        Box::pin(async move { Ok(Value::Null) })
    }

    fn stream<'a>(
        &'a self,
        _app: &'a str,
        _interface: &'a str,
        _function: &'a str,
        _input: Value,
    ) -> StreamFuture<'a> {
        let chunks = self.chunks.clone();
        Box::pin(async move {
            let items: Vec<Result<Chunk, Error>> = chunks.into_iter().map(Ok).collect();
            Ok(Box::pin(futures_util::stream::iter(items)) as ChunkStream)
        })
    }
}

async fn stream_body(state: Arc<EngineState>) -> Result<Vec<u8>, axum::Error> {
    let mut request = Request::builder()
        .method("GET")
        .uri("/api/apps/org.example.notes/stream")
        .body(Body::empty())
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .map(|bytes| bytes.to_vec())
}

#[tokio::test]
async fn a_stream_over_its_total_budget_is_truncated() {
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    set_limits(&dir, "bytes-total = 100\nbytes-per-sec = 1000000");
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Emit {
                chunks: vec![vec![b'a'; 60], vec![b'b'; 60]],
            })),
    );
    let error = stream_body(state).await.expect_err("over budget");
    assert!(error.to_string().contains("byte budget"), "{error}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_stream_over_its_rate_budget_is_truncated() {
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    set_limits(&dir, "bytes-total = 1000000\nbytes-per-sec = 50");
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Emit {
                chunks: vec![vec![b'a'; 60]],
            })),
    );
    let error = stream_body(state).await.expect_err("over rate");
    assert!(error.to_string().contains("byte/s budget"), "{error}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

struct Tracked {
    dropped: Arc<std::sync::atomic::AtomicBool>,
    sent: bool,
}

impl Stream for Tracked {
    type Item = Result<Chunk, Error>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if !self.sent {
            self.sent = true;
            return Poll::Ready(Some(Ok(b"first".to_vec())));
        }
        Poll::Pending
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[tokio::test]
async fn dropping_the_response_body_drops_the_stream() {
    use http_body_util::BodyExt;

    struct Endless {
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }

    impl AppCaller for Endless {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Ok(Value::Null) })
        }

        fn stream<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> StreamFuture<'a> {
            let dropped = Arc::clone(&self.dropped);
            Box::pin(async move {
                Ok(Box::pin(Tracked {
                    dropped,
                    sent: false,
                }) as ChunkStream)
            })
        }
    }

    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Endless {
                dropped: Arc::clone(&dropped),
            })),
    );
    let mut request = Request::builder()
        .method("GET")
        .uri("/api/apps/org.example.notes/stream")
        .body(Body::empty())
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    let mut body = response.into_body();
    let frame = body.frame().await.expect("frame").expect("data");
    assert!(frame.data_ref().is_some());
    assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
    drop(body);
    assert!(
        dropped.load(std::sync::atomic::Ordering::SeqCst),
        "the caller stream is dropped with the response body"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_route_for_a_degraded_app_is_unavailable() {
    struct Never;
    impl AppCaller for Never {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Err(Error::internal("should not be called")) })
        }
    }
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Never)),
    );
    let (status, _, value) = call(
        state.clone(),
        "GET",
        "/api/apps/org.example.notes/notes",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(value["error"]["code"], "unavailable");
    assert!(value["error"]["message"]
        .as_str()
        .expect("message")
        .contains("not running"));
    // OPTIONS belongs to the engine and does not need a running instance.
    let (status, headers, _) = call(
        state,
        "OPTIONS",
        "/api/apps/org.example.notes/notes",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(headers
        .get("allow")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|allow| allow.contains("GET")));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_route_that_is_not_declared_is_not_found() {
    struct Never;
    impl AppCaller for Never {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Err(Error::internal("should not be called")) })
        }
    }
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Never)),
    );
    let (status, _, value) =
        call(state, "GET", "/api/apps/org.example.notes/nope", Vec::new()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not-found");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_undeclared_method_is_method_not_allowed() {
    struct Never;
    impl AppCaller for Never {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Err(Error::internal("should not be called")) })
        }
    }
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Never)),
    );
    let (status, headers, value) = call(
        state,
        "POST",
        "/api/apps/org.example.notes/notes",
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(value["error"]["code"], "method-not-allowed");
    let allow = headers
        .get("allow")
        .and_then(|value| value.to_str().ok())
        .expect("allow");
    assert!(allow.contains("GET"), "{allow}");
    assert!(allow.contains("OPTIONS"), "{allow}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_body_on_a_get_route_is_refused() {
    struct Never;
    impl AppCaller for Never {
        fn call<'a>(
            &'a self,
            _app: &'a str,
            _interface: &'a str,
            _function: &'a str,
            _input: Value,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>> {
            Box::pin(async move { Err(Error::internal("should not be called")) })
        }
    }
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let state = Arc::new(
        Arc::try_unwrap(state)
            .ok()
            .expect("unique state")
            .with_caller(Arc::new(Never)),
    );
    let (status, _, value) = call(
        state,
        "GET",
        "/api/apps/org.example.notes/notes",
        br#"{}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_slot_module_is_hash_pinned_and_immutable() {
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let (status, headers, value) = call(
        state.clone(),
        "GET",
        "/api/apps/org.example.notes/assets/web/notes.js",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value, Value::Null);
    assert!(headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/javascript")));
    assert_eq!(
        headers
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("immutable")
    );
    let etag = headers
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .expect("etag")
        .to_string();
    let (status, _, _) = call_with(
        state.clone(),
        "GET",
        "/api/apps/org.example.notes/assets/web/notes.js",
        Vec::new(),
        &[("if-none-match", &etag)],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    let (status, headers, _) = call(
        state.clone(),
        "HEAD",
        "/api/apps/org.example.notes/assets/web/notes.js",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("etag").is_some());
    let (status, _, _) = call(
        state,
        "GET",
        "/api/apps/org.example.notes/assets/web/nope.js",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_tampered_asset_is_refused_without_the_host_path() {
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    std::fs::write(dir.join("apps/org.example.notes/web/notes.js"), b"tampered").expect("write");
    let (status, _, value) = call(
        state,
        "GET",
        "/api/apps/org.example.notes/assets/web/notes.js",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let message = value["error"]["message"].as_str().expect("message");
    assert!(!message.contains(&dir.display().to_string()), "{message}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_asset_of_a_degraded_app_is_unavailable() {
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    let (status, _, value) = call(
        state,
        "GET",
        "/api/apps/org.example.notes/assets/web/notes.js",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(value["error"]["code"], "unavailable");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_app_detail_carries_its_descriptor() {
    let (state, dir) = apps_state();
    let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["enabled"], true);
    assert!(
        value.get("grant").is_none(),
        "no grant was approved for this app"
    );
    assert_eq!(value["grant_state"], "none");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn the_detail_shows_a_current_grant() {
    let (state, dir) = apps_state();
    write_grant(&dir, &grant_toml(&dir));
    let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["grant_state"], "current");
    assert_eq!(value["grant"]["risk"], "read");
    assert_eq!(value["grant"]["approved_by"], "operator-3f2a");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_stale_grant_is_marked() {
    let (state, dir) = apps_state();
    let text = grant_toml(&dir).replace("app_version = \"0.1.0\"", "app_version = \"9.9.9\"");
    write_grant(&dir, &text);
    let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["grant_state"], "stale");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_invalid_grant_is_marked() {
    let (state, dir) = apps_state();
    write_grant(&dir, "this is not a grant");
    let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["grant_state"], "invalid");
    assert!(value.get("grant").is_none());
    assert!(
        value["grant_problem"]
            .as_str()
            .expect("problem")
            .contains("not valid"),
        "{value}"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn the_catalogue_states_each_grant_without_sending_it() {
    let (state, dir) = apps_state();
    write_grant(&dir, &grant_toml(&dir));
    let (status, value) = json(state, "/api/engine/apps").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["apps"][0]["grant_state"], "current");
    assert!(value["apps"][0].get("grant").is_none());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn the_detail_of_a_corrupt_tree_is_internal() {
    use std::os::unix::fs::PermissionsExt;
    let (state, dir) = apps_state();
    let broken = dir.join("apps/org.example.broken");
    std::fs::create_dir_all(&broken).expect("mkdir");
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let manifest = broken.join("manifest.toml");
    std::fs::write(&manifest, b"not a manifest").expect("write");
    std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    let (status, value) = json(state, "/api/engine/apps/org.example.broken").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(value["error"]["code"], "internal");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_corrupt_app_does_not_hide_the_catalogue() {
    use std::os::unix::fs::PermissionsExt;
    let (state, dir) = apps_state();
    let broken = dir.join("apps/org.example.broken");
    std::fs::create_dir_all(&broken).expect("mkdir");
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let manifest = broken.join("manifest.toml");
    std::fs::write(&manifest, b"not a manifest").expect("write");
    std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    let (status, value) = json(state, "/api/engine/apps").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["apps"].as_array().expect("apps").len(), 1);
    assert_eq!(value["problems"][0]["app"], "org.example.broken");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_missing_app_is_not_found() {
    let (state, dir) = apps_state();
    let (status, value) = json(state, "/api/engine/apps/org.example.missing").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not-found");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_invalid_app_id_is_refused() {
    let (state, dir) = apps_state();
    let (status, value) = json(state, "/api/engine/apps/not-an-id").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_router_without_apps_is_unavailable() {
    let (status, value) = json(state(), "/api/engine/apps").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(value["error"]["code"], "unavailable");
}

#[tokio::test]
async fn health_is_public() {
    assert_eq!(status("/health", None).await, StatusCode::OK);
}

#[tokio::test]
async fn the_api_needs_a_peer_credential() {
    assert_eq!(status("/api/health", None).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_service_user_and_root_are_accepted() {
    assert_eq!(status("/api/health", Some(1000)).await, StatusCode::OK);
    assert_eq!(status("/api/health", Some(0)).await, StatusCode::OK);
}

#[tokio::test]
async fn the_composition_is_served_with_an_empty_surface() {
    let (state, dir) = apps_state();
    let response = router(state)
        .oneshot(request("/api/engine/composition", Some(1000)))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(value["composition"]["version"], 1);
    assert_eq!(value["composition"]["profile"][0]["id"], "default");
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    assert!(value["surface"]["degraded"]
        .as_array()
        .expect("degraded")
        .is_empty());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_composed_unapproved_app_is_degraded() {
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    let response = router(state)
        .oneshot(request("/api/engine/composition", Some(1000)))
        .await
        .expect("response");
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        value["composition"]["profile"][0]["apps"][0],
        "org.example.notes"
    );
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    assert_eq!(value["surface"]["degraded"][0]["app"], "org.example.notes");
    assert_eq!(value["surface"]["degraded"][0]["reason"], "unapproved");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_approved_composed_app_is_in_the_boot_order() {
    let (state, dir) = apps_state_with(&["org.example.notes"]);
    write_grant(&dir, &grant_toml(&dir));
    let (status, value) = json(state, "/api/engine/composition").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["surface"]["apps"][0]["id"], "org.example.notes");
    assert_eq!(value["surface"]["apps"][0]["version"], "0.1.0");
    assert_eq!(value["surface"]["apps"][0]["risk"], "read");
    assert_eq!(value["surface"]["apps"][0]["routes"][0]["method"], "GET");
    assert_eq!(value["surface"]["apps"][0]["routes"][0]["path"], "/notes");
    assert_eq!(
        value["surface"]["apps"][0]["routes"][0]["interface"],
        "example:notes/thing@1.0.0"
    );
    assert_eq!(value["surface"]["apps"][0]["routes"][0]["function"], "ping");
    assert_eq!(value["surface"]["apps"][0]["slots"][0]["id"], "panel.view");
    assert_eq!(
        value["surface"]["apps"][0]["slots"][0]["module"],
        "web/notes.js"
    );
    assert_eq!(value["surface"]["apps"][0]["slots"][0]["order"], 1);
    assert_eq!(value["surface"]["apps"][0]["cli"][0]["verb"], "list");
    assert_eq!(value["surface"]["apps"][0]["cli"][0]["function"], "ping");
    assert!(value["surface"]["degraded"]
        .as_array()
        .expect("degraded")
        .is_empty());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn readiness_is_a_different_word_than_liveness() {
    let response = router(state())
        .oneshot(request("/api/health", Some(1000)))
        .await
        .expect("response");
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(value["status"], "ready");
}

#[tokio::test]
async fn a_booting_engine_is_not_ready() {
    let state = Arc::new(EngineState::new(1000).booting());
    let response = router(state.clone())
        .oneshot(request("/api/health", Some(1000)))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(value["status"], "starting");
    state.mark_ready();
    let response = router(state)
        .oneshot(request("/api/health", Some(1000)))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_trusted_in_process_request_is_accepted() {
    let mut request = request("/api/health", None);
    request.extensions_mut().insert(Trusted);
    let response = router(state()).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_real_socket_connection_carries_the_peer_uid() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let dir = std::env::temp_dir().join(format!("anth-api-uds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("engine.sock");

    let listener = UnixListener::bind(&path).expect("bind");
    let app = router(state_with(current_euid())).into_make_service_with_connect_info::<PeerUid>();
    let server = tokio::spawn(async move { axum::serve(listener, app).await });

    let mut stream = tokio::net::UnixStream::connect(&path)
        .await
        .expect("connect");
    stream
        .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.expect("read");
    let text = String::from_utf8_lossy(&response);
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
    assert!(text.contains("ready"), "{text}");

    server.abort();
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[tokio::test]
async fn any_other_peer_is_refused() {
    assert_eq!(
        status("/api/health", Some(4242)).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn version_reports_the_protocol() {
    let response = router(state())
        .oneshot(request("/api/engine/version", Some(1000)))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(value["protocol"], 1);
}

#[tokio::test]
async fn the_public_router_serves_only_health() {
    let (state, dir) = session_state();
    let public = public_router(state);
    let ok = public
        .clone()
        .oneshot(request("/health", None))
        .await
        .expect("response");
    assert_eq!(ok.status(), StatusCode::OK);
    let denied = public
        .oneshot(request("/api/health", None))
        .await
        .expect("response");
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn only_the_login_path_is_public() {
    let (state, dir) = session_state();
    let (status, _, value) = session_call(
        state,
        "POST",
        "/api/apps/org.example.notes/engine/session",
        json!({"operator": "operator", "password": "x"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "unauthenticated");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn login_works_on_a_mounted_router_without_connect_info() {
    let (state, dir) = session_state();
    let (operator, password) = credentials(&dir);
    let (status, value) = trusted_call(
        state,
        "POST",
        "/engine/session",
        json!({"operator": operator, "password": password}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_cross_origin_post_is_refused() {
    let (state, dir) = session_state();
    let request = Request::builder()
        .method("POST")
        .uri("/api/engine/session")
        .header("host", "panel.test")
        .header("origin", "https://evil.test")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"operator":"operator","password":"x"}"#))
        .expect("request");
    let response = public_router(state)
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    let value: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(value["error"]["code"], "denied");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_same_origin_post_reaches_the_handler() {
    let (state, dir) = session_state();
    let request = Request::builder()
        .method("POST")
        .uri("/api/engine/session")
        .header("host", "panel.test")
        .header("origin", "http://panel.test")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"operator":"operator","password":"x"}"#))
        .expect("request");
    let response = public_router(state)
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn only_a_loopback_peer_may_supply_the_forwarded_host() {
    let (state, dir) = session_state();
    let request = Request::builder()
        .method("POST")
        .uri("/api/engine/session")
        .header("host", "internal:8090")
        .header("x-forwarded-host", "panel.test")
        .header("origin", "http://panel.test")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"operator":"operator","password":"x"}"#))
        .expect("request");
    let mut request = request;
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4000))));
    let response = public_router(Arc::clone(&state))
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .method("POST")
        .uri("/api/engine/session")
        .header("host", "internal:8090")
        .header("x-forwarded-host", "panel.test")
        .header("origin", "http://panel.test")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"operator":"operator","password":"x"}"#))
        .expect("request");
    let mut request = request;
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 4000))));
    let response = public_router(state)
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn api_responses_carry_the_security_headers() {
    let (state, dir) = session_state();
    let response = public_router(state)
        .oneshot(request("/api/health", None))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert!(response.headers()["content-security-policy"]
        .to_str()
        .expect("csp")
        .contains("frame-ancestors 'none'"));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn login_opens_a_session_and_the_cookie_reaches_the_api() {
    let (state, dir) = session_state();
    let (operator, password) = credentials(&dir);
    let (status, headers, value) = session_call(
        Arc::clone(&state),
        "POST",
        "/api/engine/session",
        json!({"operator": operator, "password": password}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["operator"], "operator");
    assert_eq!(value["must_change_password"], true);
    let cookie = cookie(&headers);

    let (status, _, value) = session_call(
        Arc::clone(&state),
        "GET",
        "/api/engine/session",
        Value::Null,
        Some(&cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["transport"], "session");

    let (status, _, value) = session_call(
        Arc::clone(&state),
        "GET",
        "/api/health",
        Value::Null,
        Some(&cookie),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(value["error"]["code"], "password-change-required");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn changing_the_first_boot_password_lifts_the_gate() {
    let (state, dir) = session_state();
    let (operator, password) = credentials(&dir);
    let (_, headers, _) = session_call(
        Arc::clone(&state),
        "POST",
        "/api/engine/session",
        json!({"operator": operator, "password": password}),
        None,
    )
    .await;
    let first_cookie = cookie(&headers);

    let (status, headers, value) = session_call(
        Arc::clone(&state),
        "PATCH",
        "/api/engine/operator",
        json!({"current_password": password, "new_password": "a-new-password"}),
        Some(&first_cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let renewed = cookie(&headers);

    let (status, _, _) = session_call(
        Arc::clone(&state),
        "GET",
        "/api/health",
        Value::Null,
        Some(&renewed),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn logout_revokes_the_token() {
    let (state, dir) = session_state();
    let (operator, password) = credentials(&dir);
    let (_, headers, _) = session_call(
        Arc::clone(&state),
        "POST",
        "/api/engine/session",
        json!({"operator": operator, "password": password}),
        None,
    )
    .await;
    let cookie = cookie(&headers);
    let (status, _, _) = session_call(
        Arc::clone(&state),
        "DELETE",
        "/api/engine/session",
        Value::Null,
        Some(&cookie),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = session_call(
        Arc::clone(&state),
        "GET",
        "/api/engine/session",
        Value::Null,
        Some(&cookie),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_wrong_password_never_mints_a_session() {
    let (state, dir) = session_state();
    let (operator, _) = credentials(&dir);
    let (status, headers, value) = session_call(
        state,
        "POST",
        "/api/engine/session",
        json!({"operator": operator, "password": "not-the-password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "unauthenticated");
    assert!(headers.get("set-cookie").is_none());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn five_failures_block_the_source() {
    let (state, dir) = session_state();
    let (operator, password) = credentials(&dir);
    for _ in 0..5 {
        let (status, _, _) = session_call(
            Arc::clone(&state),
            "POST",
            "/api/engine/session",
            json!({"operator": operator, "password": "wrong"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, headers, value) = session_call(
        state,
        "POST",
        "/api/engine/session",
        json!({"operator": operator, "password": password}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(value["error"]["code"], "rate-limited");
    assert!(value["error"]["detail"]["retry_after"].as_u64().is_some());
    assert!(headers.get("set-cookie").is_none());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_trusted_panel_still_faces_the_gate_and_the_current_password() {
    let (state, dir) = session_state();
    let (status, value) = trusted_call(Arc::clone(&state), "GET", "/health", Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(value["error"]["code"], "password-change-required");

    let (status, value) = trusted_call(
        Arc::clone(&state),
        "PATCH",
        "/engine/operator",
        json!({"new_password": "a-new-password"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(value["error"]["code"], "unauthenticated");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn a_forwarded_https_request_marks_the_cookie_secure() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-forwarded-proto", "https".parse().expect("value"));
    assert!(cookie_secure(&headers));
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-forwarded-proto", "http".parse().expect("value"));
    assert!(!cookie_secure(&headers));
}

#[tokio::test]
async fn an_install_is_audited() {
    let (state, dir) = apps_state();
    let bundle = bundle("org.example.other", "example:other", "0.1.0", b"component");
    let (status, headers, value) = call(state, "POST", "/api/engine/apps", bundle).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        headers.get(LOCATION).expect("location").to_str().unwrap(),
        "/api/engine/apps/org.example.other"
    );
    assert_eq!(value["id"], "org.example.other");
    assert_eq!(value["grant_state"], "none");
    assert!(dir.join("apps/org.example.other/manifest.toml").is_file());
    let entries = AuditLog::new(&dir).entries().expect("audit");
    let entry = entries.last().expect("entry");
    assert_eq!(entry.action, AuditAction::AppInstall);
    assert_eq!(entry.actor, "uid:1000");
    assert_eq!(entry.result, Outcome::Ok);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_failed_install_is_audited() {
    let (state, dir) = apps_state();
    let (status, _, value) = call(state, "POST", "/api/engine/apps", b"not a zip".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid");
    let entries = AuditLog::new(&dir).entries().expect("audit");
    let entry = entries.last().expect("entry");
    assert_eq!(entry.action, AuditAction::AppInstall);
    assert_eq!(entry.result, Outcome::Failed);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn patch_disables_and_renames_the_app() {
    let (state, dir) = apps_state();
    let (status, _, value) = call(
        state.clone(),
        "PATCH",
        "/api/engine/apps/org.example.notes",
        br#"{"enabled":false,"alias":"custom"}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["enabled"], false);
    assert_eq!(value["alias"], "custom");
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.last().unwrap().action, AuditAction::AppDisable);

    let (status, _, value) = call(
        state,
        "PATCH",
        "/api/engine/apps/org.example.notes",
        b"{}".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn remove_deletes_the_app_and_the_grant() {
    let (state, dir) = apps_state();
    write_grant(&dir, &grant_toml(&dir));
    let app = AppId::parse("org.example.notes").unwrap();
    let (status, _, _) = call(
        state,
        "DELETE",
        "/api/engine/apps/org.example.notes",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!dir.join("apps/org.example.notes").exists());
    assert_eq!(GrantStore::new(&dir).load(&app).expect("load"), None);
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.last().unwrap().action, AuditAction::AppRemove);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn replace_swaps_the_bundle_and_revokes_the_grant() {
    let (state, dir) = apps_state();
    write_grant(&dir, &grant_toml(&dir));
    let app = AppId::parse("org.example.notes").unwrap();
    let bundle = bundle(
        "org.example.notes",
        "example:notes",
        "0.2.0",
        b"new component",
    );
    let (status, _, value) = call(state, "PUT", "/api/engine/apps/org.example.notes", bundle).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["version"], "0.2.0");
    assert_eq!(value["grant_state"], "none");
    assert_eq!(GrantStore::new(&dir).load(&app).expect("load"), None);
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.last().unwrap().action, AuditAction::AppUpgrade);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn patch_can_clear_the_alias() {
    let (state, dir) = apps_state();
    let (status, _, value) = call(
        state.clone(),
        "PATCH",
        "/api/engine/apps/org.example.notes",
        br#"{"alias":"custom"}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["alias"], "custom");
    let (status, _, value) = call(
        state,
        "PATCH",
        "/api/engine/apps/org.example.notes",
        br#"{"alias":null}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["alias"], "notes", "the manifest fallback");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_unauthenticated_request_is_audited() {
    let (state, dir) = apps_state();
    let mut request = Request::builder()
        .uri("/api/engine/apps")
        .body(Body::empty())
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1001)));
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let entries = AuditLog::new(&dir).entries().expect("audit");
    let entry = entries.last().expect("entry");
    assert_eq!(entry.action, AuditAction::Deny);
    assert_eq!(entry.actor, "uid:1001");
    assert_eq!(entry.result, Outcome::Denied);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_oversized_install_is_too_large() {
    let (_, dir) = apps_state();
    let state = Arc::new(EngineState::new(1000).with_apps(&dir, 10, false, false));
    let (status, _, value) = call(state, "POST", "/api/engine/apps", vec![0u8; 20]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(value["error"]["code"], "too-large");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_failed_replace_does_not_resurrect_the_grant() {
    let (state, dir) = apps_state();
    write_grant(&dir, &grant_toml(&dir));
    let app = AppId::parse("org.example.notes").unwrap();
    let (status, _, _) = call(
        state,
        "PUT",
        "/api/engine/apps/org.example.notes",
        b"not a zip".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        GrantStore::new(&dir).load(&app).expect("load"),
        None,
        "the grant is not restored"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_chunked_body_over_the_limit_is_too_large() {
    use axum::body::Bytes;
    use futures_util::stream;
    let (_, dir) = apps_state();
    let state = Arc::new(EngineState::new(1000).with_apps(&dir, 10, false, false));
    let body = axum::body::Body::from_stream(stream::iter(vec![
        Ok::<_, std::io::Error>(Bytes::from_static(b"0123456789")),
        Ok(Bytes::from_static(b"more")),
    ]));
    let mut request = Request::builder()
        .method("POST")
        .uri("/api/engine/apps")
        .body(body)
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_corrupt_tree_can_still_be_uninstalled() {
    use std::os::unix::fs::PermissionsExt;
    let (state, dir) = apps_state();
    let broken = dir.join("apps/org.example.broken");
    std::fs::create_dir_all(&broken).expect("mkdir");
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let manifest = broken.join("manifest.toml");
    std::fs::write(&manifest, b"not a manifest").expect("write");
    std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    let (status, _, _) = call(
        state,
        "DELETE",
        "/api/engine/apps/org.example.broken",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!broken.exists());
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.last().unwrap().action, AuditAction::AppRemove);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn approving_freezes_a_grant() {
    let (state, dir) = apps_state();
    let (status, _, value) = call(
        state,
        "PUT",
        "/api/engine/apps/org.example.notes/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(value["grant_state"], "current");
    assert_eq!(value["grant"]["risk"], "read");
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.last().unwrap().action, AuditAction::AppApprove);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn revoking_removes_the_grant() {
    let (state, dir) = apps_state();
    let (status, _, _) = call(
        state.clone(),
        "PUT",
        "/api/engine/apps/org.example.notes/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, _) = call(
        state,
        "DELETE",
        "/api/engine/apps/org.example.notes/grant",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.last().unwrap().action, AuditAction::AppRevoke);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn revoking_a_needed_grant_needs_acknowledgment() {
    let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
    add_dependent(&dir);
    let (status, _, _) = call(
        state.clone(),
        "PUT",
        "/api/engine/apps/org.example.notes/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, value) = call(
        state.clone(),
        "DELETE",
        "/api/engine/apps/org.example.notes/grant",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["error"]["code"], "conflict");
    assert_eq!(
        value["error"]["detail"]["dependents"][0],
        "org.example.consumer"
    );
    assert_eq!(value["error"]["detail"]["acknowledge"][0], "dependents");
    let (status, _, _) = call_with(
        state,
        "DELETE",
        "/api/engine/apps/org.example.notes/grant",
        Vec::new(),
        &[("x-anth-acknowledge", "dependents")],
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert!(
        entries
            .iter()
            .any(|entry| entry.action == AuditAction::Acknowledge),
        "{entries:?}"
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn disabling_a_needed_app_needs_acknowledgment() {
    let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
    add_dependent(&dir);
    let (status, _, value) = call(
        state.clone(),
        "PATCH",
        "/api/engine/apps/org.example.notes",
        br#"{"enabled":false}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(value["error"]["detail"]["acknowledge"][0], "dependents");
    let (status, _, value) = call_with(
        state,
        "PATCH",
        "/api/engine/apps/org.example.notes",
        br#"{"enabled":false}"#.to_vec(),
        &[("x-anth-acknowledge", "dependents")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["enabled"], false);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_grant_with_another_wit_hash_is_stale() {
    let (state, dir) = apps_state();
    let app = AppId::parse("org.example.notes").unwrap();
    let material = Registry::new(&dir)
        .approval_material(&app)
        .expect("material")
        .expect("installed");
    let text = grant_toml(&dir).replace(&material.wit_sha256, &"0".repeat(64));
    write_grant(&dir, &text);
    let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["grant_state"], "stale");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_blocked_consumer_carries_the_missing_link() {
    let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
    add_dependent(&dir);
    // The consumer is approved; the provider is not, so the consumer is
    // blocked rather than merely unapproved.
    let (status, _, _) = call(
        state.clone(),
        "PUT",
        "/api/engine/apps/org.example.consumer/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, value) = json(state, "/api/engine/composition").await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    let degraded = value["surface"]["degraded"].as_array().expect("degraded");
    let consumer = degraded
        .iter()
        .find(|app| app["app"] == "org.example.consumer")
        .expect("consumer");
    assert_eq!(consumer["reason"], "blocked");
    assert_eq!(consumer["detail"]["interface"], "example:notes/thing@1.0.0");
    assert_eq!(consumer["detail"]["provider"], "org.example.notes");
    assert!(value["surface"]["links"]
        .as_array()
        .expect("links")
        .is_empty());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_running_pair_exposes_its_link() {
    let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
    add_dependent(&dir);
    write_grant(&dir, &grant_toml(&dir));
    let (status, _, _) = call(
        state.clone(),
        "PUT",
        "/api/engine/apps/org.example.consumer/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, value) = json(state, "/api/engine/composition").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["surface"]["apps"][0]["id"], "org.example.notes");
    assert_eq!(value["surface"]["apps"][1]["id"], "org.example.consumer");
    assert_eq!(
        value["surface"]["links"][0]["consumer"],
        "org.example.consumer"
    );
    assert_eq!(
        value["surface"]["links"][0]["interface"],
        "example:notes/thing@1.0.0"
    );
    assert_eq!(
        value["surface"]["links"][0]["provider"],
        "org.example.notes"
    );
    assert!(value["surface"]["degraded"]
        .as_array()
        .expect("degraded")
        .is_empty());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn the_composition_is_replaced_with_its_validator() {
    let (state, dir) = apps_state();
    let (status, headers, _) =
        call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let etag = headers
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .expect("etag")
        .to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");
    let body = br#"{"version":1,"profile":[{"id":"default","apps":["org.example.notes"]}]}"#;
    let (status, headers, value) = call_with(
        state.clone(),
        "PUT",
        "/api/engine/composition",
        body.to_vec(),
        &[("if-match", &etag)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value["composition"]["profile"][0]["apps"][0],
        "org.example.notes"
    );
    assert_ne!(
        headers.get("etag").and_then(|value| value.to_str().ok()),
        Some(etag.as_str())
    );
    let (status, value) = json(state, "/api/engine/composition").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        value["composition"]["profile"][0]["apps"][0],
        "org.example.notes"
    );
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert_eq!(entries.last().unwrap().action, AuditAction::Compose);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_stale_or_missing_validator_is_refused() {
    let (state, dir) = apps_state();
    let body = br#"{"version":1,"profile":[{"id":"default","apps":[]}]}"#;
    let (status, _, value) = call(
        state.clone(),
        "PUT",
        "/api/engine/composition",
        body.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(value["error"]["code"], "precondition-failed");
    let (status, _, value) = call_with(
        state.clone(),
        "PUT",
        "/api/engine/composition",
        body.to_vec(),
        &[("if-match", "\"not-the-current-document\"")],
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(value["error"]["code"], "precondition-failed");
    assert!(!dir.join("composition.toml").exists());
    // `*` matches any existing document.
    let (status, _, _) = call_with(
        state,
        "PUT",
        "/api/engine/composition",
        body.to_vec(),
        &[("if-match", "*")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_invalid_composition_is_refused_and_not_written() {
    let (state, dir) = apps_state();
    let body = br#"{"version":1,"profile":[{"id":"default"},{"id":"other","apps":[]}]}"#;
    let (status, _, value) = call_with(
        state,
        "PUT",
        "/api/engine/composition",
        body.to_vec(),
        &[("if-match", "*")],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(value["error"]["code"], "invalid");
    assert!(!dir.join("composition.toml").exists());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_composition_naming_an_uninstalled_app_is_refused() {
    let (state, dir) = apps_state();
    let body = br#"{"version":1,"profile":[{"id":"default","apps":["org.example.missing"]}]}"#;
    let (status, _, value) = call_with(
        state,
        "PUT",
        "/api/engine/composition",
        body.to_vec(),
        &[("if-match", "*")],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not-found");
    assert!(!dir.join("composition.toml").exists());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_fatal_composition_is_still_readable_for_repair() {
    let (state, dir) = apps_state_with(&["org.example.missing"]);
    let (status, headers, value) = call(state, "GET", "/api/engine/composition", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("etag").is_some());
    assert_eq!(
        value["composition"]["profile"][0]["apps"][0],
        "org.example.missing"
    );
    assert_eq!(value["surface"]["fatal"]["code"], "not-found");
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn a_fatal_composition_is_repaired_with_the_served_validator() {
    let (state, dir) = apps_state_with(&["org.example.missing"]);
    let (status, headers, value) =
        call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["surface"]["fatal"]["code"], "not-found");
    let etag = headers
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .expect("etag")
        .to_string();
    let repaired = br#"{"version":1,"profile":[{"id":"default","apps":[]}]}"#;
    let (status, _, _) = call_with(
        state.clone(),
        "PUT",
        "/api/engine/composition",
        repaired.to_vec(),
        &[("if-match", &etag)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, value) = json(state, "/api/engine/composition").await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["composition"]["profile"][0]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    assert!(value["surface"].get("fatal").is_none());
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_composition_lock_covers_the_validator_check() {
    use std::time::Duration;
    let (state, dir) = apps_state();
    let (status, headers, _) =
        call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let stale = headers
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .expect("etag")
        .to_string();
    let guard = state.compose_lock.lock().await;
    let body = br#"{"version":1,"profile":[{"id":"default","apps":[]}]}"#;
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        let mut request = Request::builder()
            .method("PUT")
            .uri("/api/engine/composition")
            .header("content-type", "application/json")
            .header("if-match", stale)
            .body(Body::from(body.to_vec()))
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
        router(task_state)
            .oneshot(request)
            .await
            .expect("response")
            .status()
    });
    // Let the replacement reach the lock, then change the served document
    // while it waits. A check made before the lock would still see the old
    // validator and win; a check inside the lock sees the new one and must
    // refuse.
    tokio::time::sleep(Duration::from_millis(100)).await;
    {
        let mut composition = state.composition.write().expect("composition lock");
        composition.profile[0].apps = vec!["org.example.notes".to_string()];
    }
    drop(guard);
    assert_eq!(task.await.expect("join"), StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        state.composition.read().expect("composition").profile[0].apps,
        vec!["org.example.notes".to_string()]
    );
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_composition_lock_covers_the_write() {
    use std::time::Duration;
    let (state, dir) = apps_state();
    let (status, headers, _) =
        call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let etag = headers
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .expect("etag")
        .to_string();
    let guard = state.compose_lock.lock().await;
    let body = br#"{"version":1,"profile":[{"id":"default","apps":["org.example.notes"]}]}"#;
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        let mut request = Request::builder()
            .method("PUT")
            .uri("/api/engine/composition")
            .header("content-type", "application/json")
            .header("if-match", etag)
            .body(Body::from(body.to_vec()))
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
        router(task_state)
            .oneshot(request)
            .await
            .expect("response")
            .status()
    });
    // While the lock is held, neither the served document nor the file may
    // move: the write happens after the check, inside the same lock.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(state.composition.read().expect("composition").profile[0]
        .apps
        .is_empty());
    assert!(!dir.join("composition.toml").exists());
    drop(guard);
    assert_eq!(task.await.expect("join"), StatusCode::OK);
    assert_eq!(
        state.composition.read().expect("composition").profile[0].apps,
        vec!["org.example.notes".to_string()]
    );
    let text = std::fs::read_to_string(dir.join("composition.toml")).expect("composition");
    assert!(text.contains("org.example.notes"), "{text}");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn the_if_match_header_is_parsed_with_strong_comparison() {
    use axum::http::header::HeaderValue;
    let etag = "\"abc\"";
    let with = |value: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(IF_MATCH, HeaderValue::from_str(value).expect("header"));
        headers
    };
    assert!(if_matches(&with("\"abc\""), etag));
    assert!(if_matches(&with("\"x\", \"abc\""), etag));
    assert!(if_matches(&with(" \"abc\" "), etag));
    assert!(if_matches(&with("*"), etag));
    assert!(!if_matches(&HeaderMap::new(), etag));
    assert!(!if_matches(&with("W/\"abc\""), etag));
    assert!(!if_matches(&with("\"ABC\""), etag));
    assert!(!if_matches(&with("\"nope\""), etag));
    let mut repeated = HeaderMap::new();
    repeated.append(IF_MATCH, HeaderValue::from_static("\"nope\""));
    repeated.append(IF_MATCH, HeaderValue::from_static("\"abc\""));
    assert!(if_matches(&repeated, etag));
}

#[test]
fn a_header_token_list_can_span_lines() {
    use axum::http::header::HeaderValue;
    let mut headers = HeaderMap::new();
    headers.append("x-test", HeaderValue::from_static("a, b"));
    headers.append("x-test", HeaderValue::from_static("c"));
    assert_eq!(
        header_tokens(&headers, "x-test").collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
}

#[tokio::test]
async fn an_uncomposed_dependent_does_not_block_disabling() {
    let (state, dir) = apps_state();
    add_dependent(&dir);
    let (status, _, value) = call(
        state,
        "PATCH",
        "/api/engine/apps/org.example.notes",
        br#"{"enabled":false}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["enabled"], false);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn an_unreadable_other_grant_does_not_block_approval() {
    use std::os::unix::fs::PermissionsExt;
    let (state, dir) = apps_state();
    add_dependent(&dir);
    write_grant_for(&dir, "org.example.consumer", "not a grant");
    let broken = dir.join("apps/org.example.broken");
    std::fs::create_dir_all(&broken).expect("mkdir");
    std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let manifest = broken.join("manifest.toml");
    std::fs::write(&manifest, b"not a manifest").expect("write");
    std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    let (status, _, value) = call(
        state,
        "PUT",
        "/api/engine/apps/org.example.notes/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(value["grant_state"], "current");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn revoking_without_a_grant_is_not_found_even_with_dependents() {
    let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
    add_dependent(&dir);
    let (status, _, value) = call(
        state,
        "DELETE",
        "/api/engine/apps/org.example.notes/grant",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not-found");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn re_approving_a_grant_is_ok() {
    let (state, dir) = apps_state();
    let (status, _, _) = call(
        state.clone(),
        "PUT",
        "/api/engine/apps/org.example.notes/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, value) = call(
        state,
        "PUT",
        "/api/engine/apps/org.example.notes/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["grant_state"], "current");
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[tokio::test]
async fn the_grant_has_its_own_get() {
    let (state, dir) = apps_state();
    let (status, _, _) = call(
        state.clone(),
        "PUT",
        "/api/engine/apps/org.example.notes/grant",
        br#"{"accept":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, value) = json(state.clone(), "/api/engine/apps/org.example.notes/grant").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["risk"], "read");
    let (status, _, _) = call(
        state.clone(),
        "DELETE",
        "/api/engine/apps/org.example.notes/grant",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, value) = json(state, "/api/engine/apps/org.example.notes/grant").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(value["error"]["code"], "not-found");
    std::fs::remove_dir_all(dir).expect("cleanup");
}
