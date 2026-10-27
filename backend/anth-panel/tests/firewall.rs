//! Firewall service, HTTP and socket checks with a scripted `nft`.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::client::Client;
use anth_panel::config::{AuthConfig, Config};
use anth_panel::dispatch::Daemon;
use anth_panel::firewall::{FirewallService, NftOutput, NftRunner};
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use anth_wire::{FirewallAction, FirewallProtocol, Request, Response};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

#[derive(Default)]
struct FakeNft {
    calls: Mutex<Vec<Vec<String>>>,
    responses: Mutex<VecDeque<Result<NftOutput, String>>>,
}

impl FakeNft {
    fn new(responses: Vec<Result<NftOutput, String>>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().expect("lock").clone()
    }
}

impl NftRunner for FakeNft {
    fn run(&self, args: &[String]) -> Result<NftOutput, String> {
        self.calls.lock().expect("lock").push(args.to_vec());
        self.responses
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or_else(|| Err("fake runner ran out of responses".to_string()))
    }
}

fn ok(stdout: &str) -> Result<NftOutput, String> {
    Ok(NftOutput {
        ok: true,
        stdout: stdout.to_string(),
        stderr: String::new(),
    })
}

fn fail(stderr: &str) -> Result<NftOutput, String> {
    Ok(NftOutput {
        ok: false,
        stdout: String::new(),
        stderr: stderr.to_string(),
    })
}

fn rules_json() -> String {
    serde_json::json!({
        "nftables": [
            {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 3, "comment": "anth:deny:tcp:80|http"}}
        ]
    })
    .to_string()
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

struct Built {
    daemon: Arc<Daemon>,
    paths: anth_panel::config::Paths,
}

fn daemon(name: &str, control: bool, nft: Arc<FakeNft>) -> Built {
    let (_root, config_path) = scratch(name);
    let mut config = Config::dev_default();
    config.firewall.control = control;
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let firewall = FirewallService::with_runner(control, nft);
    let daemon = Daemon::new(config, paths.clone(), logger).with_firewall(firewall);
    Built {
        daemon: Arc::new(daemon),
        paths,
    }
}

fn state(daemon: Arc<Daemon>) -> AppState {
    let (_root, config_path) = scratch("firewall-state");
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

fn get(uri: &str) -> HttpRequest<Body> {
    HttpRequest::builder()
        .uri(uri)
        .body(Body::empty())
        .expect("request")
}

#[tokio::test]
async fn the_http_routes_read_add_delete_and_flush() {
    let nft = Arc::new(FakeNft::new(vec![
        ok(&rules_json()),
        ok("table inet anthracite"),
        fail("No such file or directory"),
        ok(""),
        fail("No such file or directory"),
        ok(""),
        ok(""),
        ok(""),
        ok(&rules_json()),
        ok("table inet anthracite"),
        ok(""),
    ]));
    let built = daemon("firewall-http", true, Arc::clone(&nft));
    let router = api::router(state(built.daemon));

    let response = router
        .clone()
        .oneshot(get("/api/firewall"))
        .await
        .expect("status");
    assert_eq!(response.status(), StatusCode::OK);
    let snapshot = body_json(response).await;
    assert_eq!(snapshot["initialized"], true);
    assert_eq!(snapshot["rules"][0]["handle"], 3);
    assert_eq!(snapshot["rules"][0]["action"], "deny");
    assert_eq!(snapshot["rules"][0]["protocol"], "tcp");
    assert_eq!(snapshot["rules"][0]["port"], 80);

    let response = router
        .clone()
        .oneshot(post(
            "/api/firewall/rule",
            json!({ "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" }),
        ))
        .await
        .expect("deny");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["message"], "denied tcp/80");

    let response = router
        .clone()
        .oneshot(post("/api/firewall/delete", json!({ "handle": 3 })))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .clone()
        .oneshot(post("/api/firewall/flush", json!({})))
        .await
        .expect("flush");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "flushed 1 firewall rule"
    );

    let response = router
        .oneshot(post(
            "/api/firewall/rule",
            json!({ "action": "nope", "protocol": "tcp", "port": 80 }),
        ))
        .await
        .expect("invalid");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let calls = nft.calls();
    assert!(calls.iter().any(|call| call
        == &vec![
            "add",
            "rule",
            "inet",
            "anthracite",
            "input",
            "tcp",
            "dport",
            "80",
            "drop",
            "comment",
            "\"anth:deny:tcp:80|http\""
        ]));
    assert!(
        calls
            .iter()
            .any(|call| call == &vec!["flush", "chain", "inet", "anthracite", "input"])
    );
}

#[tokio::test]
async fn the_http_route_refuses_when_control_is_off() {
    let nft = Arc::new(FakeNft::default());
    let built = daemon("firewall-http-off", false, Arc::clone(&nft));
    let router = api::router(state(built.daemon));

    let response = router
        .oneshot(post(
            "/api/firewall/rule",
            json!({ "action": "deny", "protocol": "tcp", "port": 22 }),
        ))
        .await
        .expect("deny");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "firewall_control_disabled"
    );
    assert!(nft.calls().is_empty());
}

#[tokio::test]
async fn the_socket_answers_status_and_mutations() {
    let nft = Arc::new(FakeNft::new(vec![
        ok(&rules_json()),
        ok("table inet anthracite"),
        fail("No such file or directory"),
        ok(""),
        fail("No such file or directory"),
        ok(""),
        ok(""),
        ok(&rules_json()),
        ok("table inet anthracite"),
        ok(""),
    ]));
    let built = daemon("firewall-socket", true, Arc::clone(&nft));
    let listener = socket::bind(&built.paths.socket).expect("bind");
    let server = tokio::spawn(socket::serve(listener, Arc::clone(&built.daemon)));

    let mut client = Client::connect(&built.paths.socket).await.expect("connect");
    client.hello().await.expect("hello");

    match client.call(&Request::FirewallStatus).await.expect("status") {
        Response::FirewallSnapshot(snapshot) => {
            assert!(snapshot.initialized);
            assert_eq!(snapshot.rules.len(), 1);
        }
        other => panic!("expected a firewall snapshot, got {other:?}"),
    }

    match client
        .call(&Request::FirewallAdd {
            action: FirewallAction::Deny,
            protocol: FirewallProtocol::Udp,
            port: 53,
            comment: Some("dns".to_string()),
        })
        .await
        .expect("add")
    {
        Response::FirewallChanged { message } => assert_eq!(message, "denied udp/53"),
        other => panic!("expected a firewall change, got {other:?}"),
    }

    match client.call(&Request::FirewallFlush).await.expect("flush") {
        Response::FirewallChanged { message } => assert!(message.starts_with("flushed")),
        other => panic!("expected a firewall change, got {other:?}"),
    }

    server.abort();
}

#[test]
fn the_service_parses_and_reports_the_policy() {
    let nft = Arc::new(FakeNft::new(vec![ok(&rules_json()), ok("text")]));
    let service = FirewallService::with_runner(true, nft);
    let snapshot = service.snapshot().expect("snapshot");
    assert!(snapshot.initialized);
    assert!(snapshot.control_enabled);
    assert_eq!(snapshot.rules[0].action, "deny");
    assert_eq!(snapshot.rules[0].comment.as_deref(), Some("http"));
}
