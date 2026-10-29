//! Firewall service, HTTP and socket checks with a scripted `nft`.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Config};
use anth_panel::dispatch::Daemon;
use anth_panel::firewall::{NftOutput, NftRunner, SystemFirewall};
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use anth_wire::{FirewallAction, FirewallChain, FirewallProtocol};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

mod common;

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

fn rules_json() -> String {
    serde_json::json!({
        "nftables": [
            {"chain": {"family": "inet", "table": "anthracite", "name": "input", "type": "filter", "hook": "input", "policy": "drop"}},
            {"chain": {"family": "inet", "table": "anthracite", "name": "output", "type": "filter", "hook": "output", "policy": "accept"}},
            {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 3, "comment": "anth:deny:tcp:80|http"}},
            {"rule": {"family": "inet", "table": "anthracite", "chain": "output", "handle": 6, "comment": "anth:allow:tcp:443|https"}}
        ]
    })
    .to_string()
}

fn empty_json() -> String {
    r#"{"nftables":[]}"#.to_string()
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
    let firewall = SystemFirewall::with_runner(control, nft);
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
async fn the_http_routes_read_add_update_delete_flush_and_set_policy() {
    let nft = Arc::new(FakeNft::new(vec![
        ok(&rules_json()),
        ok("table inet anthracite"),
        ok(""),
        ok(""),
        ok(&empty_json()),
        ok(""),
        ok(&rules_json()),
        ok(""),
        ok(&rules_json()),
        ok(""),
        ok(&rules_json()),
        ok(""),
        ok(""),
        ok(""),
        ok(&rules_json()),
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
    assert_eq!(snapshot["rules"][0]["chain"], "input");
    assert_eq!(snapshot["rules"][0]["action"], "deny");
    assert_eq!(snapshot["rules"][0]["protocol"], "tcp");
    assert_eq!(snapshot["rules"][0]["port"], 80);
    assert_eq!(snapshot["chains"][0]["chain"], "input");
    assert_eq!(snapshot["chains"][0]["policy"], "deny");
    assert_eq!(snapshot["chains"][1]["chain"], "output");
    assert_eq!(snapshot["chains"][1]["policy"], "allow");

    let response = router
        .clone()
        .oneshot(post(
            "/api/firewall/rule",
            json!({ "chain": "input", "action": "deny", "protocol": "tcp", "port": 80, "comment": "http" }),
        ))
        .await
        .expect("deny");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "denied tcp/80 on input"
    );

    let response = router
        .clone()
        .oneshot(post(
            "/api/firewall/update",
            json!({ "handle": 3, "chain": "input", "action": "allow", "protocol": "tcp", "port": 443, "comment": "https" }),
        ))
        .await
        .expect("update");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "updated firewall rule 3 to allow tcp/443 on input"
    );

    let response = router
        .clone()
        .oneshot(post("/api/firewall/delete", json!({ "handle": 3 })))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::OK);

    let response = router
        .clone()
        .oneshot(post("/api/firewall/flush", json!({ "chain": "output" })))
        .await
        .expect("flush");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "flushed 1 firewall rule from output"
    );

    let response = router
        .clone()
        .oneshot(post(
            "/api/firewall/policy",
            json!({ "chain": "input", "policy": "allow" }),
        ))
        .await
        .expect("policy");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "set input policy to allow"
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
    assert!(calls.iter().any(|call| call
        == &vec![
            "replace",
            "rule",
            "inet",
            "anthracite",
            "input",
            "handle",
            "3",
            "tcp",
            "dport",
            "443",
            "accept",
            "comment",
            "\"anth:allow:tcp:443|https\""
        ]));
    assert!(
        calls
            .iter()
            .any(|call| call == &vec!["flush", "chain", "inet", "anthracite", "output"])
    );
    assert!(calls.iter().any(|call| call
        == &vec![
            "add",
            "chain",
            "inet",
            "anthracite",
            "input",
            "{ policy accept; }"
        ]));
}

#[tokio::test]
async fn the_http_add_route_honours_the_output_chain() {
    let nft = Arc::new(FakeNft::new(vec![
        ok(""),
        ok(""),
        ok(&empty_json()),
        ok(""),
    ]));
    let built = daemon("firewall-http-output", true, Arc::clone(&nft));
    let router = api::router(state(built.daemon));

    let response = router
        .oneshot(post(
            "/api/firewall/rule",
            json!({ "chain": "output", "action": "deny", "protocol": "udp", "port": 5353 }),
        ))
        .await
        .expect("deny");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "denied udp/5353 on output"
    );

    let calls = nft.calls();
    assert_eq!(
        calls.last().expect("add"),
        &vec![
            "add",
            "rule",
            "inet",
            "anthracite",
            "output",
            "udp",
            "dport",
            "5353",
            "drop",
            "comment",
            "\"anth:deny:udp:5353\""
        ]
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
            json!({ "chain": "input", "action": "deny", "protocol": "tcp", "port": 22 }),
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
        ok(""),
        ok(""),
        ok(&empty_json()),
        ok(""),
        ok(&rules_json()),
        ok(""),
        ok(""),
        ok(""),
        ok(&rules_json()),
        ok(""),
        ok(&rules_json()),
        ok(""),
    ]));
    let built = daemon("firewall-socket", true, Arc::clone(&nft));
    let listener = socket::bind(&built.paths.socket).expect("bind");
    let router = common::local_router(Arc::clone(&built.daemon), &built.paths);
    let server = tokio::spawn(socket::serve(listener, router));

    let client = anth_client::Client::unix(&built.paths.socket);
    let snapshot = client.firewall().await.expect("status");
    assert!(snapshot.initialized);
    assert_eq!(snapshot.rules.len(), 2);
    assert_eq!(snapshot.chains[1].chain, FirewallChain::Output);

    let message = client
        .firewall_add(
            FirewallChain::Output,
            FirewallAction::Deny,
            FirewallProtocol::Udp,
            53,
            Some("dns"),
        )
        .await
        .expect("add");
    assert_eq!(message, "denied udp/53 on output");

    let message = client
        .firewall_update(
            3,
            FirewallChain::Input,
            FirewallAction::Allow,
            FirewallProtocol::Tcp,
            443,
            Some("https"),
        )
        .await
        .expect("update");
    assert_eq!(message, "updated firewall rule 3 to allow tcp/443 on input");

    let message = client
        .firewall_policy(FirewallChain::Output, FirewallAction::Deny)
        .await
        .expect("policy");
    assert_eq!(message, "set output policy to deny");

    let message = client
        .firewall_flush(FirewallChain::Input)
        .await
        .expect("flush");
    assert!(message.starts_with("flushed"));

    server.abort();
}

#[test]
fn the_service_parses_and_reports_the_policy() {
    let nft = Arc::new(FakeNft::new(vec![ok(&rules_json()), ok("text")]));
    let service = SystemFirewall::with_runner(true, nft);
    let snapshot = service.snapshot().expect("snapshot");
    assert!(snapshot.initialized);
    assert!(snapshot.control_enabled);
    assert_eq!(snapshot.rules[0].action, "deny");
    assert_eq!(snapshot.rules[0].comment.as_deref(), Some("http"));
    assert_eq!(snapshot.chains[0].policy, FirewallAction::Deny);
    assert_eq!(snapshot.chains[1].policy, FirewallAction::Allow);
}
