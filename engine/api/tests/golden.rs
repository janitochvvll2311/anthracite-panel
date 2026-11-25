//! Golden surfaces and one test per failure class (§8.5, §12).
//!
//! The fixtures live at the repository root under `tests/fixtures`; the apps are
//! real state-tree layouts and grants are derived exactly as an approval would.

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anth_api::{router, EngineState, PeerUid};
use anth_compose::audit::AuditLog;
use anth_compose::grant::GrantStore;
use anth_compose::keys::{Distribution, Keyring};
use anth_compose::registry::Registry;
use anth_contract::manifest::{Manifest, ServiceClass};
use anth_contract::{
    AppId, Composition, Grant, InterfaceId, KeyAlgorithm, Outcome, Risk, TrustedKey, GRANT_VERSION,
    KEY_VERSION,
};
use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::Value;
use tower::ServiceExt;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixtures() -> PathBuf {
    let mut dir = std::env::current_dir().expect("current directory");
    loop {
        let candidate = dir.join("tests/fixtures");
        if candidate.is_dir() {
            return candidate;
        }
        if !dir.pop() {
            panic!("no tests/fixtures above the current directory");
        }
    }
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
        "anth-golden-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("chmod");
    dir
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir");
    fs::set_permissions(dst, fs::Permissions::from_mode(0o700)).expect("chmod");
    for entry in
        fs::read_dir(src).unwrap_or_else(|error| panic!("reading {}: {error}", src.display()))
    {
        let entry = entry.expect("entry");
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).expect("copy");
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).expect("chmod");
        }
    }
}

/// The fixture directory that holds each app, keyed by app id.
const APPS: &[(&str, &str)] = &[
    ("provider", "org.example.provider"),
    ("consumer", "org.example.consumer"),
    ("other", "org.example.other"),
    ("a", "org.example.a"),
    ("b", "org.example.b"),
    ("store", "org.example.store"),
    ("reader", "org.example.reader"),
];

fn approve(state: &Path, id_text: &str) {
    approve_with(state, id_text, Risk::Read);
}

fn approve_with(state: &Path, id_text: &str, risk: Risk) {
    let id = AppId::parse(id_text).expect("id");
    // The fixtures are a keyless build: the shipped apps install as
    // `third-party` there. A build with a distribution key would refuse them
    // unless that key signed them, which is the point of the namespace rule.
    let registry = Registry::with_distribution(state, Distribution::none());
    let material = registry
        .approval_material(&id)
        .expect("material")
        .expect("installed");
    let installed = registry.get(&id).expect("get").expect("installed");
    let manifest = installed.manifest;
    let grant = Grant {
        version: GRANT_VERSION,
        app: id,
        app_version: manifest.app.version.clone(),
        manifest_sha256: material.manifest_sha256,
        wit_sha256: material.wit_sha256,
        signature: material.signature,
        risk,
        accepted: Grant::required_levels(risk).collect(),
        approved_by: "uid:1000".to_string(),
        approved_at: "2026-11-05T20:00:00Z".to_string(),
        export: manifest
            .export
            .iter()
            .map(|value| InterfaceId::parse(&value.interface).expect("interface"))
            .collect(),
        import: manifest
            .import
            .iter()
            .map(|value| InterfaceId::parse(&value.interface).expect("interface"))
            .collect(),
        roots: manifest
            .service
            .as_ref()
            .map(|service| service.roots.clone())
            .unwrap_or_default(),
        listen: manifest
            .service
            .as_ref()
            .map(|service| service.listen.clone())
            .unwrap_or_default(),
        egress: manifest
            .service
            .as_ref()
            .map(|service| service.egress.clone())
            .unwrap_or_default(),
    };
    GrantStore::new(state).save(&grant).expect("grant");
}

fn fixture_state(composition: &str, approved: &[&str]) -> (Arc<EngineState>, PathBuf) {
    let dir = scratch();
    let apps = dir.join("apps");
    fs::create_dir_all(&apps).expect("apps");
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o700)).expect("chmod");
    for (fixture, id) in APPS {
        copy_tree(&fixtures().join("apps").join(fixture), &apps.join(id));
    }
    for id in approved {
        approve(&dir, id);
    }
    let text = fs::read_to_string(fixtures().join("compositions").join(composition))
        .expect("composition fixture");
    let composition = Composition::parse(&text).expect("parse");
    composition.validate().expect("validate");
    let state = Arc::new(
        EngineState::new(1000)
            .with_composition(composition)
            .with_apps(&dir, 1 << 20, false, false),
    );
    (state, dir)
}

/// An installed tree with one extra fixture app, for a surface that the shared
/// APPS list must not gain.
fn fixture_with(fixture: &str, id: &str) -> (Arc<EngineState>, PathBuf) {
    let dir = scratch();
    let apps = dir.join("apps");
    fs::create_dir_all(&apps).expect("apps");
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o700)).expect("chmod");
    for (name, id) in APPS {
        copy_tree(&fixtures().join("apps").join(name), &apps.join(id));
    }
    copy_tree(&fixtures().join("apps").join(fixture), &apps.join(id));
    let state = Arc::new(EngineState::new(1000).with_apps(&dir, 1 << 20, false, false));
    (state, dir)
}

async fn get(state: Arc<EngineState>, path: &str) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, serde_json::from_slice(&body).expect("json"))
}

fn repo_root() -> PathBuf {
    fixtures()
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

/// The service class of each system app. The risk comes from the class, not
/// from a second table.
const SYSTEM_CLASSES: &[(&str, ServiceClass)] = &[
    ("fs", ServiceClass::Host),
    ("terminal", ServiceClass::Exec),
    ("proc", ServiceClass::Admin),
    ("net", ServiceClass::Host),
    ("firewall", ServiceClass::Host),
    ("storage", ServiceClass::Host),
    ("accounts", ServiceClass::Host),
    ("home", ServiceClass::Host),
    ("certs", ServiceClass::Admin),
    ("acme", ServiceClass::Socket),
    ("proxy", ServiceClass::Socket),
    ("containers", ServiceClass::Admin),
    ("store", ServiceClass::Socket),
];

fn system_class(name: &str) -> ServiceClass {
    SYSTEM_CLASSES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, class)| *class)
        .unwrap_or_else(|| panic!("no service class for `{name}`"))
}

/// The demo composition is the system composition; the test filters it for the
/// `net`-removal case instead of keeping a second app list.
fn demo_composition() -> Composition {
    let path = repo_root().join("demo/composition.toml");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    Composition::parse(&text).expect("demo composition")
}

/// Render the real `manifest.toml.in` with deterministic fixture values.
///
/// The app's WIT files are copied from `apps/<name>/wit`; nothing about the
/// interfaces, routes, slots or CLI is duplicated in the fixture.
fn system_manifest(app: &str, class: &str) -> String {
    let path = repo_root().join("apps").join(app).join("manifest.toml.in");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let sha = "0".repeat(64);
    let asset = "a".repeat(64);
    text.replace("{{app_name}}", "System app")
        .replace("{{app_summary}}", "System app fixture.")
        .replace("{{service_path}}", "bin/app")
        .replace("{{sha256}}", &sha)
        .replace("{{service_class}}", class)
        .replace("{{service_argv}}", r#"["--socket", "{socket}"]"#)
        .replace("{{runtime}}", "/var/run/docker.sock")
        .replace("{{module_sha256}}", &asset)
        .replace("{{modal_sha256}}", &asset)
        .replace("{{icon_sha256}}", &asset)
        .replace("{{refresh_sha256}}", &asset)
}

fn system_state(include_net: bool) -> (Arc<EngineState>, PathBuf) {
    let dir = scratch();
    let apps = dir.join("apps");
    fs::create_dir_all(&apps).expect("apps");
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o700)).expect("chmod");
    let mut composition = demo_composition();
    if !include_net {
        for profile in &mut composition.profile {
            profile.apps.retain(|id| id != "org.anthracite.net");
        }
    }
    composition.validate().expect("validate");

    let mut ids = Vec::new();
    for profile in &composition.profile {
        for id in &profile.apps {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
    }
    for id in &ids {
        let name = id.rsplit('.').next().expect("app id segment");
        let class = system_class(name);
        let app_dir = apps.join(id);
        copy_tree(
            &repo_root().join("apps").join(name).join("wit"),
            &app_dir.join("wit"),
        );
        fs::write(
            app_dir.join("manifest.toml"),
            system_manifest(name, class.as_str()),
        )
        .expect("manifest");
        fs::write(
            app_dir.join("descriptor.toml"),
            format!(
                "version = 1\napp = \"{id}\"\nenabled = true\ntier = \"third-party\"\n\
                 installed_at = \"2026-11-05T20:00:00Z\"\n"
            ),
        )
        .expect("descriptor");
        approve_with(&dir, id, class.risk());
    }
    let state = Arc::new(
        EngineState::new(1000)
            .with_composition(composition)
            .with_apps(&dir, 1 << 20, false, true)
            .keyless(),
    );
    (state, dir)
}

/// The edge composition of §21 §3, as shipped in `compositions/edge.toml`.
fn edge_composition() -> Composition {
    let path = repo_root().join("compositions/edge.toml");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    Composition::parse(&text).expect("edge composition")
}

/// The CI key that signs the edge fixtures. It is deterministic so a fixture
/// tree is reproducible; nothing trusts it outside this test.
fn edge_key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Installs, signs, trusts and approves every app of the edge composition, with
/// `removed` filtered out. Signing matters: a release build verifies the
/// manifest, and the removal matrix must not be proven on unsigned fixtures.
fn edge_state(removed: &[&str], allow_degraded: bool) -> (Arc<EngineState>, PathBuf) {
    let dir = scratch();
    let apps = dir.join("apps");
    fs::create_dir_all(&apps).expect("apps");
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o700)).expect("chmod");
    let mut composition = edge_composition();
    for profile in &mut composition.profile {
        profile
            .apps
            .retain(|id| !removed.iter().any(|wanted| id == wanted));
    }
    composition.validate().expect("validate");

    let signing = edge_key();
    let trusted = TrustedKey {
        version: KEY_VERSION,
        algorithm: KeyAlgorithm::Ed25519,
        public_key: hex(&signing.verifying_key().to_bytes()),
        label: Some("edge-ci".to_string()),
        added_by: "uid:1000".to_string(),
        added_at: "2026-11-25T00:00:00Z".to_string(),
    };
    Keyring::new(&dir).add(&trusted).expect("trusted key");

    let mut ids = Vec::new();
    for profile in &composition.profile {
        for id in &profile.apps {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
    }
    for id in &ids {
        let name = id.rsplit('.').next().expect("app id segment");
        let class = system_class(name);
        let app_dir = apps.join(id);
        copy_tree(
            &repo_root().join("apps").join(name).join("wit"),
            &app_dir.join("wit"),
        );
        fs::write(
            app_dir.join("manifest.toml"),
            system_manifest(name, class.as_str()),
        )
        .expect("manifest");
        let message = anth_compose::bundle::signed_message(&app_dir).expect("message");
        fs::write(
            app_dir.join("manifest.sig"),
            format!(
                "{}\n",
                anth_compose::signature::signature_base64(&signing.sign(&message).to_bytes())
            ),
        )
        .expect("signature");
        fs::write(
            app_dir.join("descriptor.toml"),
            format!(
                "version = 1\napp = \"{id}\"\nenabled = true\ntier = \"third-party\"\n\
                 installed_at = \"2026-11-25T00:00:00Z\"\n"
            ),
        )
        .expect("descriptor");
        approve_with(&dir, id, class.risk());
    }
    let state = Arc::new(
        EngineState::new(1000)
            .with_composition(composition)
            .allow_degraded(allow_degraded)
            .with_apps(&dir, 1 << 20, true, true),
    );
    (state, dir)
}

/// The CLI is generated from the surface, but it invokes an app's route. A
/// verb without a route is a parity bug in the app, not a runtime fallback.
#[test]
fn every_app_cli_verb_has_a_route() {
    let root = repo_root().join("apps");
    let mut apps = fs::read_dir(&root)
        .expect("apps")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    apps.sort();

    let mut apps_checked = 0;
    let mut verbs_checked = 0;
    for app_dir in apps {
        let name = app_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("app name");
        let text = if app_dir.join("manifest.toml.in").is_file() {
            system_manifest(name, "host")
        } else {
            fs::read_to_string(app_dir.join("manifest.toml"))
                .unwrap_or_else(|error| panic!("reading {name}: {error}"))
        };
        let manifest =
            Manifest::parse(&text).unwrap_or_else(|error| panic!("parsing {name}: {error}"));
        let routes = manifest
            .route
            .iter()
            .map(|route| route.call.as_str())
            .collect::<HashSet<_>>();
        for cli in &manifest.cli {
            assert!(
                routes.contains(cli.call.as_str()),
                "{name}: CLI verb `{}` calls `{}` without a matching route",
                cli.verb,
                cli.call
            );
        }
        apps_checked += 1;
        verbs_checked += manifest.cli.len();
    }

    // A silently empty apps directory must not turn this test into a no-op.
    // The counts are a floor: adding apps or verbs keeps passing.
    assert!(
        apps_checked >= 9,
        "checked only {apps_checked} app manifests"
    );
    assert!(
        verbs_checked >= 38,
        "checked only {verbs_checked} CLI verbs across the app catalogue"
    );
}

async fn composition(state: Arc<EngineState>) -> Value {
    let mut request = Request::builder()
        .uri("/api/engine/composition")
        .body(Body::empty())
        .expect("request");
    request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    serde_json::from_slice(&body).expect("json")
}

fn cleanup(dir: PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

struct EchoCaller;

impl anth_api::AppCaller for EchoCaller {
    fn call<'a>(
        &'a self,
        _app: &'a str,
        _interface: &'a str,
        _function: &'a str,
        input: serde_json::Value,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<serde_json::Value, anth_contract::Error>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { Ok(serde_json::json!({"echo": input})) })
    }
}

#[tokio::test]
async fn a_broker_call_uses_the_grant_and_is_audited() {
    let (_, dir) = fixture_state(
        "golden.toml",
        &["org.example.provider", "org.example.consumer"],
    );
    let composition = Composition::parse(
        &fs::read_to_string(fixtures().join("compositions/golden.toml")).expect("composition"),
    )
    .expect("parse");
    let state = Arc::new(
        EngineState::new(1000)
            .with_composition(composition)
            .with_apps(&dir, 1 << 20, false, false)
            .with_caller(Arc::new(EchoCaller)),
    );
    let value = anth_api::broker::call(
        &state,
        "org.example.consumer",
        &serde_json::json!({
            "interface": "example:provider/greeting@1.0.0",
            "function": "greet",
            "input": {"name": "Ada"}
        }),
    )
    .await
    .expect("brokered call");
    assert_eq!(value["echo"]["name"], "Ada");

    let denied = anth_api::broker::call(
        &state,
        "org.example.consumer",
        &serde_json::json!({
            "interface": "example:provider/other@1.0.0",
            "function": "greet"
        }),
    )
    .await
    .expect_err("not imported");
    assert_eq!(denied.0.kind(), anth_contract::ErrorKind::Denied);

    let bridge = anth_api::broker::call(
        &state,
        "org.example.consumer",
        &serde_json::json!({
            "interface": anth_api::broker::AUDIT_INTERFACE,
            "function": "record",
            "input": {"action": "proxy.apply", "detail": {"password": "hidden", "host": "a"}}
        }),
    )
    .await
    .unwrap_or_else(|anth_api::ApiError(error)| panic!("audit bridge: {error}"));
    assert_eq!(bridge["recorded"], true);

    let entries = AuditLog::new(&dir).entries().expect("audit");
    let calls: Vec<_> = entries
        .iter()
        .filter(|entry| entry.action == anth_contract::AuditAction::BrokerCall)
        .collect();
    assert!(calls.len() >= 2, "{entries:?}");
    let denials: Vec<_> = calls
        .iter()
        .filter(|entry| entry.result == Outcome::Denied)
        .collect();
    assert!(
        denials.iter().any(|entry| {
            entry
                .detail
                .as_ref()
                .and_then(|detail| detail["interface"].as_str())
                == Some("example:provider/other@1.0.0")
        }),
        "the denied call is audited: {entries:?}"
    );
    let bridged = calls
        .iter()
        .find(|entry| {
            entry
                .detail
                .as_ref()
                .and_then(|detail| detail["bridge"].as_bool())
                == Some(true)
        })
        .expect("bridge entry");
    let detail = &bridged.detail.as_ref().expect("detail")["detail"];
    assert!(detail.get("password").is_none(), "{detail}");
    assert_eq!(detail["host"], "a");

    // The socket the service actually gets: start the brokers and speak the
    // protocol over `state/broker/<id>.sock`.
    state.start_brokers().expect("brokers");
    let socket = anth_api::broker::socket_path(
        &dir,
        &anth_contract::AppId::parse("org.example.consumer").expect("id"),
    );
    let broker_dir = dir.join("broker");
    let mode = fs::metadata(&broker_dir)
        .expect("broker dir")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o711, "a service traverses the broker directory");
    let socket_meta = fs::metadata(&socket).expect("socket");
    assert_eq!(
        socket_meta.permissions().mode() & 0o777,
        0o600,
        "the socket is private to its service"
    );
    if unsafe { libc::geteuid() } == 0 {
        assert_eq!(
            socket_meta.uid(),
            anth_compose::service_uid("org.example.consumer"),
            "the socket belongs to the service uid"
        );
    }
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::UnixStream::connect(&socket)
        .await
        .expect("connect");
    let body = br#"{"interface":"example:provider/greeting@1.0.0","function":"greet","input":{"name":"Ada"}}"#;
    let head = format!(
        "POST /__anth/call HTTP/1.1\r\nHost: anthracite\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.expect("head");
    stream.write_all(body).await.expect("body");
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.expect("answer");
    let text = String::from_utf8_lossy(&answer);
    assert!(text.contains(r#""ok""#), "{text}");
    assert!(text.contains(r#""echo""#), "{text}");
    cleanup(dir);
}

struct StreamCaller;

impl anth_api::AppCaller for StreamCaller {
    fn call<'a>(
        &'a self,
        _app: &'a str,
        _interface: &'a str,
        _function: &'a str,
        _input: serde_json::Value,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<serde_json::Value, anth_contract::Error>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { Ok(serde_json::json!({})) })
    }

    fn stream<'a>(
        &'a self,
        _app: &'a str,
        _interface: &'a str,
        _function: &'a str,
        _input: serde_json::Value,
    ) -> anth_api::StreamFuture<'a> {
        Box::pin(async move {
            let chunks: Vec<Result<Vec<u8>, anth_contract::Error>> = vec![
                Ok(b"one".to_vec()),
                Ok(b"two".to_vec()),
                Ok(b"three".to_vec()),
            ];
            Ok(Box::pin(futures_util::stream::iter(chunks)) as anth_api::ChunkStream)
        })
    }
}

#[tokio::test]
async fn a_broker_stream_ends_exhausted_at_the_provider_budget() {
    let dir = scratch();
    let apps = dir.join("apps");
    fs::create_dir_all(&apps).expect("apps");
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o700)).expect("chmod");
    for (fixture, id) in APPS {
        copy_tree(&fixtures().join("apps").join(fixture), &apps.join(id));
    }
    // A provider that may emit four bytes in total: the second three-byte chunk
    // is over budget, so the broker must end the stream with `exhausted`.
    let manifest = apps.join("org.example.provider/manifest.toml");
    let mut text = fs::read_to_string(&manifest).expect("manifest");
    text.push_str("\n[limits]\nbytes-total = 4\n");
    fs::write(&manifest, text).expect("limits");
    approve(&dir, "org.example.provider");
    approve(&dir, "org.example.consumer");
    let composition = Composition::parse(
        &fs::read_to_string(fixtures().join("compositions/golden.toml")).expect("composition"),
    )
    .expect("parse");
    let state = Arc::new(
        EngineState::new(1000)
            .with_composition(composition)
            .with_apps(&dir, 1 << 20, false, false)
            .with_caller(Arc::new(StreamCaller)),
    );
    state.start_brokers().expect("brokers");
    let socket =
        anth_api::broker::socket_path(&dir, &AppId::parse("org.example.consumer").expect("id"));
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::UnixStream::connect(&socket)
        .await
        .expect("connect");
    let body = br#"{"interface":"example:provider/greeting@1.0.0","function":"chunks","input":{}}"#;
    let head = format!(
        "POST /__anth/stream HTTP/1.1\r\nHost: anthracite\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.expect("head");
    stream.write_all(body).await.expect("body");
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.expect("answer");
    let text = String::from_utf8_lossy(&answer);
    assert!(text.contains(r#""chunk":"b25l""#), "{text}");
    assert!(text.contains(r#""code":"exhausted""#), "{text}");
    let entries = AuditLog::new(&dir).entries().expect("audit");
    assert!(
        entries
            .iter()
            .any(|entry| entry.result == Outcome::Failed && entry.detail.is_some()),
        "{entries:?}"
    );
    cleanup(dir);
}

#[tokio::test]
async fn a_truncated_broker_body_is_an_error_not_a_panic() {
    let (state, dir) = fixture_state(
        "golden.toml",
        &["org.example.provider", "org.example.consumer"],
    );
    state.start_brokers().expect("brokers");
    let socket =
        anth_api::broker::socket_path(&dir, &AppId::parse("org.example.consumer").expect("id"));
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::UnixStream::connect(&socket)
        .await
        .expect("connect");
    // Announce more bytes than the request carries and close: the broker must
    // answer a typed 400 instead of indexing out of bounds and aborting.
    stream
        .write_all(
            b"POST /__anth/call HTTP/1.1\r\nHost: anthracite\r\nContent-Length: 100\r\n\r\n{",
        )
        .await
        .expect("head");
    stream.shutdown().await.expect("shutdown");
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).await.expect("answer");
    let text = String::from_utf8_lossy(&answer);
    assert!(text.starts_with("HTTP/1.1 400"), "{text}");
    cleanup(dir);
}

#[tokio::test]
async fn a_v4_bundle_shows_its_service_intent() {
    let (state, dir) = fixture_with("v4service", "org.example.v4service");
    let (status, app) = get(state.clone(), "/api/engine/apps/org.example.v4service").await;
    assert_eq!(status, StatusCode::OK, "{app}");
    assert_eq!(app["service"]["class"], "socket");
    assert_eq!(
        app["service"]["roots"],
        serde_json::json!(["/etc/ssl:ro", "/srv/data"])
    );
    assert_eq!(
        app["service"]["listen"],
        serde_json::json!(["tcp:8080", "tcp:127.0.0.1:8443"])
    );
    assert_eq!(
        app["service"]["egress"],
        serde_json::json!(["example.com:443"])
    );
    // Phase 0a freezes the intent only; L1 and the port capability land in 0b.
    assert_eq!(app["service"]["roots_mode"], "intent");
    assert_eq!(app["service"]["listen_mode"], "intent");
    assert_eq!(app["service"]["egress_mode"], "intent");

    let (status, approval) = get(state, "/api/engine/apps/org.example.v4service/approval").await;
    assert_eq!(status, StatusCode::OK, "{approval}");
    assert_eq!(approval["risk"], "admin");
    assert_eq!(approval["service"]["egress_mode"], "intent");
    cleanup(dir);
}

#[tokio::test]
async fn the_golden_composition_produces_the_golden_surface() {
    let (state, dir) = fixture_state(
        "golden.toml",
        &["org.example.provider", "org.example.consumer"],
    );
    let value = composition(state).await;
    let path = fixtures().join("expected/golden.json");
    if std::env::var_os("BLESS").is_some() {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&value).expect("render")
            ),
        )
        .expect("bless");
    }
    let expected: Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("expected fixture"))
            .expect("expected json");
    assert_eq!(value, expected);
    cleanup(dir);
}

#[tokio::test]
async fn the_system_composition_produces_the_golden_surface() {
    let (state, dir) = system_state(true);
    let value = composition(state).await;
    let path = fixtures().join("expected/system.json");
    if std::env::var_os("BLESS").is_some() {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&value).expect("render")
            ),
        )
        .expect("bless");
    }
    let expected: Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("expected fixture"))
            .expect("expected json");
    assert_eq!(value, expected);
    cleanup(dir);
}

#[tokio::test]
async fn removing_net_leaves_the_other_system_apps() {
    let (state, dir) = system_state(false);
    let value = composition(state).await;
    assert!(value["surface"]["fatal"].is_null(), "{value}");
    assert_eq!(value["surface"]["degraded"], serde_json::json!([]));

    let apps = value["surface"]["apps"].as_array().expect("apps");
    let ids = apps
        .iter()
        .map(|app| app["id"].as_str().expect("id"))
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 7, "{ids:?}");
    assert!(!ids.contains(&"org.anthracite.net"), "{ids:?}");
    assert!(ids.contains(&"org.anthracite.firewall"), "{ids:?}");

    let firewall = apps
        .iter()
        .find(|app| app["id"] == "org.anthracite.firewall")
        .expect("firewall");
    assert_eq!(firewall["risk"], "host");
    let view = firewall["slots"]
        .as_array()
        .expect("slots")
        .iter()
        .find(|slot| slot["id"] == "panel.view")
        .expect("firewall view");
    assert_eq!(view["label"], "Firewall");
    assert_eq!(view["parent"], "org.anthracite.net#anth-net");

    cleanup(dir);
}

#[tokio::test]
async fn the_edge_composition_produces_the_golden_surface() {
    let (state, dir) = edge_state(&[], true);
    let value = composition(state).await;
    let path = fixtures().join("expected/edge.json");
    if std::env::var_os("BLESS").is_some() {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&value).expect("render")
            ),
        )
        .expect("bless");
    }
    let expected: Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("expected fixture"))
            .expect("expected json");
    assert_eq!(value, expected);
    cleanup(dir);
}

#[tokio::test]
async fn the_edge_composition_boots_with_twelve_apps() {
    let (state, dir) = edge_state(&[], true);
    let value = composition(state).await;
    assert!(value["surface"]["fatal"].is_null(), "{value}");
    assert_eq!(value["surface"]["degraded"], serde_json::json!([]));
    let ids = value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .iter()
        .map(|app| app["id"].as_str().expect("id"))
        .collect::<HashSet<_>>();
    assert_eq!(ids.len(), 12, "{ids:?}");
    for app in [
        "org.anthracite.certs",
        "org.anthracite.acme",
        "org.anthracite.proxy",
        "org.anthracite.containers",
        "org.anthracite.store",
    ] {
        assert!(ids.contains(app), "`{app}` did not boot: {ids:?}");
    }
    cleanup(dir);
}

/// The removal matrix of §21 §5 at the resolution level: removing one app must
/// leave the rest running and degrade exactly the consumers that imported it.
#[tokio::test]
async fn removing_each_edge_app_leaves_the_expected_degradation() {
    for (removed, running, degraded) in [
        ("org.anthracite.certs", 10, vec!["org.anthracite.acme"]),
        ("org.anthracite.proxy", 11, vec![]),
        ("org.anthracite.acme", 11, vec![]),
        ("org.anthracite.containers", 11, vec![]),
        ("org.anthracite.store", 11, vec![]),
    ] {
        let (state, dir) = edge_state(&[removed], true);
        let value = composition(state).await;
        assert!(
            value["surface"]["fatal"].is_null(),
            "removing `{removed}` was fatal: {value}"
        );
        let apps = value["surface"]["apps"].as_array().expect("apps");
        assert_eq!(apps.len(), running, "removing `{removed}`: {apps:?}");
        assert!(!apps.iter().any(|app| app["id"] == removed));
        let names = value["surface"]["degraded"]
            .as_array()
            .expect("degraded")
            .iter()
            .map(|app| app["app"].as_str().expect("app"))
            .collect::<Vec<_>>();
        assert_eq!(names, degraded, "removing `{removed}`: {value}");
        cleanup(dir);
    }
}

#[tokio::test]
async fn removing_certs_names_the_missing_interface() {
    let (state, dir) = edge_state(&["org.anthracite.certs"], true);
    let value = composition(state).await;
    let acme = value["surface"]["degraded"]
        .as_array()
        .expect("degraded")
        .iter()
        .find(|app| app["app"] == "org.anthracite.acme")
        .expect("acme degraded");
    assert_eq!(acme["reason"], "unresolved");
    assert_eq!(acme["detail"]["interface"], "anthracite:certs/store@1.0.0");
    cleanup(dir);
}

/// Without the opt-in the same document is a fatal composition error, because
/// the operator can repair it by editing the file (§8.5).
#[tokio::test]
async fn removing_certs_without_the_opt_in_is_fatal() {
    let (state, dir) = edge_state(&["org.anthracite.certs"], false);
    let value = composition(state).await;
    assert_eq!(value["surface"]["fatal"]["code"], "invalid", "{value}");
    let message = value["surface"]["fatal"]["message"]
        .as_str()
        .expect("message");
    assert!(message.contains("org.anthracite.acme"), "{message}");
    assert!(
        message.contains("anthracite:certs/store@1.0.0"),
        "{message}"
    );
    cleanup(dir);
}

#[tokio::test]
async fn removing_all_five_edge_apps_leaves_the_system_composition() {
    let (state, dir) = edge_state(
        &[
            "org.anthracite.certs",
            "org.anthracite.acme",
            "org.anthracite.proxy",
            "org.anthracite.containers",
            "org.anthracite.store",
        ],
        true,
    );
    let value = composition(state).await;
    assert!(value["surface"]["fatal"].is_null(), "{value}");
    assert_eq!(value["surface"]["degraded"], serde_json::json!([]));
    assert_eq!(value["surface"]["apps"].as_array().expect("apps").len(), 7);
    cleanup(dir);
}

#[tokio::test]
async fn an_unapproved_app_is_degraded() {
    let (state, dir) = fixture_state("unapproved.toml", &[]);
    let value = composition(state).await;
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    assert_eq!(value["surface"]["degraded"][0]["reason"], "unapproved");
    cleanup(dir);
}

#[tokio::test]
async fn a_blocked_consumer_names_the_provider() {
    let (state, dir) = fixture_state("blocked.toml", &["org.example.consumer"]);
    let value = composition(state).await;
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    let consumer = value["surface"]["degraded"]
        .as_array()
        .expect("degraded")
        .iter()
        .find(|app| app["app"] == "org.example.consumer")
        .expect("consumer");
    assert_eq!(consumer["reason"], "blocked");
    assert_eq!(
        consumer["detail"]["interface"],
        "example:provider/greeting@1.0.0"
    );
    assert_eq!(consumer["detail"]["provider"], "org.example.provider");
    assert!(value["surface"]["links"]
        .as_array()
        .expect("links")
        .is_empty());
    cleanup(dir);
}

#[tokio::test]
async fn a_missing_app_is_fatal_and_named() {
    let (state, dir) = fixture_state("missing-app.toml", &[]);
    let value = composition(state).await;
    assert_eq!(value["surface"]["fatal"]["code"], "not-found");
    assert!(value["surface"]["fatal"]["message"]
        .as_str()
        .expect("message")
        .contains("org.example.missing"));
    cleanup(dir);
}

#[tokio::test]
async fn an_unresolved_import_is_fatal_and_named() {
    let (state, dir) = fixture_state("unresolved-import.toml", &["org.example.consumer"]);
    let value = composition(state).await;
    assert_eq!(value["surface"]["fatal"]["code"], "invalid");
    let message = value["surface"]["fatal"]["message"]
        .as_str()
        .expect("message");
    assert!(message.contains("org.example.consumer"), "{message}");
    assert!(
        message.contains("example:provider/greeting@1.0.0"),
        "{message}"
    );
    assert!(message.contains("nothing composed provides"), "{message}");
    cleanup(dir);
}

#[tokio::test]
async fn a_duplicate_export_is_fatal_and_names_both_apps() {
    let (state, dir) = fixture_state(
        "duplicate-export.toml",
        &["org.example.provider", "org.example.other"],
    );
    let value = composition(state).await;
    assert_eq!(value["surface"]["fatal"]["code"], "invalid");
    let message = value["surface"]["fatal"]["message"]
        .as_str()
        .expect("message");
    assert!(message.contains("org.example.provider"), "{message}");
    assert!(message.contains("org.example.other"), "{message}");
    cleanup(dir);
}

#[tokio::test]
async fn a_cycle_is_fatal_and_names_the_apps() {
    let (state, dir) = fixture_state("cycle.toml", &["org.example.a", "org.example.b"]);
    let value = composition(state).await;
    assert_eq!(value["surface"]["fatal"]["code"], "invalid");
    let message = value["surface"]["fatal"]["message"]
        .as_str()
        .expect("message");
    assert!(message.contains("cyclic"), "{message}");
    assert!(message.contains("org.example.a"), "{message}");
    assert!(message.contains("org.example.b"), "{message}");
    cleanup(dir);
}

#[tokio::test]
async fn a_stale_grant_degrades_the_app() {
    let (state, dir) = fixture_state("unapproved.toml", &["org.example.provider"]);
    let id = AppId::parse("org.example.provider").expect("id");
    let mut grant = GrantStore::new(&dir)
        .load(&id)
        .expect("load")
        .expect("grant");
    grant.app_version = "9.9.9".to_string();
    GrantStore::new(&dir).save(&grant).expect("save");
    let value = composition(state).await;
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    assert_eq!(value["surface"]["degraded"][0]["reason"], "grant-stale");
    cleanup(dir);
}

#[tokio::test]
async fn a_corrupt_grant_is_invalid_not_unapproved() {
    let (state, dir) = fixture_state("unapproved.toml", &["org.example.provider"]);
    fs::write(
        dir.join("grants/org.example.provider.toml"),
        "this is not a grant",
    )
    .expect("write");
    let value = composition(state).await;
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    assert_eq!(value["surface"]["degraded"][0]["reason"], "grant-invalid");
    cleanup(dir);
}

#[tokio::test]
async fn a_disabled_app_is_degraded() {
    let (state, dir) = fixture_state("unapproved.toml", &["org.example.provider"]);
    let id = AppId::parse("org.example.provider").expect("id");
    Registry::with_distribution(&dir, Distribution::none())
        .set_enabled(&id, false)
        .expect("disable");
    let value = composition(state).await;
    assert!(value["surface"]["apps"]
        .as_array()
        .expect("apps")
        .is_empty());
    assert_eq!(value["surface"]["degraded"][0]["reason"], "disabled");
    cleanup(dir);
}

#[tokio::test]
async fn a_risk_growth_degrades_the_consumer() {
    let (state, dir) = fixture_state("risk-grown.toml", &[]);
    approve_with(&dir, "org.example.store", Risk::Write);
    approve_with(&dir, "org.example.reader", Risk::Read);
    let value = composition(state).await;
    let apps = value["surface"]["apps"].as_array().expect("apps");
    assert_eq!(apps.len(), 1, "only the provider runs");
    assert_eq!(apps[0]["id"], "org.example.store");
    assert_eq!(apps[0]["risk"], "write");
    let reader = value["surface"]["degraded"]
        .as_array()
        .expect("degraded")
        .iter()
        .find(|app| app["app"] == "org.example.reader")
        .expect("reader");
    assert_eq!(reader["reason"], "risk-grown");
    assert_eq!(reader["detail"]["approved"], "read");
    assert_eq!(reader["detail"]["effective"], "write");
    cleanup(dir);
}

#[tokio::test]
async fn a_pinned_version_that_is_not_installed_is_fatal() {
    let (state, dir) = fixture_state("version-mismatch.toml", &["org.example.provider"]);
    let value = composition(state).await;
    assert_eq!(value["surface"]["fatal"]["code"], "invalid");
    assert!(value["surface"]["fatal"]["message"]
        .as_str()
        .expect("message")
        .contains("9.9.9"));
    cleanup(dir);
}
