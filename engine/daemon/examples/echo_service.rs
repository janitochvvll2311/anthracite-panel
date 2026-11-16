//! A deterministic service fixture for the daemon's service tests.
//!
//! It speaks exactly the private protocol of §19 §4: `GET /health` and
//! `POST /__anth/call`. It is never installed as an app; the tests point the
//! supervisor at this binary to exercise launch, readiness, calls, errors,
//! crashes and restarts without a bundle.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

static SHARED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static DATA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
static PROBE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn main() {
    let mut args = std::env::args().skip(1);
    let mut socket = None;
    let mut die_after = None;
    let mut die_once = None;
    let mut listen = true;
    let mut tcp = None;
    let mut shared = None;
    let mut data = None;
    let mut probe = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--socket" => socket = args.next().map(PathBuf::from),
            "--no-listen" => listen = false,
            "--shared" => shared = args.next().map(PathBuf::from),
            "--data" => data = args.next().map(PathBuf::from),
            "--probe" => probe = args.next().map(PathBuf::from),
            "--tcp" => tcp = args.next().and_then(|value| value.parse::<u16>().ok()),
            "--die-after-ms" => {
                die_after = args.next().and_then(|value| value.parse::<u64>().ok());
            }
            "--die-once" => die_once = args.next().map(PathBuf::from),
            _ => {
                eprintln!("unknown argument `{argument}`");
                std::process::exit(2);
            }
        }
    }
    let Some(socket) = socket else {
        eprintln!("--socket is required");
        std::process::exit(2);
    };
    let mut die = die_after;
    if let Some(marker) = die_once {
        // The restarted process finds the marker and stays up, which lets a test
        // observe exactly one crash and one successful restart.
        if marker.exists() {
            die = None;
        } else {
            let _ = std::fs::write(&marker, b"1");
        }
    }
    if let Some(milliseconds) = die {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(milliseconds));
            std::process::exit(1);
        });
    }

    if !listen {
        // A process that stays up without answering `/health`, for the
        // readiness-timeout test. It never binds, so the supervisor waits.
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    if let Some(path) = shared {
        let _ = SHARED.set(path);
    }
    if let Some(path) = data {
        let _ = DATA.set(path);
    }
    if let Some(path) = probe {
        let _ = PROBE.set(path);
    }
    if let Some(port) = tcp {
        // Proves the port capability: binding below 1024 fails without it.
        let bound = std::net::TcpListener::bind(("127.0.0.1", port));
        if let Err(error) = bound {
            eprintln!("the fixture could not bind tcp:{port}: {error}");
            std::process::exit(3);
        }
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).expect("bind the service socket");
    for connection in listener.incoming() {
        let Ok(stream) = connection else {
            continue;
        };
        let _ = handle(stream);
    }
}

fn handle(mut stream: UnixStream) -> std::io::Result<()> {
    let (method, path, body) = request(&mut stream)?;
    if method == "POST" && path == "/__anth/stream" {
        return stream_response(&mut stream, &body);
    }
    let (status, payload) = if method == "GET" && path == "/health" {
        (200, b"{\"ok\":true}".to_vec())
    } else if method == "POST" && path == "/__anth/call" {
        call(&body)
    } else {
        (404, envelope("not-found", "no such endpoint"))
    };
    write_json(&mut stream, status, &payload)
}

fn write_json(stream: &mut UnixStream, status: u16, payload: &[u8]) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

/// A deterministic stream: `chunks` ends cleanly, `break` closes after one chunk
/// so the caller must see a truncated body, and `fail` never sends a byte.
fn stream_response(stream: &mut UnixStream, body: &[u8]) -> std::io::Result<()> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return write_json(
            stream,
            400,
            &envelope("invalid", "the call body is not JSON"),
        );
    };
    let function = value
        .get("function")
        .and_then(|function| function.as_str())
        .unwrap_or_default();
    let chunks: &[Option<&[u8]>] = match function {
        "chunks" => &[Some(b"one"), Some(b"two"), Some(b"three")],
        "break" => &[Some(b"head"), None],
        "fail" => {
            return write_json(
                stream,
                400,
                &envelope("invalid", "the fixture refused to stream"),
            );
        }
        _ => {
            return write_json(
                stream,
                404,
                &envelope("not-found", "unknown stream function"),
            )
        }
    };
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
          Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    )?;
    for chunk in chunks {
        match chunk {
            Some(bytes) => {
                write!(stream, "{:x}\r\n", bytes.len())?;
                stream.write_all(bytes)?;
                stream.write_all(b"\r\n")?;
            }
            None => {
                // No terminator: the reader observes a failed producer.
                stream.flush()?;
                return Ok(());
            }
        }
    }
    stream.write_all(b"0\r\n\r\n")?;
    stream.flush()
}

fn call(body: &[u8]) -> (u16, Vec<u8>) {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return (400, envelope("invalid", "the call body is not JSON"));
    };
    let function = value
        .get("function")
        .and_then(|function| function.as_str())
        .unwrap_or_default();
    match function {
        "echo" => (
            200,
            serde_json::to_vec(&serde_json::json!({
                "ok": value.get("input").cloned().unwrap_or(serde_json::Value::Null),
            }))
            .expect("json"),
        ),
        "fail" => (400, envelope("invalid", "the fixture refused")),
        "write-data" => {
            let path = DATA.get().map(|dir| dir.join("fixture-data"));
            let wrote = path
                .as_ref()
                .is_some_and(|path| std::fs::write(path, b"ok").is_ok());
            (
                200,
                serde_json::to_vec(&serde_json::json!({"ok": {"wrote": wrote}})).expect("json"),
            )
        }
        "write-root" => {
            let wrote = PROBE
                .get()
                .is_some_and(|path| std::fs::write(path.join("probe"), b"x").is_ok());
            (
                200,
                serde_json::to_vec(&serde_json::json!({"ok": {"wrote": wrote}})).expect("json"),
            )
        }
        "write-shared" => {
            let path = SHARED.get().map(|dir| dir.join("fixture-shared"));
            let wrote = path
                .as_ref()
                .is_some_and(|path| std::fs::write(path, b"ok").is_ok());
            (
                200,
                serde_json::to_vec(&serde_json::json!({"ok": {"wrote": wrote}})).expect("json"),
            )
        }
        "outside" => {
            // L1: with no declared roots, `/etc/hostname` is outside the view.
            let read = std::fs::read_to_string("/etc/hostname").is_ok();
            let list = std::fs::read_dir("/").is_ok();
            (
                200,
                serde_json::to_vec(&serde_json::json!({
                    "ok": {"read": read, "list": list}
                }))
                .expect("json"),
            )
        }
        "status" => {
            let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
            let mut fields = serde_json::Map::new();
            for line in status.lines() {
                if line.starts_with("NoNewPrivs:") || line.starts_with("Seccomp:") {
                    if let Some((key, value)) = line.split_once(':') {
                        fields.insert(
                            key.to_string(),
                            serde_json::Value::String(value.trim().to_string()),
                        );
                    }
                }
            }
            (
                200,
                serde_json::to_vec(&serde_json::json!({"ok": fields})).expect("json"),
            )
        }
        "slow" => {
            std::thread::sleep(Duration::from_secs(30));
            (200, b"{\"ok\":true}".to_vec())
        }
        _ => (400, envelope("invalid", "unknown function")),
    }
}

fn envelope(code: &str, message: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "error": {"code": code, "message": message},
    }))
    .expect("json")
}

/// Reads one HTTP/1.1 request and its `Content-Length` body.
fn request(stream: &mut UnixStream) -> std::io::Result<(String, String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let split = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the request has no header block",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > 64 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the request headers are too large",
            ));
        }
    };
    let headers = String::from_utf8_lossy(&buffer[..split]).to_string();
    let mut lines = headers.lines();
    let request = lines.next().unwrap_or_default();
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let length = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .next()
        .unwrap_or(0);
    let mut body = buffer[split + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Ok((method, path, body))
}
