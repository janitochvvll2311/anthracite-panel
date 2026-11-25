//! Loopback test against a fixture Docker Engine API over a Unix socket.
//!
//! The fixture is the CI gate for the lifecycle, the refusal of bind mounts and
//! privileged mode, logs, stats, exec and the audit shape. Docker and rootless
//! Podman stay manual QA rows.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const RUNTIME: &str = "anthracite:containers/runtime@1.0.0";

#[derive(Clone)]
struct Container {
    id: String,
    name: String,
    image: String,
    state: String,
    tty: bool,
    env: Vec<String>,
    labels: Value,
    ports: Value,
    log: Vec<u8>,
}

#[derive(Default)]
struct State {
    containers: Mutex<HashMap<String, Container>>,
    volumes: Mutex<Vec<Value>>,
    next: AtomicU64,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_logs_stats_and_exec_work_against_the_fixture() {
    let dir = scratch("containers-loopback");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).expect("data");
    let runtime_socket = dir.join("docker.sock");
    let state = Arc::new(State::default());
    let (broker_socket, audit) = start_audit(&dir.join("broker.sock"));
    let service_socket = dir.join("service.sock");
    start_fixture(&runtime_socket, Arc::clone(&state));
    let child = start_service(&service_socket, &data, &runtime_socket, &broker_socket);
    wait_socket(&service_socket);
    let broker = anth_service::Broker::new(&service_socket);

    let list = call(&broker, "list-containers", json!({}))
        .await
        .expect("list");
    assert_eq!(list.as_array().expect("list").len(), 1);
    assert_eq!(list[0]["name"], "web");
    assert_eq!(list[0]["state"], "running");

    let inspected = call(&broker, "inspect", json!({"id": "web"}))
        .await
        .expect("inspect");
    assert_eq!(inspected["image"], "nginx:1.27");
    assert_eq!(inspected["health"], "healthy");

    // A bind mount is not expressible: the volume name cannot be a path.
    let bind = call(
        &broker,
        "create",
        json!({
            "image": "nginx:1.27",
            "volumes": [{"name": "/etc", "path": "/etc"}]
        }),
    )
    .await;
    assert!(bind.is_err());
    // Privileged is an unknown field and is refused by the record.
    let privileged = call(
        &broker,
        "create",
        json!({"image": "nginx:1.27", "privileged": true}),
    )
    .await;
    assert!(privileged.is_err());

    let created = call(
        &broker,
        "create",
        json!({
            "image": "nginx:1.27",
            "name": "site",
            "command": ["nginx", "-g", "daemon off;"],
            "env": [{"name": "SECRET", "value": "do-not-audit"}],
            "ports": ["8080:80"],
            "volumes": [{"name": "site-data", "path": "/usr/share/nginx/html"}],
            "restart": "unless-stopped",
            "labels": [{"name": "com.example", "value": "site"}]
        }),
    )
    .await
    .expect("create");
    let id = created["id"].as_str().expect("id").to_string();
    assert_eq!(created["name"], "site");

    for (action, body) in [
        ("start", json!({"id": id})),
        ("stop", json!({"id": id, "timeout-s": 1})),
        ("restart", json!({"id": id, "timeout-s": 1})),
        ("pause", json!({"id": id})),
        ("unpause", json!({"id": id})),
        ("kill", json!({"id": id, "signal": "SIGTERM"})),
    ] {
        let answer = call(&broker, action, body).await.expect(action);
        assert_eq!(answer, json!(true));
    }
    let bad_signal = call(&broker, "kill", json!({"id": id, "signal": "SIGSYS"})).await;
    assert!(bad_signal.is_err());

    // Logs are decoded from Docker's multiplexed frames.
    let logs = raw_stream(
        &service_socket,
        "logs",
        json!({"id": "web", "tail": 10, "follow": false}),
        10,
    )
    .await;
    let output = logs.concat();
    assert_eq!(String::from_utf8_lossy(&output), "hello from the fixture\n");

    // Stats: one sample is enough for the wire shape.
    let stats = raw_stream(&service_socket, "stats", json!({"id": "web"}), 1).await;
    let sample: Value = serde_json::from_slice(&stats[0]).expect("sample json");
    assert!(sample["cpu-percent"].as_f64().is_some());
    assert_eq!(sample["pids"], 1);

    // Exec: create, attach, write, resize, close.
    let session = call(
        &broker,
        "exec",
        json!({"id": "web", "command": ["sh"], "tty": true, "cols": 100, "rows": 30}),
    )
    .await
    .expect("exec");
    let session = session.as_str().expect("session").to_string();
    let attach_socket = service_socket.clone();
    let attach_session = session.clone();
    let attach = tokio::spawn(async move {
        raw_stream(
            &attach_socket,
            "exec-attach",
            json!({"session": attach_session}),
            1,
        )
        .await
    });
    let written = call(
        &broker,
        "exec-write",
        json!({"session": session, "data": b"ping\n".to_vec()}),
    )
    .await
    .expect("write");
    assert_eq!(written, json!(true));
    let echoed = tokio::time::timeout(Duration::from_secs(5), attach)
        .await
        .expect("echo timeout")
        .expect("echo");
    assert_eq!(echoed.concat(), b"ping\n");
    assert_eq!(
        call(
            &broker,
            "exec-resize",
            json!({"session": session, "cols": 120, "rows": 40}),
        )
        .await
        .expect("resize"),
        json!(true)
    );
    assert_eq!(
        call(&broker, "exec-close", json!({"session": session}))
            .await
            .expect("close"),
        json!(true)
    );

    // Images, pull and volumes.
    let images = call(&broker, "list-images", json!({}))
        .await
        .expect("images");
    assert_eq!(images[0]["tags"][0], "nginx:1.27");
    let pull = raw_stream(
        &service_socket,
        "pull",
        json!({"reference": "alpine:3.21"}),
        1,
    )
    .await;
    assert!(String::from_utf8_lossy(&pull[0]).contains("Pulling"));
    let volume = call(&broker, "create-volume", json!({"name": "site-data"}))
        .await
        .expect("volume");
    assert_eq!(volume["name"], "site-data");
    let volumes = call(&broker, "list-volumes", json!({}))
        .await
        .expect("volumes");
    assert!(!volumes.as_array().expect("volumes").is_empty());
    assert_eq!(
        call(
            &broker,
            "remove-volume",
            json!({"name": "site-data", "force": true})
        )
        .await
        .expect("remove volume"),
        json!(true)
    );
    let networks = call(&broker, "list-networks", json!({}))
        .await
        .expect("networks");
    assert_eq!(networks[0]["name"], "bridge");

    // A running container needs force; a stopped one does not.
    let running = call(&broker, "remove", json!({"id": "web", "force": false})).await;
    assert!(running.is_err());
    assert_eq!(
        call(&broker, "remove", json!({"id": id, "force": true}))
            .await
            .expect("remove"),
        json!(true)
    );

    let audits = audit.lock().expect("audit");
    assert!(audits
        .iter()
        .any(|entry| entry["action"] == "containers.create"));
    let text = audits
        .iter()
        .map(|entry| entry.to_string())
        .collect::<String>();
    assert!(
        !text.contains("do-not-audit"),
        "an env value reached the audit"
    );

    drop(child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The service side of `/__anth/stream` is raw chunked bytes, not the
/// engine-broker JSON framing; a direct reader keeps the fixture honest.
async fn raw_stream(socket: &Path, function: &str, input: Value, take: usize) -> Vec<Vec<u8>> {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .expect("stream socket");
    let body = serde_json::to_vec(&json!({
        "interface": RUNTIME,
        "function": function,
        "input": input,
    }))
    .expect("stream body");
    let head = format!(
        "POST /__anth/stream HTTP/1.1\r\nHost: anthracite\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.expect("head");
    stream.write_all(&body).await.expect("stream body");
    stream.flush().await.expect("flush");
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.expect("headers") == 0 || line == "\r\n" {
            break;
        }
    }
    let mut chunks = Vec::new();
    while chunks.len() < take {
        let mut size_line = String::new();
        if reader.read_line(&mut size_line).await.expect("size") == 0 {
            break;
        }
        let size_text = size_line.trim();
        if size_text.is_empty() {
            continue;
        }
        let size = usize::from_str_radix(size_text, 16).expect("chunk size");
        if size == 0 {
            break;
        }
        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk).await.expect("chunk");
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf).await.expect("crlf");
        chunks.push(chunk);
    }
    chunks
}

async fn call(
    broker: &anth_service::Broker,
    function: &str,
    input: Value,
) -> Result<Value, anth_contract::Error> {
    broker.call(RUNTIME, function, input).await
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

fn start_service(socket: &Path, data: &Path, runtime: &Path, broker: &Path) -> ChildGuard {
    let binary = std::env::var("CARGO_BIN_EXE_containers-service").expect("binary path");
    let child = Command::new(binary)
        .arg("--socket")
        .arg(socket)
        .arg("--data")
        .arg(data)
        .arg("--broker")
        .arg(broker)
        .arg("--runtime")
        .arg(runtime)
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

fn start_audit(socket: &Path) -> (PathBuf, Arc<Mutex<Vec<Value>>>) {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("audit socket");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&captured);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = Arc::clone(&seen);
            std::thread::spawn(move || {
                let mut stream = stream;
                let Some((_, _, _, body)) = read_request(&mut stream) else {
                    return;
                };
                if let Ok(value) = serde_json::from_slice::<Value>(&body) {
                    if value["function"] == "record" {
                        seen.lock().expect("audit").push(value["input"].clone());
                    }
                }
                let payload = br#"{"ok":{}}"#;
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    payload.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(payload);
            });
        }
    });
    (socket.to_path_buf(), captured)
}

fn start_fixture(socket: &Path, state: Arc<State>) {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).expect("fixture socket");
    state.containers.lock().expect("containers").insert(
        "web".to_string(),
        Container {
            id: "web".to_string(),
            name: "web".to_string(),
            image: "nginx:1.27".to_string(),
            state: "running".to_string(),
            tty: false,
            env: Vec::new(),
            labels: json!({"com.example": "web"}),
            ports: json!([{"IP": "0.0.0.0", "PrivatePort": 80, "PublicPort": 8080, "Type": "tcp"}]),
            log: b"hello from the fixture\n".to_vec(),
        },
    );
    state
        .volumes
        .lock()
        .expect("volumes")
        .push(json!({"Name": "seed", "Driver": "local", "Labels": {}}));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&state);
            std::thread::spawn(move || handle(stream, state));
        }
    });
}

fn handle(mut stream: UnixStream, state: Arc<State>) {
    let Some((method, path, _headers, body)) = read_request(&mut stream) else {
        return;
    };
    let path_only = path.split('?').next().unwrap_or(&path).to_string();
    let query = path
        .split_once('?')
        .map(|(_, query)| query.to_string())
        .unwrap_or_default();
    let parts: Vec<&str> = path_only.split('/').collect();

    // /v1.43/...
    if parts.get(1) != Some(&"v1.43") {
        return respond(
            &mut stream,
            404,
            "application/json",
            b"{\"message\":\"bad version\"}",
        );
    }
    match (method.as_str(), parts.as_slice()) {
        ("GET", ["", "v1.43", "containers", "json"]) => {
            let list: Vec<Value> = state
                .containers
                .lock()
                .expect("containers")
                .values()
                .map(container_json)
                .collect();
            respond_json(&mut stream, &Value::Array(list));
        }
        ("POST", ["", "v1.43", "containers", "create"]) => {
            let value: Value = match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => {
                    return respond(
                        &mut stream,
                        400,
                        "application/json",
                        b"{\"message\":\"body\"}",
                    )
                }
            };
            if value["HostConfig"]["Privileged"] == json!(true) {
                return respond(
                    &mut stream,
                    400,
                    "application/json",
                    b"{\"message\":\"privileged is refused\"}",
                );
            }
            let id = format!("c{}", state.next.fetch_add(1, Ordering::SeqCst));
            let name = query.strip_prefix("name=").unwrap_or(&id).to_string();
            let env: Vec<String> = value["Env"]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| entry.as_str().map(|value| value.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let container = Container {
                id: id.clone(),
                name,
                image: value["Image"].as_str().unwrap_or("").to_string(),
                state: "created".to_string(),
                tty: false,
                env,
                labels: value["Labels"].clone(),
                ports: json!([]),
                log: Vec::new(),
            };
            state
                .containers
                .lock()
                .expect("containers")
                .insert(id.clone(), container);
            respond_json(&mut stream, &json!({"Id": id}));
        }
        ("GET", ["", "v1.43", "containers", id, "json"]) => {
            let containers = state.containers.lock().expect("containers");
            match containers.get(*id) {
                Some(container) => respond_json(
                    &mut stream,
                    &json!({
                        "Id": container.id,
                        "Name": format!("/{}", container.name),
                        "Created": "2026-11-25T12:00:00Z",
                        "Config": {
                            "Image": container.image,
                            "Tty": container.tty,
                            "Labels": container.labels,
                            "Env": container.env
                        },
                        "State": {
                            "Status": container.state,
                            "Health": {"Status": "healthy"}
                        },
                        "NetworkSettings": {"Ports": {
                            "80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}]
                        }}
                    }),
                ),
                None => respond(
                    &mut stream,
                    404,
                    "application/json",
                    b"{\"message\":\"no such container\"}",
                ),
            }
        }
        ("GET", ["", "v1.43", "containers", id, "logs"]) => {
            let log = state
                .containers
                .lock()
                .expect("containers")
                .get(*id)
                .map(|container| container.log.clone())
                .unwrap_or_default();
            let mut framed = Vec::new();
            framed.extend_from_slice(&[1, 0, 0, 0]);
            framed.extend_from_slice(&(log.len() as u32).to_be_bytes());
            framed.extend_from_slice(&log);
            chunked(&mut stream, &framed);
            let _ = stream.write_all(b"0\r\n\r\n");
        }
        ("GET", ["", "v1.43", "containers", id, "stats"]) => {
            if !state
                .containers
                .lock()
                .expect("containers")
                .contains_key(*id)
            {
                return respond(&mut stream, 404, "application/json", b"{}");
            }
            respond_json(
                &mut stream,
                &json!({
                    "cpu_stats": {"cpu_usage": {"total_usage": 200}, "system_cpu_usage": 1000, "online_cpus": 2},
                    "precpu_stats": {"cpu_usage": {"total_usage": 100}, "system_cpu_usage": 500},
                    "memory_stats": {"usage": 1000, "limit": 4096, "stats": {"cache": 200}},
                    "networks": {"eth0": {"rx_bytes": 10, "tx_bytes": 20}},
                    "pids_stats": {"current": 1}
                }),
            );
        }
        ("POST", ["", "v1.43", "containers", id, "start"])
        | ("POST", ["", "v1.43", "containers", id, "stop"])
        | ("POST", ["", "v1.43", "containers", id, "restart"])
        | ("POST", ["", "v1.43", "containers", id, "pause"])
        | ("POST", ["", "v1.43", "containers", id, "unpause"])
        | ("POST", ["", "v1.43", "containers", id, "kill"]) => {
            let action = *parts.get(5).unwrap_or(&"");
            let mut containers = state.containers.lock().expect("containers");
            if let Some(container) = containers.get_mut(*id) {
                container.state = match action {
                    "stop" => "exited",
                    _ => "running",
                }
                .to_string();
            }
            drop(containers);
            respond(&mut stream, 204, "application/json", b"");
        }
        ("DELETE", ["", "v1.43", "containers", id]) => {
            let force = query.split('&').any(|pair| pair == "force=1");
            let mut containers = state.containers.lock().expect("containers");
            let running = containers
                .get(*id)
                .map(|container| container.state == "running")
                .unwrap_or(false);
            if running && !force {
                drop(containers);
                return respond(
                    &mut stream,
                    409,
                    "application/json",
                    b"{\"message\":\"container is running\"}",
                );
            }
            containers.remove(*id);
            drop(containers);
            respond(&mut stream, 204, "application/json", b"");
        }
        ("POST", ["", "v1.43", "containers", _id, "exec"]) => {
            respond_json(&mut stream, &json!({"Id": "exec-1"}));
        }
        ("POST", ["", "v1.43", "exec", _id, "start"]) => {
            let _ = stream.write_all(
                b"HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.raw-stream\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n",
            );
            let _ = stream.flush();
            // Echo every read as one TTY frame until the service closes.
            let mut buffer = [0u8; 4096];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let _ = stream.write_all(&buffer[..read]);
                        let _ = stream.flush();
                    }
                }
            }
        }
        ("POST", ["", "v1.43", "exec", _id, "resize"]) => {
            respond(&mut stream, 204, "application/json", b"");
        }
        ("GET", ["", "v1.43", "images", "json"]) => {
            respond_json(
                &mut stream,
                &json!([{
                    "Id": "sha256:nginx",
                    "RepoTags": ["nginx:1.27"],
                    "Size": 1000,
                    "Created": 10,
                    "RepoDigests": ["nginx@sha256:abc"]
                }]),
            );
        }
        ("POST", ["", "v1.43", "images", "create"]) => {
            let body = br#"{"status":"Pulling from library/alpine"}"#;
            chunked(&mut stream, body);
            let _ = stream.write_all(b"0\r\n\r\n");
        }
        ("DELETE", ["", "v1.43", "images", _id]) => {
            respond(&mut stream, 204, "application/json", b"");
        }
        ("GET", ["", "v1.43", "volumes"]) => {
            let volumes = state.volumes.lock().expect("volumes").clone();
            respond_json(&mut stream, &json!({"Volumes": volumes}));
        }
        ("POST", ["", "v1.43", "volumes", "create"]) => {
            let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let volume = json!({
                "Name": value["Name"].as_str().unwrap_or(""),
                "Driver": "local",
                "Labels": {}
            });
            state.volumes.lock().expect("volumes").push(volume.clone());
            respond_json(&mut stream, &volume);
        }
        ("DELETE", ["", "v1.43", "volumes", name]) => {
            state
                .volumes
                .lock()
                .expect("volumes")
                .retain(|volume| volume["Name"].as_str() != Some(*name));
            respond(&mut stream, 204, "application/json", b"");
        }
        ("GET", ["", "v1.43", "networks"]) => {
            respond_json(
                &mut stream,
                &json!([{
                    "Name": "bridge",
                    "Driver": "bridge",
                    "IPAM": {"Config": [{"Subnet": "172.17.0.0/16"}]},
                    "Containers": {"web": {}}
                }]),
            );
        }
        _ => respond(
            &mut stream,
            404,
            "application/json",
            b"{\"message\":\"not found\"}",
        ),
    }
}

fn container_json(container: &Container) -> Value {
    json!({
        "Id": container.id,
        "Names": [format!("/{}", container.name)],
        "Image": container.image,
        "State": container.state,
        "Status": "Up",
        "Created": 10,
        "Ports": container.ports,
        "Labels": container.labels
    })
}

type Request = (String, String, Vec<(String, String)>, Vec<u8>);

fn read_request(stream: &mut UnixStream) -> Option<Request> {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..read]);
        if raw.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if raw.len() > 64 * 1024 {
            return None;
        }
    }
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let mut parts = lines.next()?.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
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
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buffer[..read]);
    }
    body.truncate(length);
    Some((method, path, headers, body))
}

fn respond(stream: &mut UnixStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    head.extend_from_slice(body);
    let _ = stream.write_all(&head);
    let _ = stream.flush();
}

fn respond_json(stream: &mut UnixStream, value: &Value) {
    respond(
        stream,
        200,
        "application/json",
        &serde_json::to_vec(value).expect("json"),
    );
}

fn chunked(stream: &mut UnixStream, body: &[u8]) {
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
    );
    let _ = stream.write_all(format!("{:x}\r\n", body.len()).as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.write_all(b"\r\n");
    let _ = stream.flush();
}
