//! Remote acid test for the generated clients.
//!
//! The HTTP/route client and the CLI client run over the same injected
//! transport. Here the transport speaks HTTP/1.1 over TCP and carries Basic
//! auth, so the client is genuinely remote from the official panel.
//!
//! ```sh
//! ANTH_PANEL_URL=http://127.0.0.1:8090 \
//! ANTH_PANEL_AUTH=operator:anthracite-dev \
//! cargo run -p anth-client --example remote
//! ```

use std::env;
use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpStream;

use anth_client::http::GreetingHttpClient;
use anth_client::{ClientError, Transport};
use base64::Engine as _;

struct Remote {
    base: String,
    authorization: String,
}

impl Remote {
    fn new(base: String, credentials: &str) -> Self {
        let authorization = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        );
        Self {
            base: base.trim_end_matches('/').to_string(),
            authorization,
        }
    }
}

impl Transport for Remote {
    fn request(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, ClientError> {
        let url = format!("{}{}", self.base, path);
        let authority = url
            .strip_prefix("http://")
            .ok_or_else(|| ClientError::new("this example only speaks http://"))?;
        let host = authority.split('/').next().unwrap_or(authority);
        let mut stream = TcpStream::connect(host)
            .map_err(|error| ClientError::new(format!("cannot reach {host}: {error}")))?;
        let body = body.unwrap_or("");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: {}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            self.authorization,
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
}

fn parse(raw: &[u8]) -> Result<String, ClientError> {
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
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("transfer-encoding")
                && value.trim().eq_ignore_ascii_case("chunked")
            {
                chunked = true;
            }
        }
    }
    let body = if chunked {
        decode_chunked(&raw[split + 4..])?
    } else {
        raw[split + 4..].to_vec()
    };
    if status >= 400 {
        return Err(ClientError::new(format!(
            "the panel answered {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    String::from_utf8(body)
        .map_err(|error| ClientError::new(format!("the body is not text: {error}")))
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
    let credentials =
        env::var("ANTH_PANEL_AUTH").unwrap_or_else(|_| "operator:anthracite-dev".to_string());

    let http = GreetingHttpClient::new(Remote::new(base.clone(), &credentials));
    println!("http: {}", http.greet("Ada".to_string())?);

    let cli = anth_client::cli::run(
        Remote::new(base, &credentials),
        "greet",
        &["--name".to_string(), "Ada".to_string()],
    )?;
    println!("cli: {cli}");
    Ok(())
}
