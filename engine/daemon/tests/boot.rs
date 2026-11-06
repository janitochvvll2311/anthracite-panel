//! Boot against fixture state with the fake runtime (§8.5, §12).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anth_compose::grant::GrantStore;
use anth_compose::registry::Registry;
use anth_contract::{AppId, Composition, Grant, InterfaceId, Risk, GRANT_VERSION};
use anth_engine::boot::start_composed;
use anth_engine::config::LimitsSection;
use anth_engine::supervisor::Supervisor;
use anth_runtime::{FakeRuntime, LogSettings};

fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("current directory");
    loop {
        if dir.join("tests/fixtures").is_dir() {
            return dir;
        }
        if !dir.pop() {
            panic!("no tests/fixtures above the current directory");
        }
    }
}

fn scratch(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
        "anth-boot-{name}-{}-{}",
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
    for entry in fs::read_dir(src).expect("read_dir") {
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

fn approve(state: &Path, id_text: &str) {
    let id = AppId::parse(id_text).expect("id");
    let registry = Registry::new(state);
    let material = registry
        .approval_material(&id)
        .expect("material")
        .expect("installed");
    let manifest = registry.get(&id).expect("get").expect("installed").manifest;
    let grant = Grant {
        version: GRANT_VERSION,
        app: id,
        app_version: manifest.app.version.clone(),
        manifest_sha256: material.manifest_sha256,
        wit_sha256: material.wit_sha256,
        signature: material.signature,
        risk: Risk::Read,
        accepted: Vec::new(),
        approved_by: "uid:1000".to_string(),
        approved_at: "2026-11-06T12:00:00Z".to_string(),
        export: manifest
            .export
            .iter()
            .map(|value| InterfaceId::parse(&value.interface).expect("export"))
            .collect(),
        import: manifest
            .import
            .iter()
            .map(|value| InterfaceId::parse(&value.interface).expect("import"))
            .collect(),
    };
    GrantStore::new(state).save(&grant).expect("grant");
}

fn fixture(name: &str, approved: &[&str]) -> (PathBuf, Composition) {
    let dir = scratch(name);
    let apps = dir.join("apps");
    fs::create_dir_all(&apps).expect("apps");
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o700)).expect("chmod");
    let fixtures = repo_root().join("tests/fixtures/apps");
    for (fixture, id) in [
        ("provider", "org.example.provider"),
        ("consumer", "org.example.consumer"),
    ] {
        copy_tree(&fixtures.join(fixture), &apps.join(id));
    }
    for id in approved {
        approve(&dir, id);
    }
    let text = fs::read_to_string(
        repo_root()
            .join("tests/fixtures/compositions")
            .join(format!("{name}.toml")),
    )
    .expect("composition");
    (dir, Composition::parse(&text).expect("parse"))
}

async fn boot(
    dir: &Path,
    composition: &Composition,
    runtime: Arc<FakeRuntime>,
) -> anth_engine::boot::BootReport {
    let supervisor = Supervisor::new(runtime);
    start_composed(
        &supervisor,
        dir,
        composition,
        &LimitsSection::default(),
        LogSettings::default(),
    )
    .await
    .expect("boot")
}

#[tokio::test]
async fn apps_start_in_dependency_order() {
    let (dir, composition) = fixture("golden", &["org.example.provider", "org.example.consumer"]);
    let runtime = Arc::new(FakeRuntime::new());
    let report = boot(&dir, &composition, runtime.clone()).await;
    assert!(report.fatal.is_none());
    assert_eq!(
        report.started,
        [
            AppId::parse("org.example.provider").unwrap(),
            AppId::parse("org.example.consumer").unwrap()
        ]
    );
    assert!(report.degraded.is_empty());
    assert_eq!(runtime.instances(), 2);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_unapproved_provider_blocks_its_consumer() {
    let (dir, composition) = fixture("blocked", &["org.example.consumer"]);
    let runtime = Arc::new(FakeRuntime::new());
    let report = boot(&dir, &composition, runtime).await;
    assert!(report.started.is_empty());
    assert_eq!(
        report
            .degraded
            .get(&AppId::parse("org.example.provider").unwrap())
            .map(String::as_str),
        Some("unapproved")
    );
    assert_eq!(
        report
            .degraded
            .get(&AppId::parse("org.example.consumer").unwrap())
            .map(String::as_str),
        Some("blocked")
    );
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_failed_start_degrades_its_consumers() {
    let (dir, composition) = fixture("golden", &["org.example.provider", "org.example.consumer"]);
    fs::remove_file(dir.join("apps/org.example.provider/wasm/app.wasm")).expect("remove wasm");
    let runtime = Arc::new(FakeRuntime::new());
    let report = boot(&dir, &composition, runtime.clone()).await;
    assert!(report.started.is_empty());
    assert!(report
        .degraded
        .get(&AppId::parse("org.example.provider").unwrap())
        .expect("provider")
        .contains("reading"));
    assert!(report
        .degraded
        .get(&AppId::parse("org.example.consumer").unwrap())
        .expect("consumer")
        .contains("not running"));
    assert_eq!(runtime.instances(), 0);
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_fatal_composition_is_reported_without_starting_anything() {
    let dir = scratch("missing");
    fs::create_dir_all(dir.join("apps")).expect("apps");
    fs::set_permissions(dir.join("apps"), fs::Permissions::from_mode(0o700)).expect("chmod");
    let text = r#"version = 1

[[profile]]
id = "default"
apps = ["org.example.missing"]
"#;
    let composition = Composition::parse(text).expect("parse");
    let runtime = Arc::new(FakeRuntime::new());
    let report = boot(&dir, &composition, runtime).await;
    assert_eq!(
        report.fatal.as_ref().map(|error| error.kind().code()),
        Some("not-found")
    );
    assert!(report.started.is_empty());
    let _ = fs::remove_dir_all(dir);
}
