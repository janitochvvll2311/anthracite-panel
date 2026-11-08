//! Remote acid test for the generated clients.
//!
//! The HTTP/route client and the CLI client run over the same injected
//! transport. Here the transport speaks HTTP/1.1 over TCP and carries the
//! session cookie minted by the panel, so the client is genuinely remote.
//!
//! ```sh
//! ANTH_PANEL_URL=http://127.0.0.1:8090 \
//! ANTH_PANEL_OPERATOR=operator \
//! ANTH_PANEL_PASSWORD="$(sed -n 's/.*"password":"\([^"]*\)".*/\1/p' state/credentials.txt)" \
//! cargo run -p anth-client --example remote
//! ```

use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpStream;

use anth_client::http::GreetingHttpClient;
use anth_client::{ClientError, Transport};

#[derive(Clone)]
struct Remote {
    base: String,
    cookie: String,
}

impl Remote {
    /// Logs in and keeps the session cookie. The generated clients never see
    /// the password; they only see the adapter.
    fn login(base: String, operator: &str, password: &str) -> Result<Self, Box<dyn Error>> {
        let base = base.trim_end_matches('/').to_string();
        let body = serde_json::json!({
            "operator": operator,
            "password": password,
        })
        .to_string();
        let (status, headers, response) =
            exchange(&base, None, "POST", "/api/engine/session", Some(&body))?;
        if status >= 400 {
            return Err(format!("login answered {status}: {response}").into());
        }
        let cookie = headers
            .get("set-cookie")
            .and_then(|value| value.split(';').next())
            .ok_or_else(|| ClientError::new("the panel did not set a session cookie"))?;
        Ok(Self {
            base,
            cookie: cookie.to_string(),
        })
    }
}

impl Transport for Remote {
    fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, ClientError> {
        let (status, _, response) = exchange(&self.base, Some(&self.cookie), method, path, body)?;
        if status >= 400 {
            return Err(ClientError::new(format!(
                "the panel answered {status}: {response}"
            )));
        }
        Ok(response)
    }
}

fn exchange(
    base: &str,
    cookie: Option<&str>,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<(u16, HashMap<String, String>, String), ClientError> {
    let authority = base
        .strip_prefix("http://")
        .ok_or_else(|| ClientError::new("this example only speaks http://"))?;
    let host = authority.split('/').next().unwrap_or(authority);
    let mut stream = TcpStream::connect(host)
        .map_err(|error| ClientError::new(format!("cannot reach {host}: {error}")))?;
    let body = body.unwrap_or("");
    let cookie = cookie
        .map(|cookie| format!("Cookie: {cookie}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{cookie}Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| ClientError::new(format!("writing the request: {error}")))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| ClientError::new(format!("reading the response: {error}")))?;
    parse(&raw)
}

fn parse(raw: &[u8]) -> Result<(u16, HashMap<String, String>, String), ClientError> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| ClientError::new("the response has no header terminator"))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|error| ClientError::new(format!("the response head is not text: {error}")))?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| ClientError::new("the response has no HTTP status"))?;
    let mut headers = HashMap::new();
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked") {
                chunked = true;
            }
            headers.insert(name, value);
        }
    }
    let body = if chunked {
        decode_chunked(&raw[split + 4..])?
    } else {
        raw[split + 4..].to_vec()
    };
    let body = String::from_utf8(body)
        .map_err(|error| ClientError::new(format!("the body is not text: {error}")))?;
    Ok((status, headers, body))
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, ClientError> {
    let mut body = Vec::new();
    loop {
        let end = data
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| ClientError::new("a chunk header is not terminated"))?;
        let size = std::str::from_utf8(&data[..end])
            .map_err(|error| ClientError::new(format!("a chunk size is not text: {error}")))?;
        let size = size
            .split(';')
            .next()
            .and_then(|size| usize::from_str_radix(size.trim(), 16).ok())
            .ok_or_else(|| ClientError::new("a chunk size is not hexadecimal"))?;
        data = &data[end + 2..];
        if size == 0 {
            return Ok(body);
        }
        if data.len() < size + 2 {
            return Err(ClientError::new("a chunk is shorter than its size"));
        }
        body.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let base = env::var("ANTH_PANEL_URL").unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let operator = env::var("ANTH_PANEL_OPERATOR").unwrap_or_else(|_| "operator".to_string());
    let password = env::var("ANTH_PANEL_PASSWORD")
        .map_err(|_| "ANTH_PANEL_PASSWORD is required (read it from state/credentials.txt)")?;

    let remote = Remote::login(base, &operator, &password)?;
    let http = GreetingHttpClient::new(remote.clone());
    let greeting = http.greet("Ada".to_string())?;
    check("http", &greeting)?;
    println!("http: {greeting}");

    let cli = anth_client::cli::run(remote, "greet", &["--name".to_string(), "Ada".to_string()])?;
    check("cli", &cli)?;
    println!("cli: {cli}");
    Ok(())
}

/// The acid test is only meaningful if the panel actually answered with the
/// app's greeting, not with an empty body or an error envelope.
fn check(layer: &str, body: &str) -> Result<(), Box<dyn Error>> {
    let expected = "hello, Ada";
    if !body.contains(expected) {
        return Err(format!("{layer}: expected `{expected}` in `{body}`").into());
    }
    Ok(())
}
