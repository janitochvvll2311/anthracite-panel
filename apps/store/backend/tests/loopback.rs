//! Loopback catalogue: a signed native index, a GitHub-shaped release, a local
//! source and the digest-pinned download.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const STORE: &str = "anthracite:store/catalog@1.0.0";

struct Fixture {
    index: Arc<Vec<u8>>,
    signature: String,
    bundle: Vec<u8>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signed_index_searches_and_downloads_verified_bytes() {
    let dir = scratch("store-loopback");
    let data = dir.join("data");
    let shared = dir.join("shared");
    std::fs::create_dir_all(data.join("cache")).expect("data");
    std::fs::create_dir_all(shared.join("store")).expect("shared");

    let key = SigningKey::from_bytes(&[7u8; 32]);
    let public = key.verifying_key().to_bytes();
    let public_hex: String = public.iter().map(|byte| format!("{byte:02x}")).collect();
    let bundle = b"PK\x03\x04 a fake bundle".to_vec();
    let digest: String = {
        use sha2::{Digest, Sha256};
        Sha256::digest(&bundle)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    let index = index_bytes(&digest, &bundle);
    let signature = sign(&key, &index);
    let index_for_tamper = index.clone();
    let signature_for_local = signature.clone();
    let base = start_fixture(Arc::new(Fixture {
        index: Arc::new(index.clone()),
        signature,
        bundle: bundle.clone(),
    }));

    // A local source with the same signed index.
    let local = shared.join("store/airgap");
    std::fs::create_dir_all(&local).expect("local");
    std::fs::write(local.join("index.json"), &index).expect("local index");
    std::fs::write(local.join("index.json.sig"), &signature_for_local).expect("local sig");
    std::fs::write(local.join("notes-1.0.0.anth"), &bundle).expect("local bundle");

    let config = json!({
        "sources": [
            {
                "id": "native",
                "name": "Native",
                "kind": "native",
                "url": base,
                "public-key": public_hex,
                "allow-private": true
            },
            {
                "id": "local",
                "name": "Airgap",
                "kind": "local",
                "url": "airgap",
                "public-key": public_hex
            },
            {
                "id": "github",
                "name": "GitHub",
                "kind": "github",
                "url": format!("{base}/gh/releases/latest"),
                "public-key": public_hex,
                "allow-private": true
            }
        ]
    });
    std::fs::write(
        data.join("sources.json"),
        serde_json::to_vec_pretty(&config).expect("config"),
    )
    .expect("sources");

    let socket = dir.join("store.sock");
    let child = start_service(&socket, &data, &shared);
    wait_socket(&socket);
    let broker = anth_service::Broker::new(&socket);

    let sources = call(&broker, "sources", json!({})).await.expect("sources");
    assert_eq!(sources.as_array().expect("sources").len(), 3);
    assert_eq!(sources[0]["trusted"], false);

    let refreshed = call(&broker, "refresh", json!({"source": "native"}))
        .await
        .expect("refresh");
    assert_eq!(refreshed["trusted"], true);
    let github = call(&broker, "refresh", json!({"source": "github"}))
        .await
        .expect("github refresh");
    assert_eq!(github["trusted"], true);

    let results = call(&broker, "search", json!({"query": "notes", "limit": 10}))
        .await
        .expect("search");
    assert_eq!(results[0]["id"], "org.example.notes");
    assert_eq!(results[0]["latest"], "1.0.0");

    let detail = call(&broker, "app", json!({"id": "org.example.notes"}))
        .await
        .expect("app");
    assert_eq!(detail["publisher"], "Example");
    let versions = call(&broker, "versions", json!({"id": "org.example.notes"}))
        .await
        .expect("versions");
    assert_eq!(versions.as_array().expect("versions").len(), 1);

    let updates = call(
        &broker,
        "check-updates",
        json!({"installed": [{"id": "org.example.notes", "version": "0.9.0"}]}),
    )
    .await
    .expect("updates");
    assert_eq!(updates[0]["latest"], "1.0.0");

    let download = raw_stream(
        &socket,
        "download",
        json!({"id": "org.example.notes", "version": "1.0.0"}),
        10,
    )
    .await;
    assert_eq!(download.concat(), bundle);

    // A tampered index is refused and the previous cache stays.
    let mut bad_value = serde_json::from_slice::<Value>(&index_for_tamper).expect("index");
    bad_value["apps"][0]["name"] = json!("Tampered");
    let bad = serde_json::to_vec(&bad_value).expect("bad index");
    let bad_sig = sign(&key, b"not the index");
    std::fs::write(
        data.join("sources.json"),
        serde_json::to_vec_pretty(&json!({
            "sources": [
                {"id":"native","name":"Native","kind":"native","url":base,"public-key":public_hex,"allow-private":true},
                {"id":"local","name":"Airgap","kind":"local","url":"airgap","public-key":public_hex}
            ]
        }))
        .expect("config"),
    )
    .expect("sources");
    // Point the fixture at the tampered pair and refresh again in a new service.
    drop(child);
    let tampered_base = start_fixture(Arc::new(Fixture {
        index: Arc::new(bad),
        signature: bad_sig,
        bundle: bundle.clone(),
    }));
    let mut config =
        serde_json::from_slice::<Value>(&std::fs::read(data.join("sources.json")).expect("config"))
            .expect("config");
    config["sources"][0]["url"] = json!(tampered_base);
    std::fs::write(
        data.join("sources.json"),
        serde_json::to_vec_pretty(&config).expect("config"),
    )
    .expect("sources");
    let socket = dir.join("store2.sock");
    let child = start_service(&socket, &data, &shared);
    wait_socket(&socket);
    let broker = anth_service::Broker::new(&socket);
    let refused = call(&broker, "refresh", json!({"source": "native"})).await;
    assert!(refused.is_err(), "a tampered signature was accepted");
    // The cached catalogue is still there.
    let results = call(&broker, "search", json!({"query": "notes", "limit": 10}))
        .await
        .expect("search");
    assert_eq!(results[0]["id"], "org.example.notes");

    // A replay with an older sequence is refused and the cache is kept.
    let mut old = serde_json::from_slice::<Value>(&index_for_tamper).expect("index");
    old["sequence"] = json!(1);
    old["apps"][0]["name"] = json!("Old");
    let old = serde_json::to_vec(&old).expect("old index");
    let old_sig = sign(&key, &old);
    let old_base = start_fixture(Arc::new(Fixture {
        index: Arc::new(old),
        signature: old_sig,
        bundle: bundle.clone(),
    }));
    config["sources"][0]["url"] = json!(old_base);
    std::fs::write(
        data.join("sources.json"),
        serde_json::to_vec_pretty(&config).expect("config"),
    )
    .expect("sources");
    let replay_socket = dir.join("store-replay.sock");
    let replay = start_service(&replay_socket, &data, &shared);
    wait_socket(&replay_socket);
    let replay_broker = anth_service::Broker::new(&replay_socket);
    let replay_error = call(&replay_broker, "refresh", json!({"source": "native"})).await;
    assert!(replay_error.is_err(), "an older sequence was accepted");
    drop(replay);

    // A digest mismatch never reaches the manager.
    let mut bad = serde_json::from_slice::<Value>(&index_for_tamper).expect("index");
    bad["apps"][0]["versions"][0]["sha256"] = json!("ff".repeat(32));
    let bad = serde_json::to_vec(&bad).expect("bad index");
    let bad_sig = sign(&key, &bad);
    let mismatched = shared.join("store/mismatch");
    std::fs::create_dir_all(&mismatched).expect("local");
    std::fs::write(mismatched.join("index.json"), &bad).expect("index");
    std::fs::write(mismatched.join("index.json.sig"), &bad_sig).expect("sig");
    std::fs::write(mismatched.join("notes-1.0.0.anth"), &bundle).expect("bundle");
    let mut config =
        serde_json::from_slice::<Value>(&std::fs::read(data.join("sources.json")).expect("config"))
            .expect("config");
    config["sources"] = json!([
        {"id":"mismatch","name":"Mismatch","kind":"local","url":"mismatch","public-key":public_hex}
    ]);
    std::fs::write(
        data.join("sources.json"),
        serde_json::to_vec_pretty(&config).expect("config"),
    )
    .expect("sources");
    let socket = dir.join("store3.sock");
    let child2 = start_service(&socket, &data, &shared);
    wait_socket(&socket);
    let broker = anth_service::Broker::new(&socket);
    call(&broker, "refresh", json!({"source": "mismatch"}))
        .await
        .expect("refresh");
    let error = raw_stream_optional(
        &socket,
        "download",
        json!({"id": "org.example.notes", "version": "1.0.0"}),
    )
    .await;
    assert!(error.is_err(), "a digest mismatch was streamed");

    drop(child);
    drop(child2);
    let _ = std::fs::remove_dir_all(&dir);
}

fn index_bytes(digest: &str, bundle: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": 1,
        "sequence": 2,
        "store": {"id": "org.example.store", "name": "Example"},
        "apps": [{
            "id": "org.example.notes",
            "name": "Notes",
            "summary": "A tiny notes app.",
            "description": "Notes with a tiny editor.",
            "publisher": "Example",
            "license": "Apache-2.0",
            "categories": ["productivity"],
            "versions": [{
                "version": "1.0.0",
                "channel": "stable",
                "published_at": 1,
                "sha256": digest,
                "size": bundle.len(),
                "bundle_url": "notes-1.0.0.anth",
                "signer": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "yanked": false
            }]
        }]
    }))
    .expect("index")
}

fn sign(key: &SigningKey, bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(key.sign(bytes).to_bytes())
}

async fn call(
    broker: &anth_service::Broker,
    function: &str,
    input: Value,
) -> Result<Value, anth_contract::Error> {
    broker.call(STORE, function, input).await
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

fn start_service(socket: &Path, data: &Path, shared: &Path) -> ChildGuard {
    let binary = std::env::var("CARGO_BIN_EXE_store-service").expect("binary path");
    let child = Command::new(binary)
        .arg("--socket")
        .arg(socket)
        .arg("--data")
        .arg(data)
        .arg("--shared")
        .arg(shared)
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

/// Starts an HTTP fixture and returns its base URL. The fixture is boxed in a
/// thread; the socket is served until the test ends.
fn start_fixture(fixture: Arc<Fixture>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
    let port = listener.local_addr().expect("address").port();
    let base = format!("http://127.0.0.1:{port}");
    let state = Arc::clone(&fixture);
    let root = base.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&state);
            let root = root.clone();
            std::thread::spawn(move || serve(stream, state, &root));
        }
    });
    base
}

fn serve(mut stream: TcpStream, fixture: Arc<Fixture>, base: &str) {
    let Some((method, path, headers, _body)) = request(&mut stream) else {
        return;
    };
    let path = path.split('?').next().unwrap_or(&path).to_string();
    match (method.as_str(), path.as_str()) {
        ("GET", "/index.json") => {
            let etag = format!(
                "\"{:x}\"",
                fixture.index.iter().fold(0u64, |hash, byte| hash
                    .wrapping_mul(31)
                    .wrapping_add(*byte as u64))
            );
            let etag = etag.as_str();
            if headers
                .iter()
                .any(|(name, value)| name == "if-none-match" && value == etag)
            {
                respond(&mut stream, 304, &[("etag", etag)], b"");
                return;
            }
            respond(
                &mut stream,
                200,
                &[("etag", etag), ("content-type", "application/json")],
                &fixture.index,
            );
        }
        ("GET", "/index.json.sig") => {
            respond(&mut stream, 200, &[], fixture.signature.as_bytes());
        }
        ("GET", "/notes-1.0.0.anth") => {
            respond(&mut stream, 200, &[], &fixture.bundle);
        }
        ("GET", "/gh/releases/latest") => {
            let body = serde_json::to_vec(&json!({
                "assets": [
                    {"name": "index.json", "browser_download_url": format!("{base}/index.json")},
                    {"name": "index.json.sig", "browser_download_url": format!("{base}/index.json.sig")}
                ]
            }))
            .expect("release");
            respond(
                &mut stream,
                200,
                &[("content-type", "application/json")],
                &body,
            );
        }
        _ => respond(&mut stream, 404, &[], b"not found"),
    }
}

type Request = (String, String, Vec<(String, String)>, Vec<u8>);

fn request(stream: &mut TcpStream) -> Option<Request> {
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
    }
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let mut parts = lines.next()?.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    Some((method, path, headers, Vec::new()))
}

fn respond(stream: &mut TcpStream, status: u16, headers: &[(&str, &str)], body: &[u8]) {
    let reason = match status {
        200 => "OK",
        304 => "Not Modified",
        404 => "Not Found",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

async fn raw_stream(socket: &Path, function: &str, input: Value, take: usize) -> Vec<Vec<u8>> {
    raw_stream_optional(socket, function, input)
        .await
        .unwrap_or_else(|error| panic!("{error}"))
        .into_iter()
        .take(take)
        .collect()
}

async fn raw_stream_optional(
    socket: &Path,
    function: &str,
    input: Value,
) -> Result<Vec<Vec<u8>>, String> {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|error| error.to_string())?;
    let body = serde_json::to_vec(&json!({
        "interface": STORE,
        "function": function,
        "input": input,
    }))
    .map_err(|error| error.to_string())?;
    let head = format!(
        "POST /__anth/stream HTTP/1.1\r\nHost: anthracite\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(&body)
        .await
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader
        .read_line(&mut status)
        .await
        .map_err(|error| error.to_string())?;
    if !status.contains(" 200 ") {
        let mut error = String::new();
        let _ = reader.read_to_string(&mut error).await;
        return Err(error);
    }
    loop {
        let mut line = String::new();
        if reader
            .read_line(&mut line)
            .await
            .map_err(|error| error.to_string())?
            == 0
            || line == "\r\n"
        {
            break;
        }
    }
    let mut chunks = Vec::new();
    loop {
        let mut size = String::new();
        if reader
            .read_line(&mut size)
            .await
            .map_err(|error| error.to_string())?
            == 0
        {
            break;
        }
        let text = size.trim();
        if text.is_empty() {
            continue;
        }
        let size = usize::from_str_radix(text, 16).map_err(|error| error.to_string())?;
        if size == 0 {
            break;
        }
        let mut chunk = vec![0u8; size];
        reader
            .read_exact(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        let mut crlf = [0u8; 2];
        reader
            .read_exact(&mut crlf)
            .await
            .map_err(|error| error.to_string())?;
        chunks.push(chunk);
    }
    Ok(chunks)
}
