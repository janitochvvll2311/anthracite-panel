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

fn main() {
    let mut args = std::env::args().skip(1);
    let mut socket = None;
    let mut die_after = None;
    let mut die_once = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--socket" => socket = args.next().map(PathBuf::from),
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
    let (status, payload) = if method == "GET" && path == "/health" {
        (200, b"{\"ok\":true}".to_vec())
    } else if method == "POST" && path == "/__anth/call" {
        call(&body)
    } else {
        (404, envelope("not-found", "no such endpoint"))
    };
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(&payload)?;
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
