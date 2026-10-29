//! Accounts over HTTP and the socket with a scripted host and runner.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::accounts::{AccountOutput, AccountRunner, HostProbe, LocalFiles, SystemAccounts};
use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AccountsConfig, AuthConfig, Config};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

mod common;

const PASSWD: &str = "\
root:x:0:0:root:/root:/bin/sh
alice:x:1001:1001:Alice:/home/alice:/bin/sh
bob:x:1002:1002::/home/bob:/bin/sh
";

const GROUP: &str = "\
root:x:0:
wheel:x:10:alice
alice:x:1001:
bob:x:1002:
scratch:x:1501:
";

const SHADOW: &str = "\
root:!:19000:0:99999:7:::
alice:$6$salt$hash:19000:0:99999:7:::
bob:!!:19000:0:99999:7:::
";

fn files() -> LocalFiles {
    LocalFiles {
        passwd: PASSWD.to_string(),
        group: GROUP.to_string(),
        shadow: Some(SHADOW.to_string()),
        gshadow: Some("wheel:!::alice\n".to_string()),
        login_defs: Some("UID_MIN 1000\nGID_MIN 1000\n".to_string()),
        nss_passwd: Some(String::new()),
        nss_group: Some(String::new()),
    }
}

struct FakeProbe {
    files: LocalFiles,
}

impl FakeProbe {
    fn new() -> Self {
        Self { files: files() }
    }
}

impl HostProbe for FakeProbe {
    fn read(&self) -> LocalFiles {
        self.files.clone()
    }
}

type Call = (String, Vec<String>, Option<String>);

#[derive(Debug, Default)]
struct FakeRunner {
    calls: Mutex<Vec<Call>>,
    output: Mutex<VecDeque<AccountOutput>>,
}

impl FakeRunner {
    fn new(outputs: Vec<AccountOutput>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            output: Mutex::new(outputs.into()),
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().expect("lock").clone()
    }
}

impl AccountRunner for FakeRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
    ) -> Result<AccountOutput, String> {
        self.calls.lock().expect("lock").push((
            program.to_string(),
            args.to_vec(),
            stdin.map(str::to_string),
        ));
        Ok(self
            .output
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or(AccountOutput {
                ok: true,
                stderr: String::new(),
            }))
    }
}

fn accounts_config(control: bool) -> AccountsConfig {
    AccountsConfig {
        control,
        manage_passwords: true,
        allow_system: true,
        allow_privileged_groups: true,
        home_root: "/home".to_string(),
        shells: vec!["/bin/sh".to_string()],
        default_shell: "/bin/sh".to_string(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("scratch");
    root.join("config.toml")
}

fn daemon(
    name: &str,
    control: bool,
    probe: Arc<FakeProbe>,
    runner: Arc<FakeRunner>,
) -> (Arc<Daemon>, anth_panel::config::Paths) {
    let config_path = scratch(name);
    let mut config = Config::dev_default();
    config.accounts = accounts_config(control);
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let accounts = SystemAccounts::new(config.accounts.settings())
        .with_probe(probe)
        .with_runner(runner)
        .with_daemon_user("panel");
    let daemon = Daemon::new(config, paths.clone(), logger).with_accounts(accounts);
    (Arc::new(daemon), paths)
}

fn state(daemon: Arc<Daemon>) -> AppState {
    let config_path = scratch("accounts-api-state");
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

#[tokio::test]
async fn the_http_inventory_exposes_users_groups_and_policy() {
    let runner = Arc::new(FakeRunner::new(vec![]));
    let (daemon, _paths) = daemon(
        "accounts-api-list",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/accounts")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("accounts");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["control_enabled"], true);
    assert_eq!(body["password_enabled"], true);
    assert_eq!(body["users"][1]["name"], "alice");
    assert_eq!(body["users"][1]["locked"], false);
    assert_eq!(body["users"][1]["groups"][0], "wheel");
    assert_eq!(body["groups"][1]["name"], "wheel");
    assert!(runner.calls().is_empty());
}

#[tokio::test]
async fn the_http_create_route_runs_useradd_with_the_expected_argv() {
    let runner = Arc::new(FakeRunner::new(vec![]));
    let (daemon, _paths) = daemon(
        "accounts-api-create",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));
    let response = router
        .oneshot(post(
            "/api/accounts/users",
            json!({ "name": "carol", "gecos": "Carol", "uid": 1500 }),
        ))
        .await
        .expect("create");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["message"], "created user carol");
    let calls = runner.calls();
    assert_eq!(calls[0].0, "useradd");
    assert_eq!(
        calls[0].1,
        vec![
            "--create-home",
            "--uid",
            "1500",
            "--comment",
            "Carol",
            "--home-dir",
            "/home/carol",
            "--shell",
            "/bin/sh",
            "carol"
        ]
    );
    assert!(calls[0].2.is_none());
}

#[tokio::test]
async fn the_http_password_route_sends_chpasswd_on_stdin() {
    let runner = Arc::new(FakeRunner::new(vec![]));
    let (daemon, _paths) = daemon(
        "accounts-api-password",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));
    let response = router
        .oneshot(post(
            "/api/accounts/users/password",
            json!({ "name": "bob", "password": "long enough" }),
        ))
        .await
        .expect("password");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "set the password for bob"
    );
    let calls = runner.calls();
    assert_eq!(calls[0].0, "chpasswd");
    assert!(calls[0].1.is_empty());
    assert_eq!(calls[0].2.as_deref(), Some("bob:long enough\n"));
}

#[tokio::test]
async fn the_http_delete_group_and_user_routes_use_the_scripted_runner() {
    let runner = Arc::new(FakeRunner::new(vec![]));
    let (daemon, _paths) = daemon(
        "accounts-api-delete",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));
    let response = router
        .clone()
        .oneshot(post(
            "/api/accounts/users/delete",
            json!({ "name": "bob", "remove_home": true }),
        ))
        .await
        .expect("delete user");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["message"], "deleted user bob");

    let response = router
        .oneshot(post(
            "/api/accounts/groups",
            json!({ "name": "builders", "gid": 1500 }),
        ))
        .await
        .expect("create group");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "created group builders"
    );

    let calls = runner.calls();
    assert_eq!(
        calls[0],
        (
            "userdel".to_string(),
            vec!["--remove".to_string(), "bob".to_string()],
            None
        )
    );
    assert_eq!(
        calls[1],
        (
            "groupadd".to_string(),
            vec![
                "--gid".to_string(),
                "1500".to_string(),
                "builders".to_string()
            ],
            None
        )
    );
}

#[tokio::test]
async fn the_http_guards_refuse_the_last_administrator_and_disabled_control() {
    let runner = Arc::new(FakeRunner::new(vec![]));
    let (guarded_daemon, _paths) = daemon(
        "accounts-api-guard",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(guarded_daemon));
    let response = router
        .oneshot(post(
            "/api/accounts/users/delete",
            json!({ "name": "alice" }),
        ))
        .await
        .expect("delete admin");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "account_protected"
    );
    assert!(runner.calls().is_empty());

    let disabled_runner = Arc::new(FakeRunner::new(vec![]));
    let (disabled_daemon, _paths) = daemon(
        "accounts-api-disabled",
        false,
        Arc::new(FakeProbe::new()),
        Arc::clone(&disabled_runner),
    );
    let response = api::router(state(disabled_daemon))
        .oneshot(post(
            "/api/accounts/groups/delete",
            json!({ "name": "bob" }),
        ))
        .await
        .expect("disabled");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "accounts_disabled"
    );
    assert!(disabled_runner.calls().is_empty());
}

#[tokio::test]
async fn the_socket_uses_the_same_account_dispatch() {
    let runner = Arc::new(FakeRunner::new(vec![]));
    let (daemon, paths) = daemon(
        "accounts-api-socket",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let listener = socket::bind(&paths.socket).expect("bind");
    let router = common::local_router(Arc::clone(&daemon), &paths);
    let server = tokio::spawn(socket::serve(listener, router));

    let client = anth_client::Client::unix(&paths.socket);
    let snapshot = client.accounts().await.expect("list");
    assert_eq!(snapshot.users.len(), 3);
    let message = client
        .account_group_delete(&anth_wire::AccountGroupDelete {
            name: "scratch".to_string(),
        })
        .await
        .expect("delete group");
    assert_eq!(message.message, "deleted group scratch");
    assert_eq!(runner.calls()[0].0, "groupdel");
    assert_eq!(runner.calls()[0].1, vec!["scratch"]);

    server.abort();
}
