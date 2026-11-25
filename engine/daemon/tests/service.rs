//! The service supervisor against the fixture in `examples/echo_service.rs`.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anth_compose::audit::AuditLog;
use anth_contract::{AppId, AuditAction, ExportTarget, Listener, Root, ServiceClass};
use anth_engine::service::{ServiceLaunch, ServiceSupervisor};
use futures_util::StreamExt;
use serde_json::json;

const APP: &str = "org.example.notes";
const INTERFACE: &str = "example:notes/list@1.0.0";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("anthracite-panel")
        .join(format!("anth-service-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    dir
}

fn fixture() -> PathBuf {
    let test = std::env::current_exe().expect("test binary");
    let path = test
        .parent()
        .and_then(Path::parent)
        .expect("target directory")
        .join("examples")
        .join("echo_service");
    if !path.exists() {
        // `cargo test --workspace` builds examples; a narrow `--test service`
        // does not, so build the one this test needs instead of failing oddly.
        let status = std::process::Command::new(
            std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()),
        )
        .args(["build", "--example", "echo_service", "--manifest-path"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .status()
        .expect("build the service fixture");
        assert!(status.success(), "the service fixture did not build");
    }
    path
}

fn launch(state: &Path, extra: &[&str], start_timeout: Duration) -> ServiceLaunch {
    let app = AppId::parse(APP).expect("app id");
    let socket = ServiceSupervisor::socket_path(state, &app);
    let mut argv = vec![
        "--socket".to_string(),
        socket.to_string_lossy().into_owned(),
    ];
    argv.extend(extra.iter().map(|argument| argument.to_string()));
    let mut exports = BTreeMap::new();
    exports.insert(INTERFACE.to_string(), ExportTarget::Service);
    ServiceLaunch {
        app: APP.to_string(),
        binary: fixture(),
        class: ServiceClass::Socket,
        argv,
        socket,
        data_dir: state.join("data").join(APP),
        roots: Vec::new(),
        listen: Vec::new(),
        exports,
        start_timeout,
    }
}

#[tokio::test]
async fn a_service_starts_answers_and_stops() {
    let state = scratch("lifecycle");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    let spec = launch(&state, &[], Duration::from_millis(500));
    let socket = spec.socket.clone();
    services.start(spec).await.expect("start");

    let mode = std::fs::metadata(&socket)
        .expect("socket")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);

    let output = services
        .call(APP, INTERFACE, "echo", json!({"message": "hi"}))
        .await
        .expect("call");
    assert_eq!(output, json!({"message": "hi"}));

    // L1: with no declared roots the service cannot read or list `/`, even
    // though it is a real process on the host.
    let outside = services
        .call(APP, INTERFACE, "outside", json!({}))
        .await
        .expect("outside");
    assert_eq!(outside["read"], false, "{outside}");
    assert_eq!(outside["list"], false, "{outside}");

    services.stop(APP).await;
    assert!(!socket.exists(), "the socket is removed on stop");
    let error = services
        .call(APP, INTERFACE, "echo", json!({}))
        .await
        .expect_err("stopped");
    assert_eq!(error.kind(), anth_contract::ErrorKind::Unavailable);
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn the_data_directory_is_writable_on_an_ensured_tree() {
    let state = scratch("data-ensure");
    anth_engine::state::ensure(&state).expect("ensure");
    let data = state.join("data").join(APP);
    let spec = launch(
        &state,
        &["--data", data.to_str().expect("utf8")],
        Duration::from_millis(500),
    );
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services.start(spec).await.expect("start");
    let answer = services
        .call(APP, INTERFACE, "write-data", json!({}))
        .await
        .expect("write-data");
    assert_eq!(answer["wrote"], true, "{answer}");
    assert!(data.join("fixture-data").exists());
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_read_only_root_cannot_be_written() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let state = scratch("read-only-root");
    let probe = state.join("probe");
    std::fs::create_dir(&probe).expect("probe");
    let uid = anth_engine::sandbox::allocate_uid(APP).expect("uid");
    let gid = anth_engine::sandbox::engine_gid();
    if anth_engine::sandbox::is_root() {
        let path = CString::new(probe.as_os_str().as_bytes()).expect("path");
        // SAFETY: chown takes a NUL-terminated path.
        assert_eq!(unsafe { libc::chown(path.as_ptr(), uid, gid) }, 0);
    }
    let mut spec = launch(
        &state,
        &["--probe", probe.to_str().expect("utf8")],
        Duration::from_millis(500),
    );
    spec.roots = vec![Root {
        path: probe.to_string_lossy().into_owned(),
        read_only: true,
    }];
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services.start(spec).await.expect("start");
    let answer = services
        .call(APP, INTERFACE, "write-root", json!({}))
        .await
        .expect("write-root");
    // The directory belongs to the service uid, so DAC would allow the write;
    // only the `:ro` Landlock rule stops it.
    assert_eq!(answer["wrote"], false, "{answer}");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_missing_root_is_denied_not_fatal() {
    // A runtime socket that is not installed is the common case: the service
    // must start so the app can report it, while the missing path stays denied.
    let state = scratch("missing-root");
    let mut spec = launch(&state, &[], Duration::from_millis(500));
    spec.roots = vec![Root {
        path: "/does/not/exist/anthracite".to_string(),
        read_only: true,
    }];
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services.start(spec).await.expect("start");
    let outside = services
        .call(APP, INTERFACE, "outside", json!({}))
        .await
        .expect("outside");
    assert_eq!(outside["read"], false, "{outside}");
    assert_eq!(outside["list"], false, "{outside}");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_socket_root_starts_and_answers() {
    let state = scratch("socket-root");
    let path = state.join("runtime.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&path).expect("socket");
    let mut spec = launch(&state, &[], Duration::from_millis(500));
    spec.roots = vec![Root {
        path: path.to_string_lossy().into_owned(),
        read_only: false,
    }];
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services
        .start(spec)
        .await
        .expect("start with a socket root");
    let output = services
        .call(APP, INTERFACE, "echo", json!({"message": "socket"}))
        .await
        .expect("call");
    assert_eq!(output, json!({"message": "socket"}));
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn the_shared_domain_is_writable_by_the_service() {
    let state = scratch("shared");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    let shared = state.join("shared");
    let spec = launch(
        &state,
        &["--shared", shared.to_str().expect("utf8")],
        Duration::from_millis(500),
    );
    services.start(spec).await.expect("start");
    let answer = services
        .call(APP, INTERFACE, "write-shared", json!({}))
        .await
        .expect("write-shared");
    assert_eq!(answer["wrote"], true, "{answer}");
    assert!(shared.join("fixture-shared").exists());
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_declared_root_is_readable_and_the_rest_is_not() {
    let state = scratch("roots");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    let mut spec = launch(&state, &[], Duration::from_millis(500));
    spec.roots = vec![Root {
        path: "/etc".to_string(),
        read_only: true,
    }];
    services.start(spec).await.expect("start");
    let outside = services
        .call(APP, INTERFACE, "outside", json!({}))
        .await
        .expect("outside");
    assert_eq!(outside["read"], true, "{outside}");
    // `/` was not declared, so listing it stays denied.
    assert_eq!(outside["list"], false, "{outside}");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_declared_privileged_listener_is_granted() {
    // Port 443 needs CAP_NET_BIND_SERVICE; a host without it refuses, which is
    // the documented denial. The sandbox this runs in is root with the bit.
    if !anth_engine::sandbox::is_root() || !anth_engine::sandbox::has_net_bind_service() {
        eprintln!("skipping: the engine has no CAP_NET_BIND_SERVICE");
        return;
    }
    let state = scratch("privileged");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    let mut spec = launch(&state, &["--tcp", "443"], Duration::from_secs(2));
    spec.listen = vec![Listener::Tcp {
        ip: None,
        port: 443,
    }];
    services.start(spec).await.expect("start with 443");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_refused_launch_is_a_denial_not_a_crash() {
    let state = scratch("deny");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    let mut spec = launch(&state, &[], Duration::from_millis(500));
    // A missing binary is the cheapest sandbox refusal: the supervisor cannot
    // exec what the grant approved.
    spec.binary = state.join("missing-service");
    let error = services.start(spec).await.expect_err("refused");
    assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);

    let entries = AuditLog::new(&state).entries().expect("audit");
    let deny = entries
        .iter()
        .find(|entry| entry.action == AuditAction::ServiceDeny)
        .expect("the refusal is audited");
    assert_eq!(
        deny.detail
            .as_ref()
            .and_then(|detail| detail["class"].as_str()),
        Some("socket")
    );
    assert!(
        !entries
            .iter()
            .any(|entry| entry.action == AuditAction::ServiceCrash),
        "{entries:?}"
    );
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_service_error_keeps_its_contract_envelope() {
    let state = scratch("error");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services
        .start(launch(&state, &[], Duration::from_millis(500)))
        .await
        .expect("start");
    let error = services
        .call(APP, INTERFACE, "fail", json!({}))
        .await
        .expect_err("refused");
    assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    assert!(error.message().contains("refused"), "{error}");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_service_that_never_answers_health_is_degraded() {
    let state = scratch("health");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    // The fixture stays up but never binds, so readiness times out.
    let spec = launch(&state, &["--no-listen"], Duration::from_millis(300));
    let error = services.start(spec).await.expect_err("no health");
    assert_eq!(error.kind(), anth_contract::ErrorKind::Unavailable);
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn the_engine_caller_routes_a_service_export() {
    use anth_api::AppCaller;

    let state = scratch("caller");
    let services =
        std::sync::Arc::new(ServiceSupervisor::for_state(&state, Duration::from_secs(2)));
    services
        .start(launch(&state, &[], Duration::from_millis(500)))
        .await
        .expect("start");
    let wasm = anth_engine::supervisor::SupervisorCaller::new(std::sync::Arc::new(
        anth_engine::supervisor::Supervisor::new(std::sync::Arc::new(
            anth_runtime::FakeRuntime::new(),
        )),
    ));
    let caller = anth_engine::service::EngineCaller::new(wasm, services.clone());
    let output = caller
        .call(APP, INTERFACE, "echo", json!({"routed": true}))
        .await
        .expect("routed");
    assert_eq!(output, json!({"routed": true}));
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_service_stream_yields_chunks_in_order() {
    let state = scratch("stream");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services
        .start(launch(&state, &[], Duration::from_millis(500)))
        .await
        .expect("start");
    let mut stream = services
        .stream(APP, INTERFACE, "chunks", json!({}))
        .await
        .expect("stream");
    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        collected.extend_from_slice(&chunk.expect("chunk"));
    }
    assert_eq!(collected, b"onetwothree");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_service_stream_error_before_the_first_byte_is_an_error() {
    let state = scratch("stream-error");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services
        .start(launch(&state, &[], Duration::from_millis(500)))
        .await
        .expect("start");
    let error = match services.stream(APP, INTERFACE, "fail", json!({})).await {
        Ok(_) => panic!("the fixture should refuse"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    assert!(error.message().contains("refused to stream"), "{error}");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_service_stream_that_ends_early_is_truncated_not_clean() {
    let state = scratch("stream-break");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services
        .start(launch(&state, &[], Duration::from_millis(500)))
        .await
        .expect("start");
    let mut stream = services
        .stream(APP, INTERFACE, "break", json!({}))
        .await
        .expect("stream");
    let first = stream.next().await.expect("one chunk").expect("chunk");
    assert_eq!(first, b"head");
    let error = stream
        .next()
        .await
        .expect("the stream ends")
        .expect_err("not a clean EOF");
    assert!(error.message().contains("before its terminator"), "{error}");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn the_engine_caller_routes_a_service_stream() {
    use anth_api::AppCaller;

    let state = scratch("stream-caller");
    let services =
        std::sync::Arc::new(ServiceSupervisor::for_state(&state, Duration::from_secs(2)));
    services
        .start(launch(&state, &[], Duration::from_millis(500)))
        .await
        .expect("start");
    let wasm = anth_engine::supervisor::SupervisorCaller::new(std::sync::Arc::new(
        anth_engine::supervisor::Supervisor::new(std::sync::Arc::new(
            anth_runtime::FakeRuntime::new(),
        )),
    ));
    let caller = anth_engine::service::EngineCaller::new(wasm, services.clone());
    let mut stream = caller
        .stream(APP, INTERFACE, "chunks", json!({}))
        .await
        .expect("routed");
    let mut collected = Vec::new();
    while let Some(chunk) = stream.next().await {
        collected.extend_from_slice(&chunk.expect("chunk"));
    }
    assert_eq!(collected, b"onetwothree");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}

#[tokio::test]
async fn a_crash_restarts_the_service() {
    let state = scratch("restart");
    let marker = state.join("crashed");
    let services = ServiceSupervisor::for_state(&state, Duration::from_secs(2));
    services
        .start(launch(
            &state,
            &[
                "--die-once",
                marker.to_string_lossy().as_ref(),
                "--die-after-ms",
                "300",
            ],
            Duration::from_millis(500),
        ))
        .await
        .expect("start");

    // The fixture exits once; make sure it has, then wait for the restart.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut output = None;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(value) = services
            .call(APP, INTERFACE, "echo", json!({"again": true}))
            .await
        {
            output = Some(value);
            break;
        }
    }
    assert_eq!(
        output,
        Some(json!({"again": true})),
        "the restarted service answers"
    );
    let actions: Vec<AuditAction> = AuditLog::new(&state)
        .entries()
        .expect("audit")
        .into_iter()
        .map(|entry| entry.action)
        .collect();
    assert!(
        actions
            .iter()
            .filter(|action| **action == AuditAction::ServiceStart)
            .count()
            >= 2,
        "{actions:?}"
    );
    assert!(actions.contains(&AuditAction::ServiceCrash), "{actions:?}");
    services.stop(APP).await;
    let _ = std::fs::remove_dir_all(&state);
}
