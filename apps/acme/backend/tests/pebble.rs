//! The real Pebble gate for HTTP-01.
//!
//! Ignored by default: it needs a running Pebble and its CA bundle. Run
//! `scripts/acme-pebble.sh`, start the printed command, then run this test with
//! the environment the script prints. The loopback test is the CI gate.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

const CLIENT: &str = "anthracite:acme/client@1.0.0";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs Pebble; run scripts/acme-pebble.sh"]
async fn pebble_http01_issues_and_stores() {
    let Ok(directory) = std::env::var("ANTH_PEBBLE_DIRECTORY") else {
        eprintln!("ANTH_PEBBLE_DIRECTORY is not set; see scripts/acme-pebble.sh");
        return;
    };
    let ca_bundle = std::env::var("ANTH_PEBBLE_CA").expect("ANTH_PEBBLE_CA");
    let dir = scratch("acme-pebble");
    let data = dir.join("data");
    let shared = dir.join("shared");
    let token_dir = shared.join("acme/http-01");
    std::fs::create_dir_all(&token_dir).expect("token dir");
    std::fs::create_dir_all(&data).expect("data");

    // Pebble's httpPort is 5002 in the config written by the script.
    let token_port = start_token_server(token_dir.clone(), 5002);
    let socket = dir.join("acme.sock");
    let broker_socket = dir.join("broker.sock");
    let captured = start_broker(&broker_socket);
    let config = json!({
        "http-01-port": token_port,
        "renew-interval-secs": 3600,
        "directories": [{
            "id": "pebble",
            "label": "Pebble",
            "url": directory,
            "staging": false,
            "allow-private": true
        }],
        "ca-bundle": ca_bundle,
        "solvers": [{"id": "manual", "kind": "manual"}]
    });
    std::fs::write(
        data.join("config.json"),
        serde_json::to_vec_pretty(&config).expect("config"),
    )
    .expect("config file");

    let _child = start_service(&socket, &data, &shared, &broker_socket);
    wait_socket(&socket);
    let broker = anth_service::Broker::new(&socket);
    let account = call(
        &broker,
        "create-account",
        json!({"directory": directory, "contact": [], "key-type": "ecdsa-p256"}),
    )
    .await
    .expect("account");
    let order = call(
        &broker,
        "issue",
        json!({
            "account": account["id"],
            "names": ["localhost"],
            "challenge": "http-01",
            "key-type": "ecdsa-p256",
            "auto-renew": false,
            "staging": false,
        }),
    )
    .await
    .expect("issue");
    let id = order["id"].as_str().expect("order id").to_string();
    let mut last = Value::Null;
    for _ in 0..200 {
        let orders = call(&broker, "list-orders", json!({}))
            .await
            .expect("orders");
        if let Some(order) = orders.as_array().and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["id"].as_str() == Some(&id))
        }) {
            last = order.clone();
            if matches!(
                order["status"].as_str().unwrap_or(""),
                "valid" | "invalid" | "issued-unstored"
            ) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    assert_eq!(last["status"], "valid", "{last}");
    assert_eq!(captured.lock().expect("captured").len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

async fn call(
    broker: &anth_service::Broker,
    function: &str,
    input: Value,
) -> Result<Value, anth_contract::Error> {
    broker.call(CLIENT, function, input).await
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
    let binary = std::env::var("CARGO_BIN_EXE_acme-service").expect("binary path");
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
        .expect("start the service");
    ChildGuard(child)
}

fn wait_socket(socket: &Path) {
    for _ in 0..200 {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the service did not bind {}", socket.display());
}

fn start_token_server(root: PathBuf, port: u16) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("token listener on 5002");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let root = root.clone();
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
                let head = String::from_utf8_lossy(&raw).to_string();
                let path = head
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_string();
                let token = path
                    .strip_prefix("/.well-known/acme-challenge/")
                    .unwrap_or("");
                let body = if !token.is_empty() && !token.contains('/') {
                    std::fs::read(root.join(token)).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let status = if body.is_empty() { 404 } else { 200 };
                let response = format!(
                    "HTTP/1.1 {status} {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    if status == 200 { "OK" } else { "Not Found" },
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    port
}

fn start_broker(socket: &Path) -> Arc<Mutex<Vec<Value>>> {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("broker socket");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&captured);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = Arc::clone(&seen);
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
                let answer = if request["function"] == "store-issued" {
                    seen.lock()
                        .expect("captured")
                        .push(request["input"].clone());
                    json!({"ok": {"id": "cert-1"}})
                } else {
                    json!({"ok": {}})
                };
                let body = serde_json::to_vec(&answer).expect("answer");
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    captured
}
