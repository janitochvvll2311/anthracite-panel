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
    boot_with(dir, composition, runtime, false).await
}

async fn boot_with(
    dir: &Path,
    composition: &Composition,
    runtime: Arc<FakeRuntime>,
    allow_degraded: bool,
) -> anth_engine::boot::BootReport {
    let supervisor = Supervisor::new(runtime);
    let services = anth_engine::service::ServiceSupervisor::new(
        dir,
        anth_compose::audit::AuditLog::new(dir),
        std::time::Duration::from_secs(5),
    );
    start_composed(
        &supervisor,
        &services,
        dir,
        composition,
        anth_engine::boot::BootOptions {
            allow_degraded,
            allow_third_party_service: false,
            limits: &LimitsSection::default(),
            log: LogSettings::default(),
        },
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
async fn an_unresolved_import_degrades_without_taking_the_boot_down() {
    let (dir, composition) = fixture("unresolved-import", &["org.example.consumer"]);
    let runtime = Arc::new(FakeRuntime::new());
    let report = boot_with(&dir, &composition, runtime, true).await;
    assert!(report.fatal.is_none(), "{:?}", report.fatal);
    assert!(report.started.is_empty());
    assert_eq!(
        report
            .degraded
            .get(&AppId::parse("org.example.consumer").unwrap())
            .map(String::as_str),
        Some("unresolved")
    );
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_unresolved_import_is_fatal_without_the_opt_in() {
    let (dir, composition) = fixture("unresolved-import", &["org.example.consumer"]);
    let runtime = Arc::new(FakeRuntime::new());
    let report = boot(&dir, &composition, runtime).await;
    assert!(report.started.is_empty());
    let fatal = report.fatal.expect("fatal resolution");
    assert!(fatal.message().contains("example:provider/greeting@1.0.0"));
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

/// Copies the fixture binary into a service-only app tree and approves it at
/// its class risk. The fixture speaks the private protocol of §19 §4.
fn service_app(dir: &Path) -> (AppId, PathBuf) {
    use sha2::{Digest, Sha256};

    let app = AppId::parse("org.example.service").expect("id");
    let app_dir = dir.join("apps").join(app.as_str());
    for path in [app_dir.join("bin"), app_dir.join("wit")] {
        fs::create_dir_all(&path).expect("mkdir");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("chmod");
    }
    let test = std::env::current_exe().expect("test binary");
    let fixture = test
        .parent()
        .and_then(Path::parent)
        .expect("target directory")
        .join("examples/echo_service");
    if !fixture.exists() {
        let status = std::process::Command::new(
            std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()),
        )
        .args(["build", "--example", "echo_service", "--manifest-path"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .status()
        .expect("build the service fixture");
        assert!(status.success(), "the service fixture did not build");
    }
    let binary = app_dir.join("bin/service");
    fs::copy(&fixture, &binary).expect("copy fixture");
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).expect("chmod");
    let hash: String = Sha256::digest(fs::read(&binary).expect("read binary"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let manifest = format!(
        "manifest_version = 3\n\n[app]\nid = \"org.example.service\"\nnamespace = \"example:service\"\nname = \"Service\"\nversion = \"0.1.0\"\n\n[service]\npath = \"bin/service\"\nsha256 = \"{hash}\"\nclass = \"socket\"\nargv = [\"--socket\", \"{{socket}}\"]\n\n[[export]]\ninterface = \"example:service/notes@1.0.0\"\n\n[[route]]\nmethod = \"GET\"\npath = \"/notes\"\ncall = \"example:service/notes@1.0.0#echo\"\n\n[[cli]]\ncall = \"example:service/notes@1.0.0#echo\"\nverb = \"list\"\n"
    );
    let manifest_path = app_dir.join("manifest.toml");
    fs::write(&manifest_path, manifest).expect("manifest");
    fs::set_permissions(&manifest_path, fs::Permissions::from_mode(0o600)).expect("chmod");
    fs::write(
        app_dir.join("wit/service.wit"),
        "package example:service@1.0.0;\n\ninterface notes {\n  echo: func(input: string) -> string;\n}\n\nworld app {\n  export notes;\n}\n",
    )
    .expect("wit");
    fs::set_permissions(
        app_dir.join("wit/service.wit"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("chmod");

    let registry = Registry::new(dir);
    let material = registry
        .approval_material(&app)
        .expect("material")
        .expect("installed");
    let installed = registry.get(&app).expect("get").expect("installed");
    let grant = Grant {
        version: GRANT_VERSION,
        app: app.clone(),
        app_version: installed.manifest.app.version.clone(),
        manifest_sha256: material.manifest_sha256,
        wit_sha256: material.wit_sha256,
        signature: material.signature,
        risk: Risk::Admin,
        accepted: Grant::required_levels(Risk::Admin).collect(),
        approved_by: "uid:1000".to_string(),
        approved_at: "2026-11-06T12:00:00Z".to_string(),
        export: installed
            .manifest
            .export
            .iter()
            .map(|value| InterfaceId::parse(&value.interface).expect("export"))
            .collect(),
        import: Vec::new(),
        roots: Vec::new(),
        listen: Vec::new(),
        egress: Vec::new(),
    };
    GrantStore::new(dir).save(&grant).expect("grant");
    let socket = anth_engine::service::ServiceSupervisor::socket_path(dir, &app);
    (app, socket)
}

#[tokio::test]
async fn the_switch_off_degrades_an_installed_service() {
    let dir = scratch("service-off");
    fs::create_dir_all(dir.join("apps")).expect("apps");
    fs::set_permissions(dir.join("apps"), fs::Permissions::from_mode(0o700)).expect("chmod");
    let (app, socket) = service_app(&dir);
    let composition = Composition::parse(
        "version = 1\n\n[[profile]]\nid = \"default\"\napps = [\"org.example.service\"]\n",
    )
    .expect("parse");
    let services =
        anth_engine::service::ServiceSupervisor::for_state(&dir, std::time::Duration::from_secs(2));
    let supervisor = Supervisor::new(Arc::new(FakeRuntime::new()));
    let report = start_composed(
        &supervisor,
        &services,
        &dir,
        &composition,
        anth_engine::boot::BootOptions {
            allow_degraded: false,
            allow_third_party_service: false,
            limits: &LimitsSection::default(),
            log: LogSettings::default(),
        },
    )
    .await
    .expect("boot");
    assert!(report.started.is_empty());
    let reason = report.degraded.get(&app).expect("degraded");
    assert!(reason.contains("allow-third-party"), "{reason}");
    assert!(!socket.exists());
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_service_only_app_starts_answers_and_stops() {
    let dir = scratch("service");
    fs::create_dir_all(dir.join("apps")).expect("apps");
    fs::set_permissions(dir.join("apps"), fs::Permissions::from_mode(0o700)).expect("chmod");
    let (app, socket) = service_app(&dir);
    let composition = Composition::parse(
        "version = 1\n\n[[profile]]\nid = \"default\"\napps = [\"org.example.service\"]\n",
    )
    .expect("parse");
    let services =
        anth_engine::service::ServiceSupervisor::for_state(&dir, std::time::Duration::from_secs(2));
    let supervisor = Supervisor::new(Arc::new(FakeRuntime::new()));
    // A cold CI runner is slower than a local shell; the default 5s start
    // window makes this test flaky under load.
    let limits = LimitsSection {
        start_timeout_ms: 30_000,
        ..LimitsSection::default()
    };
    let report = start_composed(
        &supervisor,
        &services,
        &dir,
        &composition,
        anth_engine::boot::BootOptions {
            allow_degraded: false,
            allow_third_party_service: true,
            limits: &limits,
            log: LogSettings::default(),
        },
    )
    .await
    .expect("boot");
    assert_eq!(
        report.started,
        vec![app.clone()],
        "degraded: {:?}",
        report.degraded
    );
    assert!(socket.exists());
    let mode = fs::metadata(dir.join("broker"))
        .expect("broker directory")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o711,
        "boot must not turn the broker directory back into 0700"
    );
    let value = services
        .call(
            app.as_str(),
            "example:service/notes@1.0.0",
            "echo",
            serde_json::json!({"message": "hello"}),
        )
        .await
        .expect("call");
    assert_eq!(value, serde_json::json!({"message": "hello"}));
    services.stop_all().await;
    assert!(!socket.exists());
    let _ = fs::remove_dir_all(dir);
}
