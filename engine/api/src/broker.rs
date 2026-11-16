//! The private broker: a service calls another service's interface through the
//! grant, never through the public router (§22, D9).
//!
//! One socket per composed app lives at `state/broker/<app-id>.sock`. The
//! service receives the path as `{broker}`; the engine owns the listener and
//! resolves the interface to the composed provider, applies the caller's grant
//! and the provider's budget, and audits every call. The audit bridge
//! (`anth:audit/record@1.0.0`) is served on the same socket.

use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;
use std::sync::Arc;

use anth_compose::{InstalledApp, Provider};
use anth_contract::{AppId, AuditAction, Error, InterfaceId, Outcome};
use base64::Engine as _;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::{limited_stream, ApiError, EngineState};

const CALL_CAP: usize = 4 * 1024 * 1024;
const HEAD_CAP: usize = 8 * 1024;
pub const AUDIT_INTERFACE: &str = "anth:audit/record@1.0.0";

/// Starts one broker listener per composed app. A stale socket from a previous
/// boot is replaced; the directory is `0711` so a service uid can reach its own
/// socket, while the socket itself is `0600` and owned by that uid.
pub fn start_all(state: &Arc<EngineState>) -> Result<(), Error> {
    let store = state
        .apps
        .as_ref()
        .ok_or_else(|| Error::unavailable("the engine has no state tree to broker"))?;
    let view = state.broker_view()?;
    let resolution = &view.resolution;
    let dir = store.state.join("broker");
    std::fs::create_dir_all(&dir)
        .map_err(|error| Error::internal(format!("creating {}: {error}", dir.display())))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o711))
        .map_err(|error| Error::internal(format!("setting {} to 0711: {error}", dir.display())))?;
    for id in &resolution.order {
        let path = dir.join(format!("{id}.sock"));
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_socket() => {
                let _ = std::fs::remove_file(&path);
            }
            Ok(_) => {
                return Err(Error::invalid(format!(
                    "{} exists and is not a socket",
                    path.display()
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::internal(format!(
                    "inspecting {}: {error}",
                    path.display()
                )))
            }
        }
        let listener = UnixListener::bind(&path).map_err(|error| {
            Error::internal(format!("binding the broker {}: {error}", path.display()))
        })?;
        // The socket is private to the service uid: `0600` plus the uid that
        // actually runs the service. The directory stays `0711`, so a service
        // reaches its own socket without listing its peers. On a non-root host
        // the service keeps the engine uid and no ownership check applies.
        let uid = if unsafe { libc::geteuid() } == 0 {
            state.service_uid_of(id)
        } else {
            None
        };
        if let Some(uid) = uid {
            std::os::unix::fs::chown(&path, Some(uid), None).map_err(|error| {
                Error::internal(format!("owning {} by uid {uid}: {error}", path.display()))
            })?;
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| Error::internal(format!("setting {} to 0600: {error}", path.display())),
        )?;
        let caller = id.clone();
        let state = Arc::clone(state);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = Arc::clone(&state);
                let caller = caller.clone();
                tokio::spawn(async move {
                    let _ = serve(stream, state, caller, uid).await;
                });
            }
        });
    }
    Ok(())
}

/// The broker directory for a state tree, so callers agree on the path.
pub fn socket_path(state_root: &std::path::Path, app: &AppId) -> PathBuf {
    state_root.join("broker").join(format!("{app}.sock"))
}

async fn serve(
    mut stream: UnixStream,
    state: Arc<EngineState>,
    caller: AppId,
    expected_uid: Option<u32>,
) -> std::io::Result<()> {
    // The socket is `0600` and owned by the service, but the credential check
    // is the authority: root is the engine (and the test harness) and a peer
    // with any other uid is rejected, even if the mode let it connect.
    if let Some(expected) = expected_uid {
        let peer = stream
            .peer_cred()
            .map(|credential| credential.uid())
            .unwrap_or(u32::MAX);
        if peer != 0 && peer != expected {
            if let Some(store) = state.apps.as_ref() {
                let entry = anth_contract::AuditEntry {
                    at: anth_compose::audit::timestamp(),
                    actor: format!("app:{caller}"),
                    action: AuditAction::BrokerCall,
                    app: Some(caller.to_string()),
                    result: Outcome::Denied,
                    detail: Some(json!({"peer_uid": peer, "expected_uid": expected})),
                };
                let _ = store.audit.append(&entry);
            }
            let error = Error::denied(format!(
                "peer uid {peer} cannot use the broker of `{caller}`"
            ));
            return respond(
                &mut stream,
                error.status(),
                &serde_json::to_value(error.envelope()).unwrap_or(Value::Null),
            )
            .await;
        }
    }
    let (method, path, body) = match request(&mut stream).await {
        Ok(request) => request,
        Err(error) => {
            let envelope = ApiError(Error::invalid(error.to_string())).0.envelope();
            return respond(
                &mut stream,
                400,
                &serde_json::to_value(envelope).unwrap_or(Value::Null),
            )
            .await;
        }
    };
    if method == "GET" && path == "/health" {
        return respond(&mut stream, 200, &json!({"status": "ok"})).await;
    }
    if method == "POST" && path == "/__anth/call" {
        return match call(&state, caller.as_str(), &body).await {
            Ok(value) => respond(&mut stream, 200, &json!({"ok": value})).await,
            Err(ApiError(error)) => {
                respond(
                    &mut stream,
                    error.status(),
                    &serde_json::to_value(error.envelope()).unwrap_or(Value::Null),
                )
                .await
            }
        };
    }
    if method == "POST" && path == "/__anth/stream" {
        return match stream_call(&state, caller.as_str(), &body).await {
            Ok(chunks) => relay(&mut stream, &state, caller.as_str(), chunks).await,
            Err(ApiError(error)) => {
                respond(
                    &mut stream,
                    error.status(),
                    &serde_json::to_value(error.envelope()).unwrap_or(Value::Null),
                )
                .await
            }
        };
    }
    respond(
        &mut stream,
        404,
        &json!({"error": {"code": "not-found", "message": "no such endpoint"}}),
    )
    .await
}

/// Relays a bounded provider stream as chunked JSON frames: `{"chunk": <base64>}`
/// per chunk and a terminal `{"error": …}` when the provider or its budget ends
/// the stream early. Keeping the error inside the body lets the caller see
/// `exhausted` instead of a truncated connection.
async fn relay(
    stream: &mut UnixStream,
    state: &Arc<EngineState>,
    caller: &str,
    mut chunks: crate::ChunkStream,
) -> std::io::Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\n\
              Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        )
        .await?;
    while let Some(item) = chunks.next().await {
        match item {
            Ok(bytes) => {
                let frame = json!({
                    "chunk": base64::engine::general_purpose::STANDARD.encode(bytes),
                });
                write_frame(stream, &frame).await?;
            }
            Err(error) => {
                if let Some(store) = state.apps.as_ref() {
                    let entry = anth_contract::AuditEntry {
                        at: anth_compose::audit::timestamp(),
                        actor: format!("app:{caller}"),
                        action: AuditAction::BrokerCall,
                        app: Some(caller.to_string()),
                        result: Outcome::Failed,
                        detail: Some(json!({"stream": true, "error": error.message()})),
                    };
                    let _ = store.audit.append(&entry);
                }
                write_frame(
                    stream,
                    &serde_json::to_value(error.envelope()).unwrap_or(Value::Null),
                )
                .await?;
                stream.write_all(b"0\r\n\r\n").await?;
                return stream.flush().await;
            }
        }
    }
    stream.write_all(b"0\r\n\r\n").await?;
    stream.flush().await
}

async fn write_frame(stream: &mut UnixStream, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    stream
        .write_all(format!("{:x}\r\n", body.len()).as_bytes())
        .await?;
    stream.write_all(&body).await?;
    stream.write_all(b"\r\n").await
}

async fn respond(stream: &mut UnixStream, status: u16, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.flush().await
}

async fn request(stream: &mut UnixStream) -> std::io::Result<(String, String, Value)> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the request ended before its headers",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > HEAD_CAP {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the request headers are too large",
            ));
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
    let body_start = head_end + 4;
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_string();
    let path = first.next().unwrap_or_default().to_string();
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if length > CALL_CAP {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the request body is too large",
        ));
    }
    while buffer.len() < body_start + length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the request ended before its body",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&buffer[body_start..body_start + length]).unwrap_or(Value::Null)
    };
    Ok((method, path, body))
}

/// One brokered call: the caller's grant must import the interface, the
/// provider must be composed and running, and the call is audited either way.
pub async fn call(state: &Arc<EngineState>, caller: &str, body: &Value) -> Result<Value, ApiError> {
    let store = state
        .apps
        .as_ref()
        .ok_or_else(|| ApiError(Error::unavailable("the engine has no state tree")))?;
    let interface = body
        .get("interface")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError(Error::invalid("the call needs an `interface`")))?;
    let function = body
        .get("function")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError(Error::invalid("the call needs a `function`")))?;
    let input = body.get("input").cloned().unwrap_or(Value::Null);
    InterfaceId::parse(interface).map_err(ApiError)?;

    if interface == AUDIT_INTERFACE {
        if function != "record" {
            return Err(ApiError(Error::not_found(format!(
                "`{AUDIT_INTERFACE}` serves `record`, not `{function}`"
            ))));
        }
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("app.record");
        let detail = sanitize(input.get("detail").cloned().unwrap_or(Value::Null));
        let entry = anth_contract::AuditEntry {
            at: anth_compose::audit::timestamp(),
            actor: format!("app:{caller}"),
            action: AuditAction::BrokerCall,
            app: Some(caller.to_string()),
            result: Outcome::Ok,
            detail: Some(json!({"bridge": true, "action": action, "detail": detail})),
        };
        store.audit.append(&entry).map_err(ApiError)?;
        return Ok(json!({"recorded": true}));
    }

    let route = route(state, caller, body).await?;
    let caller_impl = state
        .caller
        .as_ref()
        .ok_or_else(|| ApiError(Error::unavailable("this host has no app caller")))?;
    let result = caller_impl
        .call(
            route.provider.as_str(),
            &route.interface,
            &route.function,
            route.input.clone(),
        )
        .await;
    let entry = anth_contract::AuditEntry {
        at: anth_compose::audit::timestamp(),
        actor: format!("app:{caller}"),
        action: AuditAction::BrokerCall,
        app: Some(caller.to_string()),
        result: if result.is_ok() {
            Outcome::Ok
        } else {
            Outcome::Denied
        },
        detail: Some(json!({
            "callee": route.provider,
            "interface": route.interface,
            "function": route.function,
            "error": result.as_ref().err().map(|error| error.message()),
        })),
    };
    let _ = store.audit.append(&entry);
    result.map_err(ApiError)
}

/// One brokered stream: same grant resolution as [`call`], then the provider's
/// chunks are bounded by the provider manifest's byte budget. The stream ends
/// with `exhausted` when that budget runs out.
pub async fn stream_call(
    state: &Arc<EngineState>,
    caller: &str,
    body: &Value,
) -> Result<crate::ChunkStream, ApiError> {
    let store = state
        .apps
        .as_ref()
        .ok_or_else(|| ApiError(Error::unavailable("the engine has no state tree")))?;
    let route = route(state, caller, body).await?;
    let caller_impl = state
        .caller
        .as_ref()
        .ok_or_else(|| ApiError(Error::unavailable("this host has no app caller")))?;
    let chunks = caller_impl
        .stream(
            route.provider.as_str(),
            &route.interface,
            &route.function,
            route.input.clone(),
        )
        .await
        .map_err(ApiError)?;
    let entry = anth_contract::AuditEntry {
        at: anth_compose::audit::timestamp(),
        actor: format!("app:{caller}"),
        action: AuditAction::BrokerCall,
        app: Some(caller.to_string()),
        result: Outcome::Ok,
        detail: Some(json!({
            "stream": true,
            "callee": route.provider,
            "interface": route.interface,
            "function": route.function,
        })),
    };
    let _ = store.audit.append(&entry);
    Ok(limited_stream(chunks, &route.provider_app))
}

/// The resolved provider of one call, plus the caller's input.
struct Route {
    provider: AppId,
    interface: String,
    function: String,
    input: Value,
    provider_app: InstalledApp,
}

/// Resolves the caller's import to a running provider. A missing import or an
/// engine target is a denial and is audited before it is returned.
async fn route(state: &Arc<EngineState>, caller: &str, body: &Value) -> Result<Route, ApiError> {
    let interface = body
        .get("interface")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError(Error::invalid("the call needs an `interface`")))?;
    let function = body
        .get("function")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError(Error::invalid("the call needs a `function`")))?;
    let input = body.get("input").cloned().unwrap_or(Value::Null);
    InterfaceId::parse(interface).map_err(ApiError)?;
    let view = state.broker_view().map_err(ApiError)?;
    let link =
        match view.resolution.links.iter().find(|link| {
            link.consumer.as_str() == caller && link.interface.to_string() == interface
        }) {
            Some(link) => link,
            None => {
                let error = Error::denied(format!(
                    "`{caller}` does not import `{interface}` in the composition"
                ));
                audit_denied(state, caller, interface, function, &error);
                return Err(ApiError(error));
            }
        };
    let provider = match &link.provider {
        Provider::App(provider) => provider.clone(),
        Provider::Engine => {
            let error = Error::denied(format!(
                "`{interface}` is an engine interface and is not a broker target"
            ));
            audit_denied(state, caller, interface, function, &error);
            return Err(ApiError(error));
        }
    };
    let provider_app = view
        .installed
        .get(provider.as_str())
        .cloned()
        .ok_or_else(|| {
            ApiError(Error::internal(format!(
                "`{provider}` is resolved but its manifest is missing"
            )))
        })?;
    Ok(Route {
        provider,
        interface: interface.to_string(),
        function: function.to_string(),
        input,
        provider_app,
    })
}

/// Writes the denial into the audit log before the caller can see it (§22).
fn audit_denied(state: &EngineState, caller: &str, interface: &str, function: &str, error: &Error) {
    let Some(store) = state.apps.as_ref() else {
        return;
    };
    let entry = anth_contract::AuditEntry {
        at: anth_compose::audit::timestamp(),
        actor: format!("app:{caller}"),
        action: AuditAction::BrokerCall,
        app: Some(caller.to_string()),
        result: Outcome::Denied,
        detail: Some(json!({
            "interface": interface,
            "function": function,
            "reason": error.message(),
        })),
    };
    let _ = store.audit.append(&entry);
}

/// Drops values that look like secrets from an app-supplied bridge detail.
fn sanitize(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| {
                    let key = key.to_ascii_lowercase();
                    ![
                        "password",
                        "secret",
                        "token",
                        "key",
                        "authorization",
                        "header",
                        "env",
                    ]
                    .iter()
                    .any(|needle| key.contains(needle))
                })
                .map(|(key, value)| (key, sanitize(value)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize).collect()),
        other => other,
    }
}
