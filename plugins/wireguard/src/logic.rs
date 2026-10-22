//! The panel-facing logic of the WireGuard plugin.
//!
//! Kept apart from the wasm shim so it can be tested on the host: this module
//! touches no import, no memory and no allocator, and reaches the daemon only
//! through [`Executor`]. Everything that decides what to ask for, and how to
//! read the answer, lives here.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// The host's `anth_exec`, as a seam.
///
/// The wasm build implements it over the imported function; the host tests
/// implement it over a table of canned replies. A refusal is an `Err` carrying
/// the detail the daemon gave, never a panic: a plugin that cannot reach `wg`
/// still has to answer the browser.
pub trait Executor {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String>;
    fn log(&mut self, message: &str);
}

/// One request envelope.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    Http(HttpRequest),
    Cli { argv: Vec<String> },
}

#[derive(Debug, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: Map<String, Value>,
    #[serde(default)]
    pub body: Option<Value>,
}

impl HttpRequest {
    /// The request body as an object, however the host delivered it.
    ///
    /// `plugins::api::anchor` forwards the raw bytes as a JSON *string*: an
    /// anchor is not required to speak JSON, so the host does not parse it.
    /// Accepting only an object would answer `400` to every real request, which
    /// is what happened before this existed. A string is parsed here; an object
    /// is taken as-is so a CLI caller and a future host that parses bodies both
    /// work. The error text is the caller's, so it names the body, not the host.
    fn json_body(&self) -> Result<Map<String, Value>, String> {
        match self.body.as_ref() {
            Some(Value::Object(map)) => Ok(map.clone()),
            Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
                Ok(Value::Object(map)) => Ok(map),
                Ok(_) => Err("the request body must be a JSON object".to_string()),
                Err(error) => Err(format!("the request body is not JSON: {error}")),
            },
            None => Err("a JSON object body is required".to_string()),
            Some(_) => Err("the request body must be a JSON object".to_string()),
        }
    }
}

/// Parses an envelope and answers it.
///
/// A malformed envelope is a `400` the browser can display, not a trap: a guest
/// that aborts leaves the panel with a dead request and no explanation.
pub fn handle<E: Executor>(executor: &mut E, request: &str) -> String {
    match serde_json::from_str::<Request>(request) {
        Ok(Request::Http(request)) => route(executor, &request),
        Ok(Request::Cli { argv }) => cli(executor, &argv),
        Err(error) => envelope(
            400,
            json!({"error": format!("the request is not an envelope: {error}")}),
        ),
    }
}

/// Wraps a body in the envelope `plugins::api::guest_response` understands.
pub fn envelope(status: u16, body: Value) -> String {
    json!({"status": status, "body": body}).to_string()
}

fn failure(status: u16, error: String) -> String {
    envelope(status, json!({"error": error}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

pub fn route<E: Executor>(executor: &mut E, request: &HttpRequest) -> String {
    let method = request.method.to_ascii_uppercase();
    let path = request.path.trim_end_matches('/');
    match (method.as_str(), path) {
        ("GET", "") | ("GET", "/") => {
            envelope(200, json!({"plugin": "org.anth.wireguard", "ok": true}))
        }
        ("GET", "/interfaces") => interfaces(executor),
        ("GET", "/status") => status(executor),
        ("GET", "/peers") => peers(executor, &request.query),
        ("POST", "/peers") => add_peer(executor, request),
        _ => failure(404, format!("the plugin has no route for {method} {path}")),
    }
}

/// The CLI half, for `plugin call`.
pub fn cli<E: Executor>(executor: &mut E, argv: &[String]) -> String {
    match argv.first().map(String::as_str) {
        Some("list") | None => interfaces(executor),
        Some("status") => status(executor),
        Some("peers") => {
            let mut query = Map::new();
            if let Some(interface) = argv.get(1) {
                query.insert("interface".to_string(), Value::from(interface.as_str()));
            }
            peers(executor, &query)
        }
        Some(other) => failure(
            400,
            format!("unknown subcommand `{other}`; try list, status or peers"),
        ),
    }
}

/// The names `wg show interfaces` reports.
///
/// `ip link` lists every device and says nothing about the driver, so this is
/// the only trustworthy source. A failure here is the whole request failing:
/// without the list there is nothing to report.
fn interface_names<E: Executor>(executor: &mut E) -> Result<Vec<String>, String> {
    let listing = executor.exec("wg.interfaces", &json!({}))?;
    let names = listing
        .get("stdout")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(names.split_whitespace().map(ToOwned::to_owned).collect())
}

fn interfaces<E: Executor>(executor: &mut E) -> String {
    let names = match interface_names(executor) {
        Ok(names) => names,
        Err(error) => return failure(502, error),
    };
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        match executor.exec("wg.show", &json!({"interface": name})) {
            Ok(reply) => {
                let text = reply
                    .get("stdout")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                entries.push(parse_interface(&name, text));
            }
            // One unreadable interface does not hide the others. The error
            // travels inside the entry, where the panel renders it in place.
            Err(error) => {
                executor.log(&format!("{name}: {error}"));
                entries.push(json!({"name": name, "error": error}));
            }
        }
    }
    envelope(200, json!({"interfaces": entries}))
}

fn status<E: Executor>(executor: &mut E) -> String {
    let names = match interface_names(executor) {
        Ok(names) => names,
        Err(error) => return failure(502, error),
    };
    let mut peers = 0usize;
    for name in &names {
        if let Ok(reply) = executor.exec("wg.show", &json!({"interface": name})) {
            let text = reply
                .get("stdout")
                .and_then(Value::as_str)
                .unwrap_or_default();
            peers += parse_interface(name, text)
                .get("peers")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
        }
    }
    envelope(
        200,
        json!({"interfaces": names.len(), "peers": peers, "names": names}),
    )
}

fn peers<E: Executor>(executor: &mut E, query: &Map<String, Value>) -> String {
    let Some(interface) = query.get("interface").and_then(Value::as_str) else {
        return failure(400, "`interface` is required".to_string());
    };
    match executor.exec("wg.show", &json!({"interface": interface})) {
        Ok(reply) => {
            let text = reply
                .get("stdout")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let parsed = parse_interface(interface, text);
            envelope(
                200,
                json!({
                    "interface": parsed.get("name"),
                    "peers": parsed.get("peers"),
                }),
            )
        }
        Err(error) => failure(502, error),
    }
}

/// Adds a peer.
///
/// The body is `{"interface": "wg0", "public_key": "...", "allowed_ips":
/// [...], "endpoint": "host:51820"}`. Fields are checked here so the browser
/// gets a specific message; the host validates them again, and only its verdict
/// decides what runs.
fn add_peer<E: Executor>(executor: &mut E, request: &HttpRequest) -> String {
    let body = match request.json_body() {
        Ok(body) => body,
        Err(error) => return failure(400, error),
    };
    let Some(interface) = body.get("interface").and_then(Value::as_str) else {
        return failure(400, "`interface` is required".to_string());
    };
    let Some(public_key) = body.get("public_key").and_then(Value::as_str) else {
        return failure(400, "`public_key` is required".to_string());
    };
    let allowed_ips = match body.get("allowed_ips") {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::String(parts)) => parts.clone(),
        _ => return failure(400, "`allowed_ips` is required".to_string()),
    };
    if allowed_ips.trim().is_empty() {
        return failure(400, "`allowed_ips` must not be empty".to_string());
    }

    let mut args = json!({
        "interface": interface,
        "public_key": public_key,
        "allowed_ips": allowed_ips,
    });
    // The endpoint decides which recipe runs, not an optional argument on one:
    // `wg` wants the flag and its value as two elements, and the host drops
    // whole elements, so an omitted endpoint would otherwise leave a bare
    // `endpoint` at the end of the argv.
    let mut capability = "wg.add_peer";
    if let Some(endpoint) = body.get("endpoint").and_then(Value::as_str)
        && !endpoint.is_empty()
    {
        // A host:port pair no single validator covers, which is why the endpoint
        // recipe declares `accepts_text` and the installer warns about it.
        args["endpoint"] = Value::from(endpoint);
        capability = "wg.add_peer_endpoint";
    }

    match executor.exec(capability, &args) {
        Ok(_) => envelope(
            201,
            json!({"added": true, "interface": interface, "endpoint": capability == "wg.add_peer_endpoint"}),
        ),
        Err(error) => failure(502, error),
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// One `wg show <interface>` block into JSON.
///
/// `wg` prints a stable text format: an `interface:` line, then indented
/// `key: value` pairs, then one `peer:` line per peer with its own indented
/// pairs. There is no `peers:` header, so the first `peer:` is what ends the
/// interface section. Parsing here rather than in the browser keeps
/// WireGuard knowledge out of the panel, and means the HTTP anchor and the CLI
/// anchor return the same shape.
pub fn parse_interface(name: &str, text: &str) -> Value {
    let mut public_key = None;
    let mut listen_port = None;
    let mut peers: Vec<Value> = Vec::new();
    let mut current: Option<Map<String, Value>> = None;
    let mut in_peers = false;

    for line in text.lines() {
        let trimmed = line.trim();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        // The key alone decides the two section headers, because they carry the
        // same text in both sections and the state is what changes.
        if key == "peer" {
            if let Some(peer) = current.take() {
                peers.push(Value::Object(peer));
            }
            let mut peer = Map::new();
            peer.insert("public_key".to_string(), Value::from(value));
            current = Some(peer);
            in_peers = true;
            continue;
        }
        match (in_peers, key) {
            (false, "public key") => public_key = Some(Value::from(value)),
            (false, "listening port") => listen_port = value.parse::<u64>().ok().map(Value::from),
            (true, "allowed ips") => {
                if let Some(peer) = current.as_mut() {
                    let ips: Vec<Value> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|part| !part.is_empty())
                        .map(Value::from)
                        .collect();
                    peer.insert("allowed_ips".to_string(), Value::Array(ips));
                }
            }
            (true, "latest handshake") => {
                if let Some(peer) = current.as_mut() {
                    peer.insert("latest_handshake".to_string(), Value::from(value));
                }
            }
            (true, "transfer") => {
                if let Some(peer) = current.as_mut() {
                    let (received, sent) = split_transfer(value);
                    peer.insert("received".to_string(), Value::from(received));
                    peer.insert("sent".to_string(), Value::from(sent));
                }
            }
            _ => {}
        }
    }
    if let Some(peer) = current.take() {
        peers.push(Value::Object(peer));
    }

    json!({
        "name": name,
        "public_key": public_key,
        "listen_port": listen_port,
        "peers": peers,
    })
}

/// Splits `wg`'s transfer line into the two strings it already formatted.
///
/// The values stay text: `wg` picks its own unit, and re-parsing into bytes so
/// the panel can format them again would only lose precision.
pub fn split_transfer(value: &str) -> (String, String) {
    let mut received = String::new();
    let mut sent = String::new();
    for part in value.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_suffix(" received") {
            received = rest.to_string();
        } else if let Some(rest) = part.strip_suffix(" sent") {
            sent = rest.to_string();
        }
    }
    (received, sent)
}

#[cfg(test)]
mod tests;
