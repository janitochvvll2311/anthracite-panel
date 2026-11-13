//! Golden surfaces and one test per failure class (§8.5, §12).
//!
//! The fixtures live at the repository root under `tests/fixtures`; the apps are
//! real state-tree layouts and grants are derived exactly as an approval would.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anth_api::{router, EngineState, PeerUid};
use anth_compose::grant::GrantStore;
use anth_compose::registry::Registry;
use anth_contract::{AppId, Composition, Grant, InterfaceId, Risk, GRANT_VERSION};
use axum::body::{to_bytes, Body};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
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
    let registry = Registry::new(state);
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

fn repo_root() -> PathBuf {
    fixtures()
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

/// The real system apps, with the risk their service class contributes.
const SYSTEM_APPS: &[(&str, &str, Risk, &str)] = &[
    ("fs", "org.anthracite.fs", Risk::Host, "host"),
    ("terminal", "org.anthracite.terminal", Risk::Host, "exec"),
    ("proc", "org.anthracite.proc", Risk::Admin, "admin"),
    ("net", "org.anthracite.net", Risk::Host, "host"),
    ("firewall", "org.anthracite.firewall", Risk::Host, "host"),
    ("storage", "org.anthracite.storage", Risk::Host, "host"),
    ("accounts", "org.anthracite.accounts", Risk::Host, "host"),
    ("home", "org.anthracite.home", Risk::Host, "host"),
];

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
        .replace("{{module_sha256}}", &asset)
        .replace("{{modal_sha256}}", &asset)
        .replace("{{icon_sha256}}", &asset)
        .replace("{{refresh_sha256}}", &asset)
}

fn system_state(composition: &str) -> (Arc<EngineState>, PathBuf) {
    let dir = scratch();
    let apps = dir.join("apps");
    fs::create_dir_all(&apps).expect("apps");
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o700)).expect("chmod");
    for (name, id, risk, class) in SYSTEM_APPS {
        let app_dir = apps.join(id);
        copy_tree(
            &repo_root().join("apps").join(name).join("wit"),
            &app_dir.join("wit"),
        );
        fs::write(app_dir.join("manifest.toml"), system_manifest(name, class)).expect("manifest");
        fs::write(
            app_dir.join("descriptor.toml"),
            format!(
                "version = 1\napp = \"{id}\"\nenabled = true\ntier = \"third-party\"\n\
                 installed_at = \"2026-11-05T20:00:00Z\"\n"
            ),
        )
        .expect("descriptor");
        approve_with(&dir, id, *risk);
    }
    let text = fs::read_to_string(fixtures().join("compositions").join(composition))
        .expect("composition fixture");
    let composition = Composition::parse(&text).expect("parse");
    composition.validate().expect("validate");
    let state = Arc::new(
        EngineState::new(1000)
            .with_composition(composition)
            .with_apps(&dir, 1 << 20, false, true),
    );
    (state, dir)
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
    let (state, dir) = system_state("system.toml");
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
    let (state, dir) = system_state("system-without-net.toml");
    let value = composition(state).await;
    assert!(value["surface"]["fatal"].is_null(), "{value}");
    assert_eq!(value["surface"]["degraded"], serde_json::json!([]));
    assert_eq!(value["surface"]["links"], serde_json::json!([]));

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
    Registry::new(&dir)
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
