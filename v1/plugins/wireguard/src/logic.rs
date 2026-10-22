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

    if method == "PATCH"
        && let Some(interface) = path.strip_prefix("/interfaces/")
    {
        return update_interface(executor, request, interface);
    }
    if method == "DELETE"
        && let Some(interface) = path.strip_prefix("/interfaces/")
    {
        return delete_interface(executor, interface);
    }

    match (method.as_str(), path) {
        ("GET", "") | ("GET", "/") => {
            envelope(200, json!({"plugin": "org.anth.wireguard", "ok": true}))
        }
        ("GET", "/interfaces") => interfaces(executor),
        ("POST", "/interfaces") => create_interface(executor, request),
        ("GET", "/status") => status(executor),
        ("GET", "/peers") => peers(executor, &request.query),
        ("POST", "/peers") => add_peer(executor, request),
        ("DELETE", "/peers") => remove_peer(executor, request),
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

/// Runs a host job and parses the JSON document it returns in `stdout`.
fn job_json<E: Executor>(
    executor: &mut E,
    capability: &str,
    args: &Value,
) -> Result<Value, String> {
    let reply = executor.exec(capability, args)?;
    let stdout = reply
        .get("stdout")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{capability}` returned no stdout"))?;
    if stdout.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(stdout).map_err(|error| {
        format!("`{capability}` returned invalid JSON: {error}")
    })
}

fn listed_interfaces<E: Executor>(executor: &mut E) -> Option<Vec<Value>> {
    match job_json(executor, "wg.list", &json!({})) {
        Ok(Value::Array(entries)) => Some(entries),
        _ => None,
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
    if let Some(entries) = listed_interfaces(executor) {
        return envelope(200, json!({"interfaces": entries}));
    }
    runtime_interfaces(executor)
}

/// Compatibility path for a host whose grant predates the `wg.list` job.
fn runtime_interfaces<E: Executor>(executor: &mut E) -> String {
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
    if let Some(entries) = listed_interfaces(executor) {
        let peers = entries
            .iter()
            .map(|entry| {
                entry
                    .get("peers")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0)
            })
            .sum::<usize>();
        let names: Vec<Value> = entries
            .iter()
            .filter_map(|entry| entry.get("interface").cloned())
            .collect();
        return envelope(
            200,
            json!({"interfaces": entries.len(), "peers": peers, "names": names}),
        );
    }

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
    if let Some(entries) = listed_interfaces(executor) {
        let entry = entries
            .iter()
            .find(|entry| entry.get("interface").and_then(Value::as_str) == Some(interface));
        return match entry {
            Some(entry) => envelope(
                200,
                json!({
                    "interface": interface,
                    "peers": entry.get("peers").cloned().unwrap_or_else(|| json!([])),
                }),
            ),
            None => failure(404, format!("interface `{interface}` is not configured")),
        };
    }

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

fn string_field(body: &Map<String, Value>, key: &str) -> Option<String> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn number_field(body: &Map<String, Value>, key: &str) -> Option<String> {
    match body.get(key) {
        Some(Value::Number(number)) => Some(number.to_string()),
        Some(Value::String(text)) if !text.trim().is_empty() => {
            Some(text.trim().to_string())
        }
        _ => None,
    }
}

fn addresses_field(body: &Map<String, Value>) -> Option<String> {
    match body.get("addresses") {
        Some(Value::Array(parts)) => {
            let values: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            (!values.is_empty()).then(|| values.join(","))
        }
        Some(Value::String(text)) if !text.trim().is_empty() => {
            Some(text.trim().to_string())
        }
        _ => None,
    }
}

fn interface_args(body: &Map<String, Value>) -> Map<String, Value> {
    let mut args = Map::new();
    if let Some(interface) = string_field(body, "interface") {
        args.insert("interface".to_string(), Value::from(interface));
    }
    if let Some(key) = string_field(body, "private_key") {
        args.insert("private_key".to_string(), Value::from(key));
    }
    if let Some(port) = number_field(body, "listen_port") {
        args.insert("listen_port".to_string(), Value::from(port));
    }
    if let Some(addresses) = addresses_field(body) {
        args.insert("addresses".to_string(), Value::from(addresses));
    }
    if let Some(mtu) = number_field(body, "mtu") {
        args.insert("mtu".to_string(), Value::from(mtu));
    }
    args
}

fn create_interface<E: Executor>(executor: &mut E, request: &HttpRequest) -> String {
    let body = match request.json_body() {
        Ok(body) => body,
        Err(error) => return failure(400, error),
    };
    if string_field(&body, "interface").is_none() {
        return failure(400, "`interface` is required".to_string());
    }
    match job_json(executor, "wg.create", &Value::Object(interface_args(&body))) {
        Ok(summary) => envelope(201, summary),
        Err(error) => failure(502, error),
    }
}

fn update_interface<E: Executor>(
    executor: &mut E,
    request: &HttpRequest,
    interface: &str,
) -> String {
    if interface.is_empty() || interface.contains('/') {
        return failure(400, "`interface` is required in the path".to_string());
    }
    let body = match request.json_body() {
        Ok(body) => body,
        Err(error) => return failure(400, error),
    };
    let mut args = interface_args(&body);
    args.insert("interface".to_string(), Value::from(interface));
    match job_json(executor, "wg.update", &Value::Object(args)) {
        Ok(summary) => envelope(200, summary),
        Err(error) => failure(502, error),
    }
}

fn delete_interface<E: Executor>(executor: &mut E, interface: &str) -> String {
    if interface.is_empty() || interface.contains('/') {
        return failure(400, "`interface` is required in the path".to_string());
    }
    match job_json(
        executor,
        "wg.delete",
        &json!({"interface": interface}),
    ) {
        Ok(summary) => envelope(200, summary),
        Err(error) => failure(502, error),
    }
}

fn add_peer<E: Executor>(executor: &mut E, request: &HttpRequest) -> String {
    let body = match request.json_body() {
        Ok(body) => body,
        Err(error) => return failure(400, error),
    };
    let Some(interface) = string_field(&body, "interface") else {
        return failure(400, "`interface` is required".to_string());
    };
    let Some(public_key) = string_field(&body, "public_key") else {
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
    let endpoint = string_field(&body, "endpoint");

    let mut args = json!({
        "interface": interface,
        "public_key": public_key,
        "allowed_ips": allowed_ips,
    });
    if let Some(endpoint) = &endpoint {
        args["endpoint"] = Value::from(endpoint.as_str());
    }

    match executor.exec("wg.add_peer", &args) {
        Ok(_) => envelope(
            201,
            json!({"added": true, "interface": interface, "endpoint": endpoint.is_some()}),
        ),
        Err(job_error) => {
            // Compatibility with a grant from before peer persistence: the old
            // recipe split endpoint/no-endpoint into two capability ids.
            let legacy = if endpoint.is_some() {
                "wg.add_peer_endpoint"
            } else {
                "wg.add_peer"
            };
            match executor.exec(legacy, &args) {
                Ok(_) => envelope(
                    201,
                    json!({"added": true, "interface": interface, "endpoint": endpoint.is_some()}),
                ),
                Err(_) => failure(502, job_error),
            }
        }
    }
}

fn remove_peer<E: Executor>(executor: &mut E, request: &HttpRequest) -> String {
    let body = match request.json_body() {
        Ok(body) => body,
        Err(error) => return failure(400, error),
    };
    let Some(interface) = string_field(&body, "interface") else {
        return failure(400, "`interface` is required".to_string());
    };
    let Some(public_key) = string_field(&body, "public_key") else {
        return failure(400, "`public_key` is required".to_string());
    };
    match job_json(
        executor,
        "wg.remove_peer",
        &json!({"interface": interface, "public_key": public_key}),
    ) {
        Ok(summary) => envelope(200, summary),
        Err(error) => failure(502, error),
    }
}

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
