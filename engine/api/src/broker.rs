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

use anth_compose::Provider;
use anth_contract::{AppId, AuditAction, Error, InterfaceId, Outcome};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::{current_composition, installed_apps, ApiError, EngineState};

const CALL_CAP: usize = 4 * 1024 * 1024;
const HEAD_CAP: usize = 8 * 1024;
pub const AUDIT_INTERFACE: &str = "anth:audit/record@1.0.0";

/// Starts one broker listener per composed app. A stale socket from a previous
/// boot is replaced; the directory is `0711` so a service uid can reach its own
/// socket, while the socket itself is `0660` with the engine gid.
pub fn start_all(state: &Arc<EngineState>) -> Result<(), Error> {
    let store = state
        .apps
        .as_ref()
        .ok_or_else(|| Error::unavailable("the engine has no state tree to broker"))?;
    let composition = current_composition(state).map_err(|ApiError(error)| error)?;
    let installed = installed_apps(store)?;
    let resolution =
        anth_compose::resolve(&composition, &installed).map_err(|error| error.into_error())?;
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
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).map_err(
            |error| Error::internal(format!("setting {} to 0660: {error}", path.display())),
        )?;
        let caller = id.clone();
        let state = Arc::clone(state);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = Arc::clone(&state);
                let caller = caller.clone();
                tokio::spawn(async move {
                    let _ = serve(stream, state, caller).await;
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
) -> std::io::Result<()> {
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
    let (status, value) = if method == "GET" && path == "/health" {
        (200, json!({"status": "ok"}))
    } else if method == "POST" && path == "/__anth/call" {
        match call(&state, caller.as_str(), &body).await {
            Ok(value) => (200, json!({"ok": value})),
            Err(ApiError(error)) => (
                error.status(),
                serde_json::to_value(error.envelope()).unwrap_or(Value::Null),
            ),
        }
    } else {
        (
            404,
            json!({"error": {"code": "not-found", "message": "no such endpoint"}}),
        )
    };
    respond(&mut stream, status, &value).await
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
            break;
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

    let composition = current_composition(state).map_err(|ApiError(error)| error)?;
    let installed = installed_apps(store)?;
    let resolution =
        anth_compose::resolve(&composition, &installed).map_err(|error| error.into_error())?;
    let link = resolution
        .links
        .iter()
        .find(|link| link.consumer.as_str() == caller && link.interface.to_string() == interface)
        .ok_or_else(|| {
            ApiError(Error::denied(format!(
                "`{caller}` does not import `{interface}` in the composition"
            )))
        })?;
    let provider = match &link.provider {
        Provider::App(provider) => provider.clone(),
        Provider::Engine => {
            return Err(ApiError(Error::denied(format!(
                "`{interface}` is an engine interface and is not a broker target"
            ))))
        }
    };
    let caller_impl = state
        .caller
        .as_ref()
        .ok_or_else(|| ApiError(Error::unavailable("this host has no app caller")))?;
    let result = caller_impl
        .call(provider.as_str(), interface, function, input)
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
            "callee": provider,
            "interface": interface,
            "function": function,
            "error": result.as_ref().err().map(|error| error.message()),
        })),
    };
    let _ = store.audit.append(&entry);
    result.map_err(ApiError)
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
