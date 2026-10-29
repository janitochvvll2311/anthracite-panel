//! Guarded network mutations over the service, the socket and HTTP.
//!
//! Every test scripts `ip` with [`FakeIp`]; none of them changes the host.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::network::{IpError, IpRunner, SystemNetwork};
use anth_panel::socket;
use anth_wire::{NetworkCreate, NetworkEdit};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

mod common;

#[derive(Default)]
struct FakeIp {
    calls: Mutex<Vec<Vec<String>>>,
    failure: Option<IpError>,
}

impl FakeIp {
    fn failing(stderr: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            failure: Some(IpError::Failed(stderr.to_string())),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().expect("lock").clone()
    }

    fn argv(&self) -> Vec<Vec<String>> {
        self.calls()
    }
}

impl IpRunner for FakeIp {
    fn run(&self, args: &[String]) -> Result<(), IpError> {
        self.calls.lock().expect("lock").push(args.to_vec());
        match &self.failure {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn service(ip: Arc<FakeIp>) -> SystemNetwork {
    SystemNetwork::with_runner(true, true, ip)
}

fn create(name: &str, kind: &str) -> NetworkCreate {
    NetworkCreate {
        name: name.to_string(),
        kind: kind.to_string(),
        peer: None,
        link: None,
        vlan_id: None,
        mtu: None,
        up: false,
    }
}

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("create scratch");
    (root.clone(), root.join("config.toml"))
}

fn daemon(name: &str, network: SystemNetwork) -> (Arc<Daemon>, anth_panel::config::Paths) {
    let (_root, config_path) = scratch(name);
    let config = Config::dev_default();
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let daemon = Daemon::new(config, paths.clone(), logger).with_network(network);
    (Arc::new(daemon), paths)
}

fn state(daemon: Arc<Daemon>) -> AppState {
    let (_root, config_path) = scratch("network-mutate-state");
    let config = Config::dev_default();
    let paths = config.paths(&config_path).expect("paths");
    let auth = AuthConfig {
        enabled: false,
        ..AuthConfig::default()
    };
    let (auth, _) = AuthService::load(&auth, &paths).expect("auth");
    AppState::new(
        daemon,
        auth,
        Arc::new(Logger::buffer(Settings::default()).0),
    )
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

#[test]
fn create_runs_the_scripted_argv_without_touching_the_host() {
    let ip = Arc::new(FakeIp::default());
    let service = service(Arc::clone(&ip));

    let message = service
        .create(&NetworkCreate {
            mtu: Some(1400),
            up: true,
            ..create("anthtest0", "dummy")
        })
        .expect("create");
    assert_eq!(message, "created dummy interface `anthtest0`");
    assert_eq!(
        ip.argv(),
        vec![
            vec!["link", "add", "anthtest0", "type", "dummy"],
            vec!["link", "set", "dev", "anthtest0", "mtu", "1400"],
            vec!["link", "set", "dev", "anthtest0", "up"],
        ]
    );
}

#[test]
fn guards_run_before_ip_and_leave_no_side_effects() {
    let ip = Arc::new(FakeIp::default());
    let service = service(Arc::clone(&ip));

    let loopback = service.set_state("lo", false).expect_err("loopback down");
    assert_eq!(loopback.code, "protected_interface");

    let delete_loopback = service.delete("lo", None).expect_err("delete loopback");
    assert_eq!(delete_loopback.code, "protected_interface");

    let delete_missing = service.delete("anthmissing0", None).expect_err("missing");
    assert_eq!(delete_missing.code, "interface_not_found");

    let bad_name = service
        .create(&create("bad name", "dummy"))
        .expect_err("bad name");
    assert_eq!(bad_name.code, "invalid_interface");

    let bad_mtu = service
        .create(&NetworkCreate {
            mtu: Some(2),
            ..create("anthtest1", "dummy")
        })
        .expect_err("bad mtu");
    assert_eq!(bad_mtu.code, "invalid_interface");

    let bad_vlan = service
        .create(&NetworkCreate {
            link: Some("anthmissing0".to_string()),
            vlan_id: Some(2),
            ..create("anthtest2", "vlan")
        })
        .expect_err("missing parent");
    assert_eq!(bad_vlan.code, "interface_not_found");

    let nothing = service
        .edit(&NetworkEdit {
            name: "lo".to_string(),
            mtu: None,
            up: None,
        })
        .expect_err("nothing");
    assert_eq!(nothing.code, "invalid_interface");

    let manage_only = SystemNetwork::with_runner(false, true, ip.clone());
    let no_control = manage_only
        .edit(&NetworkEdit {
            name: "eth0".to_string(),
            mtu: None,
            up: Some(false),
        })
        .expect_err("state through edit");
    assert_eq!(no_control.code, "network_control_disabled");

    assert!(ip.calls().is_empty(), "no guard may invoke ip");
}

#[test]
fn edit_and_state_changes_call_the_scripted_runner() {
    let ip = Arc::new(FakeIp::default());
    let service = service(Arc::clone(&ip));

    assert_eq!(service.set_state("lo", true).expect("up"), "`lo` is up");
    assert_eq!(
        service
            .edit(&NetworkEdit {
                name: "lo".to_string(),
                mtu: Some(9000),
                up: None,
            })
            .expect("mtu"),
        "updated `lo`: mtu 9000"
    );
    assert_eq!(
        ip.argv(),
        vec![
            vec!["link", "set", "dev", "lo", "up"],
            vec!["link", "set", "dev", "lo", "mtu", "9000"],
        ]
    );
}

#[test]
fn a_refused_ip_maps_to_a_wire_error() {
    let ip = Arc::new(FakeIp::failing(
        "RTNETLINK answers: Operation not permitted",
    ));
    let service = service(ip);

    let error = service
        .create(&create("anthtest3", "dummy"))
        .expect_err("permission");
    assert_eq!(error.kind, anth_wire::ErrorKind::Forbidden);
    assert_eq!(error.code, "ip_permission");
    assert!(error.message.contains("Operation not permitted"));
}

#[tokio::test]
async fn the_http_routes_map_policy_and_success() {
    let ip = Arc::new(FakeIp::default());
    let (daemon, _paths) = daemon("network-mutate-http", service(Arc::clone(&ip)));
    let router = api::router(state(daemon));

    let response = router
        .clone()
        .oneshot(post(
            "/api/network/state",
            json!({ "name": "lo", "up": false }),
        ))
        .await
        .expect("state");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "protected_interface"
    );

    let response = router
        .clone()
        .oneshot(post(
            "/api/network/create",
            json!({ "name": "bad name", "kind": "dummy" }),
        ))
        .await
        .expect("create invalid");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = router
        .clone()
        .oneshot(post(
            "/api/network/create",
            json!({ "name": "anthtest4", "kind": "dummy" }),
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "created dummy interface `anthtest4`"
    );

    let response = router
        .clone()
        .oneshot(post("/api/network/delete", json!({ "name": "lo" })))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = router
        .oneshot(post(
            "/api/network/rename",
            json!({ "name": "anthmissing0", "new_name": "x" }),
        ))
        .await
        .expect("rename");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    assert_eq!(
        ip.argv(),
        vec![vec!["link", "add", "anthtest4", "type", "dummy"]]
    );
}

#[tokio::test]
async fn the_socket_answers_mutations_through_the_same_dispatch() {
    let ip = Arc::new(FakeIp::default());
    let (daemon, paths) = daemon("network-mutate-socket", service(Arc::clone(&ip)));
    let listener = socket::bind(&paths.socket).expect("bind");
    let router = common::local_router(Arc::clone(&daemon), &paths);
    let server = tokio::spawn(socket::serve(listener, router));

    let client = anth_client::Client::unix(&paths.socket);
    let message = client
        .network_create(&create("anthtest5", "bridge"))
        .await
        .expect("create");
    assert_eq!(message, "created bridge interface `anthtest5`");

    let error = client
        .network_set_state("lo", false)
        .await
        .expect_err("state");
    assert_eq!(error.code, "protected_interface");

    assert_eq!(
        ip.argv(),
        vec![vec!["link", "add", "anthtest5", "type", "bridge"]]
    );
    server.abort();
}
