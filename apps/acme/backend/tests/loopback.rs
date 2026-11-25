//! Loopback issuance against a local RFC 8555 fixture.
//!
//! This is the automated gate for A1–A2: the real ACME service process, a CA
//! that verifies the JWS and fetches the HTTP-01 token, a token server and a
//! fake `certs.store-issued` broker. Pebble stays the manual QA row
//! (`scripts/acme-pebble.sh`).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use rcgen::{BasicConstraints, Certificate, CertificateParams, IsCa, KeyPair};
use serde_json::{json, Value};

const CLIENT: &str = "anthracite:acme/client@1.0.0";

struct Ca {
    base: String,
    nonce: AtomicU64,
    nonces: Mutex<HashSet<String>>,
    jwk: Mutex<Option<Value>>,
    authz: Mutex<String>,
    names: Mutex<Vec<String>>,
    challenge: Mutex<String>,
    certificate: Mutex<Option<String>>,
    revoked: AtomicBool,
    token_port: u16,
    authority: Mutex<(Certificate, KeyPair)>,
}

impl Ca {
    fn nonce(&self) -> String {
        let value = self.nonce.fetch_add(1, Ordering::SeqCst);
        let nonce = format!("nonce-{value}");
        self.nonces.lock().expect("nonce").insert(nonce.clone());
        nonce
    }

    fn consume(&self, nonce: &str) -> bool {
        self.nonces.lock().expect("nonce").remove(nonce)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_http01_and_manual_dns_order_lands_in_the_store() {
    let dir = scratch("acme-loopback");
    let data = dir.join("data");
    let shared = dir.join("shared");
    let token_dir = shared.join("acme/http-01");
    std::fs::create_dir_all(&token_dir).expect("token dir");

    let token_port = start_token_server(token_dir.clone());
    let (base, ca) = start_ca(token_port);

    let socket = dir.join("acme.sock");
    let broker_socket = dir.join("broker.sock");
    let store = start_broker(&broker_socket);

    let config = json!({
        "http-01-port": token_port,
        "renew-interval-secs": 3600,
        "directories": [
            {
                "id": "fixture",
                "label": "Loopback fixture",
                "url": format!("{base}/directory"),
                "staging": false,
                "allow-private": true
            },
            {
                "id": "fixture-staging",
                "label": "Loopback staging",
                "url": format!("{base}/staging-directory"),
                "staging": true,
                "allow-private": true
            }
        ],
        "solvers": [{"id": "manual", "kind": "manual"}]
    });
    std::fs::create_dir_all(&data).expect("data");
    std::fs::write(
        data.join("config.json"),
        serde_json::to_vec_pretty(&config).expect("config"),
    )
    .expect("write config");

    let child = start_service(&socket, &data, &shared, &broker_socket);
    wait_socket(&socket);
    let broker = anth_service::Broker::new(&socket);

    let account = call(
        &broker,
        "create-account",
        json!({
            "directory": format!("{base}/directory"),
            "contact": ["ops@example.com"],
            "key-type": "ecdsa-p256",
        }),
    )
    .await
    .expect("account");
    let account_id = account["id"].as_str().expect("account id").to_string();
    assert!(!account.to_string().contains("PRIVATE"));
    assert_eq!(account["directory"], format!("{base}/directory"));

    let order = call(
        &broker,
        "issue",
        json!({
            "account": account_id,
            "names": ["localhost"],
            "challenge": "http-01",
            "key-type": "ecdsa-p256",
            "auto-renew": true,
            "staging": false,
        }),
    )
    .await
    .expect("issue");
    let order_id = order["id"].as_str().expect("order id").to_string();
    let done = wait_order(&broker, &order_id).await;
    assert_eq!(done["status"], "valid", "{done}");
    assert_eq!(done["certificate"], "cert-1");

    // The store received the real key and the order file kept only public data.
    let stored = store.captured.lock().expect("captured").clone();
    assert_eq!(stored.len(), 1);
    assert!(stored[0]["key-pem"]
        .as_str()
        .expect("key")
        .contains("PRIVATE KEY"));
    assert!(stored[0]["cert-pem"]
        .as_str()
        .expect("cert")
        .contains("CERTIFICATE"));
    let order_file = std::fs::read_to_string(data.join("orders").join(format!("{order_id}.json")))
        .expect("order file");
    assert!(!order_file.contains("PRIVATE KEY"));
    assert!(!order_file.contains("account.key"));
    assert!(!data
        .join("orders")
        .join(&order_id)
        .join("privkey.pem")
        .exists());
    assert!(data
        .join("orders")
        .join(&order_id)
        .join("chain.pem")
        .exists());
    assert!(std::fs::read_dir(&token_dir)
        .expect("token dir")
        .next()
        .is_none());
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(data.join("orders").join(&order_id).join("chain.pem"))
                .expect("chain")
                .permissions()
        ) & 0o777,
        0o600
    );

    // Nothing on the wire exposes the account key.
    let accounts = call(&broker, "list-accounts", json!({}))
        .await
        .expect("accounts");
    assert!(!accounts.to_string().contains("PRIVATE"));
    assert!(!accounts.to_string().contains("account/1"));

    // An identical order is refused while the certificate is valid.
    let duplicate = call(
        &broker,
        "issue",
        json!({
            "account": account_id,
            "names": ["localhost"],
            "challenge": "http-01",
            "key-type": "ecdsa-p256",
            "auto-renew": true,
            "staging": false,
        }),
    )
    .await;
    assert!(duplicate.is_err(), "the duplicate order was accepted");

    // Revocation uses the account key and the stored chain.
    let revoked = call(
        &broker,
        "revoke",
        json!({"certificate": "cert-1", "reason": "superseded"}),
    )
    .await
    .expect("revoke");
    assert_eq!(revoked, json!(true));
    assert!(ca.revoked.load(Ordering::SeqCst));

    // Manual DNS-01: the order waits for the operator, then completes.
    ca.authz
        .lock()
        .expect("authz")
        .clone_from(&"pending".to_string());
    ca.challenge
        .lock()
        .expect("challenge")
        .clone_from(&"dns-01".to_string());
    let dns = call(
        &broker,
        "issue",
        json!({
            "account": account_id,
            "names": ["*.example.com"],
            "challenge": "dns-01",
            "solver": "manual",
            "key-type": "ecdsa-p256",
            "auto-renew": false,
            "staging": false,
        }),
    )
    .await
    .expect("dns order");
    let dns_id = dns["id"].as_str().expect("dns id").to_string();
    let event = wait_event(&data, &dns_id, "challenge-ready").await;
    let detail = event["detail"].as_str().expect("detail");
    assert!(detail.contains("_acme-challenge.example.com"), "{detail}");
    assert!(detail.contains("confirm"), "{detail}");
    let confirmed = call(&broker, "confirm-challenge", json!({"order": dns_id}))
        .await
        .expect("confirm");
    assert_eq!(confirmed["challenge"], "dns-01");
    let done = wait_order(&broker, &dns_id).await;
    assert_eq!(done["status"], "valid", "{done}");

    // A staging account can never issue a production order.
    let staging = call(
        &broker,
        "create-account",
        json!({
            "directory": format!("{base}/staging-directory"),
            "contact": [],
            "key-type": "ecdsa-p256",
        }),
    )
    .await
    .expect("staging account");
    let staging_id = staging["id"].as_str().expect("staging id").to_string();
    let mismatch = call(
        &broker,
        "issue",
        json!({
            "account": staging["id"],
            "names": ["staging.test"],
            "challenge": "http-01",
            "key-type": "ecdsa-p256",
            "auto-renew": false,
            "staging": false,
        }),
    )
    .await;
    assert!(
        mismatch.is_err(),
        "a staging directory issued a production order"
    );

    // The renewal loop: a due certificate creates exactly one order, and a
    // second pass skips it once the renewal is valid.
    drop(child);
    *store.inventory.lock().expect("inventory") = json!([{
        "id": "renew-me",
        "names": ["localhost"],
        "source": "acme",
        "auto-renew": true,
        "not-before": now_secs() - 90 * 86_400,
        "not-after": now_secs() + 5 * 86_400,
        "status": "valid"
    }]);
    let config = acme_service::config::Config::load(&data).expect("config");
    let http = acme_service::http::Http::new(None).expect("http");
    let orders = acme_service::order::Orders::load(&data).expect("orders");
    let core = Arc::new(acme_service::Core {
        data: data.clone(),
        shared: shared.clone(),
        http01_root: token_dir.clone(),
        config,
        orders,
        http,
        broker: Some(anth_service::Broker::new(&broker_socket)),
        handle: tokio::runtime::Handle::current(),
        renew_backoff: std::sync::Mutex::new(std::collections::HashMap::new()),
    });
    core.orders.insert(acme_service::order::Order {
        id: "maps-renew-me".to_string(),
        account: staging_id.clone(),
        names: vec!["localhost".to_string()],
        challenge: acme_service::order::Challenge::Http01,
        solver: None,
        key_type: "ecdsa-p256".to_string(),
        auto_renew: true,
        staging: false,
        directory: format!("{base}/staging-directory"),
        status: acme_service::order::Status::Valid,
        expires_at: now_secs() + 3_600,
        certificate: Some("renew-me".to_string()),
        last_error: None,
        order_url: None,
        finalize_url: None,
        certificate_url: None,
        renew_of: None,
        confirmed: false,
        events: Vec::new(),
        created_at: now_secs(),
        updated_at: now_secs(),
    });
    let started = acme_service::renew::run_once(&core)
        .await
        .expect("renewal pass");
    assert_eq!(started, 1, "the due certificate did not start a renewal");
    let mut renewal = None;
    for _ in 0..100 {
        if let Some(order) = core
            .orders
            .list()
            .into_iter()
            .find(|order| order.renew_of.as_deref() == Some("renew-me"))
        {
            let done = matches!(
                order.status,
                acme_service::order::Status::Valid
                    | acme_service::order::Status::IssuedUnstored
                    | acme_service::order::Status::Invalid
            );
            if done {
                renewal = Some(order);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let renewal = renewal.expect("the renewal did not finish");
    assert_eq!(
        renewal.status,
        acme_service::order::Status::Valid,
        "{renewal:?}"
    );
    let again = acme_service::renew::run_once(&core)
        .await
        .expect("second pass");
    assert_eq!(again, 0, "the replaced certificate was renewed again");
    let count = core
        .orders
        .list()
        .iter()
        .filter(|order| order.renew_of.as_deref() == Some("renew-me"))
        .count();
    assert_eq!(count, 1);
    assert!(store
        .auto_renew
        .lock()
        .expect("auto renew")
        .iter()
        .any(|call| call["id"] == "renew-me" && call["enabled"] == false));

    let _ = std::fs::remove_dir_all(&dir);
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs())
        .unwrap_or(0)
}

async fn call(
    broker: &anth_service::Broker,
    function: &str,
    input: Value,
) -> Result<Value, anth_contract::Error> {
    broker.call(CLIENT, function, input).await
}

async fn wait_order(broker: &anth_service::Broker, id: &str) -> Value {
    for _ in 0..150 {
        let orders = call(broker, "list-orders", json!({}))
            .await
            .expect("orders");
        if let Some(order) = orders
            .as_array()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["id"].as_str() == Some(id))
            })
            .cloned()
        {
            let status = order["status"].as_str().unwrap_or("");
            if matches!(status, "valid" | "invalid" | "issued-unstored") {
                return order;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("the order {id} did not finish");
}

async fn wait_event(data: &Path, id: &str, kind: &str) -> Value {
    let path = data.join("orders").join(format!("{id}.json"));
    for _ in 0..100 {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(order) = serde_json::from_str::<Value>(&raw) {
                if let Some(event) = order["events"]
                    .as_array()
                    .and_then(|entries| {
                        entries
                            .iter()
                            .find(|entry| entry["kind"].as_str() == Some(kind))
                    })
                    .cloned()
                {
                    return event;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("the order {id} did not emit {kind}");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("anthracite-panel")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_service(socket: &Path, data: &Path, shared: &Path, broker: &Path) -> ChildGuard {
    let binary =
        std::env::var("CARGO_BIN_EXE_acme-service").expect("the test binary path is set by cargo");
    let child = Command::new(binary)
        .arg("--socket")
        .arg(socket)
        .arg("--data")
        .arg(data)
        .arg("--shared")
        .arg(shared)
        .arg("--broker")
        .arg(broker)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start the ACME service");
    ChildGuard(child)
}

fn wait_socket(socket: &Path) {
    for _ in 0..200 {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the ACME service did not bind {}", socket.display());
}

fn start_token_server(root: PathBuf) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("token listener");
    let port = listener.local_addr().expect("token address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
            std::thread::spawn(move || {
                let Ok((method, path, _, _)) = read_request(stream.try_clone().expect("clone"))
                else {
                    return;
                };
                let token = path
                    .strip_prefix("/.well-known/acme-challenge/")
                    .unwrap_or("");
                let body = if method == "GET" && !token.is_empty() && !token.contains('/') {
                    std::fs::read(root.join(token)).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let status = if body.is_empty() { 404 } else { 200 };
                respond(stream, status, &[], &body, false);
            });
        }
    });
    port
}

fn start_ca(token_port: u16) -> (String, Arc<Ca>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("ca listener");
    let port = listener.local_addr().expect("ca address").port();
    let base = format!("http://127.0.0.1:{port}");
    let authority = fixture_authority();
    let ca = Arc::new(Ca {
        base: base.clone(),
        nonce: AtomicU64::new(1),
        nonces: Mutex::new(HashSet::new()),
        jwk: Mutex::new(None),
        authz: Mutex::new("pending".to_string()),
        names: Mutex::new(Vec::new()),
        challenge: Mutex::new("http-01".to_string()),
        certificate: Mutex::new(None),
        revoked: AtomicBool::new(false),
        token_port,
        authority: Mutex::new(authority),
    });
    let state = Arc::clone(&ca);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&state);
            std::thread::spawn(move || handle_ca(stream, state));
        }
    });
    (base, ca)
}

fn fixture_authority() -> (Certificate, KeyPair) {
    let mut params = CertificateParams::new(Vec::new()).expect("ca");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Anthracite loopback CA");
    let key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).expect("ca key");
    let cert = params.self_signed(&key).expect("ca cert");
    (cert, key)
}

fn handle_ca(stream: TcpStream, ca: Arc<Ca>) {
    let Ok((method, path, _headers, body)) = read_request(stream.try_clone().expect("clone"))
    else {
        return;
    };
    let path = path.split('?').next().unwrap_or(&path).to_string();
    let nonce = ca.nonce();
    let replay = ("replay-nonce".to_string(), nonce);
    match (method.as_str(), path.as_str()) {
        ("GET", "/directory") | ("GET", "/staging-directory") => {
            let body = serde_json::to_vec(&json!({
                "newNonce": format!("{}/nonce", ca.base),
                "newAccount": format!("{}/new-account", ca.base),
                "newOrder": format!("{}/new-order", ca.base),
                "revokeCert": format!("{}/revoke-cert", ca.base),
            }))
            .expect("directory");
            respond(
                stream,
                200,
                &[
                    replay,
                    ("content-type".to_string(), "application/json".to_string()),
                ],
                &body,
                false,
            );
        }
        ("HEAD", "/nonce") | ("GET", "/nonce") => {
            respond(stream, 200, &[replay], &[], method == "HEAD");
        }
        ("POST", "/new-account") => {
            let Ok(payload) = verify(&ca, &body, &format!("{}/new-account", ca.base)) else {
                respond(stream, 400, &[replay], b"bad jws", false);
                return;
            };
            if payload.get("termsOfServiceAgreed") != Some(&json!(true)) {
                respond(stream, 400, &[replay], b"terms", false);
                return;
            }
            let account = json!({"status": "valid"});
            respond(
                stream,
                201,
                &[
                    replay,
                    ("location".to_string(), format!("{}/account/1", ca.base)),
                    ("content-type".to_string(), "application/json".to_string()),
                ],
                &serde_json::to_vec(&account).expect("account"),
                false,
            );
        }
        ("POST", "/new-order") => {
            let Ok(payload) = verify(&ca, &body, &format!("{}/new-order", ca.base)) else {
                respond(stream, 400, &[replay], b"bad jws", false);
                return;
            };
            let names: Vec<String> = payload["identifiers"]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| entry["value"].as_str().map(|name| name.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            *ca.names.lock().expect("names") = names.clone();
            *ca.authz.lock().expect("authz") = "pending".to_string();
            let order = json!({
                "status": "pending",
                "expires": "2030-01-01T00:00:00Z",
                "identifiers": names.iter().map(|name| json!({"type": "dns", "value": name})).collect::<Vec<_>>(),
                "authorizations": [format!("{}/authz/1", ca.base)],
                "finalize": format!("{}/finalize/1", ca.base),
            });
            respond(
                stream,
                201,
                &[
                    replay,
                    ("location".to_string(), format!("{}/order/1", ca.base)),
                    ("content-type".to_string(), "application/json".to_string()),
                ],
                &serde_json::to_vec(&order).expect("order"),
                false,
            );
        }
        ("POST", "/authz/1") => {
            let _ = verify(&ca, &body, &format!("{}/authz/1", ca.base));
            let status = ca.authz.lock().expect("authz").clone();
            let authz = json!({
                "status": status,
                "identifier": {"type": "dns", "value": ca.names.lock().expect("names").first().cloned().unwrap_or_default()},
                "challenges": [
                    {"type": "http-01", "url": format!("{}/chall-http/1", ca.base), "token": "token-1", "status": status},
                    {"type": "dns-01", "url": format!("{}/chall-dns/1", ca.base), "token": "token-2", "status": status}
                ]
            });
            respond(
                stream,
                200,
                &[
                    replay,
                    ("content-type".to_string(), "application/json".to_string()),
                ],
                &serde_json::to_vec(&authz).expect("authz"),
                false,
            );
        }
        ("POST", "/chall-http/1") => {
            let _ = verify(&ca, &body, &format!("{}/chall-http/1", ca.base));
            let name = ca
                .names
                .lock()
                .expect("names")
                .first()
                .cloned()
                .unwrap_or_default();
            let jwk = ca.jwk.lock().expect("jwk").clone().expect("account jwk");
            let thumbprint = URL_SAFE_NO_PAD
                .encode(sha256(serde_json::to_string(&jwk).expect("jwk").as_bytes()));
            let expected = format!("token-1.{thumbprint}");
            match fetch_http(ca.token_port, "/.well-known/acme-challenge/token-1", &name) {
                Some(body) if body.trim() == expected => {
                    *ca.authz.lock().expect("authz") = "valid".to_string();
                    *ca.challenge.lock().expect("challenge") = "http-01".to_string();
                    let challenge = json!({"status": "valid"});
                    respond(
                        stream,
                        200,
                        &[
                            replay,
                            ("content-type".to_string(), "application/json".to_string()),
                        ],
                        &serde_json::to_vec(&challenge).expect("challenge"),
                        false,
                    );
                }
                other => {
                    let message = format!("self-check mismatch: {other:?}");
                    respond(stream, 400, &[replay], message.as_bytes(), false);
                }
            }
        }
        ("POST", "/chall-dns/1") => {
            let _ = verify(&ca, &body, &format!("{}/chall-dns/1", ca.base));
            *ca.authz.lock().expect("authz") = "valid".to_string();
            *ca.challenge.lock().expect("challenge") = "dns-01".to_string();
            let challenge = json!({"status": "valid"});
            respond(
                stream,
                200,
                &[
                    replay,
                    ("content-type".to_string(), "application/json".to_string()),
                ],
                &serde_json::to_vec(&challenge).expect("challenge"),
                false,
            );
        }
        ("POST", "/finalize/1") => {
            let Ok(payload) = verify(&ca, &body, &format!("{}/finalize/1", ca.base)) else {
                respond(stream, 400, &[replay], b"bad jws", false);
                return;
            };
            let Some(encoded) = payload["csr"].as_str() else {
                respond(stream, 400, &[replay], b"csr", false);
                return;
            };
            let Ok(der) = URL_SAFE_NO_PAD.decode(encoded) else {
                respond(stream, 400, &[replay], b"csr base64", false);
                return;
            };
            let names = ca.names.lock().expect("names").clone();
            let cert = match sign(&ca, &der, &names) {
                Ok(cert) => cert,
                Err(error) => {
                    respond(stream, 400, &[replay], error.as_bytes(), false);
                    return;
                }
            };
            let authority = ca.authority.lock().expect("authority");
            let chain = format!("{}\n{}", cert.pem(), authority.0.pem());
            drop(authority);
            *ca.certificate.lock().expect("certificate") = Some(chain);
            let order = json!({"status": "valid", "certificate": format!("{}/cert/1", ca.base)});
            respond(
                stream,
                200,
                &[
                    replay,
                    ("content-type".to_string(), "application/json".to_string()),
                ],
                &serde_json::to_vec(&order).expect("order"),
                false,
            );
        }
        ("POST", "/order/1") => {
            let _ = verify(&ca, &body, &format!("{}/order/1", ca.base));
            let order = json!({"status": "valid", "certificate": format!("{}/cert/1", ca.base)});
            respond(
                stream,
                200,
                &[
                    replay,
                    ("content-type".to_string(), "application/json".to_string()),
                ],
                &serde_json::to_vec(&order).expect("order"),
                false,
            );
        }
        ("POST", "/cert/1") => {
            let _ = verify(&ca, &body, &format!("{}/cert/1", ca.base));
            let chain = ca
                .certificate
                .lock()
                .expect("certificate")
                .clone()
                .unwrap_or_default();
            respond(
                stream,
                200,
                &[
                    replay,
                    (
                        "content-type".to_string(),
                        "application/pem-certificate-chain".to_string(),
                    ),
                ],
                chain.as_bytes(),
                false,
            );
        }
        ("POST", "/revoke-cert") => {
            let _ = verify(&ca, &body, &format!("{}/revoke-cert", ca.base));
            ca.revoked.store(true, Ordering::SeqCst);
            respond(stream, 200, &[replay], b"{}", false);
        }
        _ => {
            respond(stream, 404, &[replay], b"not found", false);
        }
    }
}

fn sign(ca: &Ca, der: &[u8], _names: &[String]) -> Result<Certificate, String> {
    let params = rcgen::CertificateSigningRequestParams::from_der(&der.into())
        .map_err(|error| format!("the CSR did not parse: {error}"))?;
    let authority = ca.authority.lock().expect("authority");
    params
        .signed_by(&authority.0, &authority.1)
        .map_err(|error| format!("signing the leaf: {error}"))
}

fn verify(ca: &Ca, body: &[u8], expected_url: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_slice(body).map_err(|error| error.to_string())?;
    let protected: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(value["protected"].as_str().ok_or("no protected")?)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let payload = URL_SAFE_NO_PAD
        .decode(value["payload"].as_str().ok_or("no payload")?)
        .map_err(|error| error.to_string())?;
    let signature = URL_SAFE_NO_PAD
        .decode(value["signature"].as_str().ok_or("no signature")?)
        .map_err(|error| error.to_string())?;
    if protected["url"].as_str() != Some(expected_url) {
        return Err("the JWS url does not match".to_string());
    }
    let nonce = protected["nonce"].as_str().ok_or("no nonce")?;
    if !ca.consume(nonce) {
        return Err("the nonce was not issued".to_string());
    }
    let jwk = match protected.get("jwk") {
        Some(jwk) => jwk.clone(),
        None => ca.jwk.lock().expect("jwk").clone().ok_or("no jwk")?,
    };
    if protected["alg"] == "ES256" {
        let crv = jwk["crv"].as_str();
        if crv != Some("P-256") {
            return Err("unexpected curve".to_string());
        }
        let x = URL_SAFE_NO_PAD
            .decode(jwk["x"].as_str().ok_or("x")?)
            .map_err(|error| error.to_string())?;
        let y = URL_SAFE_NO_PAD
            .decode(jwk["y"].as_str().ok_or("y")?)
            .map_err(|error| error.to_string())?;
        let mut point = vec![4u8];
        point.extend_from_slice(&x);
        point.extend_from_slice(&y);
        let key = VerifyingKey::from_sec1_bytes(&point).map_err(|error| error.to_string())?;
        let signature = Signature::from_slice(&signature).map_err(|error| error.to_string())?;
        let message = format!(
            "{}.{}",
            value["protected"].as_str().expect("protected"),
            value["payload"].as_str().expect("payload")
        );
        key.verify(message.as_bytes(), &signature)
            .map_err(|error| error.to_string())?;
    }
    if let Some(jwk) = protected.get("jwk") {
        *ca.jwk.lock().expect("jwk") = Some(jwk.clone());
    }
    Ok(serde_json::from_slice(&payload).unwrap_or(Value::Null))
}

fn fetch_http(port: u16, path: &str, host: &str) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    let split = answer.find("\r\n\r\n")?;
    Some(answer[split + 4..].to_string())
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

type Request = (String, String, Vec<(String, String)>, Vec<u8>);

fn read_request(mut stream: TcpStream) -> std::io::Result<Request> {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if raw.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if raw.len() > 64 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "header too large",
            ));
        }
    }
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "no header end"))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let request = lines.next().unwrap_or_default();
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    let mut length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                length = value.parse().unwrap_or(0);
            }
            headers.push((name.to_ascii_lowercase(), value));
        }
    }
    let mut body = raw[split + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    body.truncate(length);
    Ok((method, path, headers, body))
}

fn respond(
    mut stream: TcpStream,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
    head: bool,
) {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let mut out = format!("HTTP/1.1 {status} {reason}\r\n");
    out.push_str(&format!("content-length: {}\r\n", body.len()));
    out.push_str("connection: close\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    let _ = stream.write_all(out.as_bytes());
    if !head {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

struct FakeStore {
    captured: Arc<Mutex<Vec<Value>>>,
    inventory: Arc<Mutex<Value>>,
    auto_renew: Arc<Mutex<Vec<Value>>>,
}

fn start_broker(socket: &Path) -> FakeStore {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("broker socket");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let inventory = Arc::new(Mutex::new(json!([])));
    let auto_renew = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&captured);
    let stock = Arc::clone(&inventory);
    let renew = Arc::clone(&auto_renew);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = Arc::clone(&seen);
            let stock = Arc::clone(&stock);
            let renew = Arc::clone(&renew);
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut raw = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let Ok(read) = stream.read(&mut buffer) else {
                        return;
                    };
                    if read == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buffer[..read]);
                    if raw.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let split = raw
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .unwrap_or(0);
                let head = String::from_utf8_lossy(&raw[..split]).to_string();
                let mut length = 0usize;
                for line in head.lines() {
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = raw[split + 4..].to_vec();
                while body.len() < length {
                    let Ok(read) = stream.read(&mut buffer) else {
                        return;
                    };
                    if read == 0 {
                        break;
                    }
                    body.extend_from_slice(&buffer[..read]);
                }
                body.truncate(length);
                let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let function = request["function"].as_str().unwrap_or("");
                let answer = match function {
                    "store-issued" => {
                        seen.lock()
                            .expect("captured")
                            .push(request["input"].clone());
                        json!({"ok": {"id": "cert-1", "names": request["input"]["names"]}})
                    }
                    "inventory" => json!({"ok": stock.lock().expect("inventory").clone()}),
                    "set-auto-renew" => {
                        renew
                            .lock()
                            .expect("auto renew")
                            .push(request["input"].clone());
                        json!({"ok": {}})
                    }
                    _ => json!({"ok": {}}),
                };
                let body = serde_json::to_vec(&answer).expect("broker answer");
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            });
        }
    });
    FakeStore {
        captured,
        inventory,
        auto_renew,
    }
}
