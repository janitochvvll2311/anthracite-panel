//! HTTP anchors: `GET|POST /api/plugins/<id>/<path>`.
//!
//! The route is one wildcard rather than one axum route per declared anchor, and
//! that is deliberate. A manifest is a file inside a bundle, so rules that only
//! consult the manifest at install time would be the manifest talking: a plugin
//! that rewrote `plugin.toml` after install would reach a route the installer
//! never saw, or a different method on one it did. So the declared anchors are
//! never used for authorization. The request is turned into the same `{"kind":
//! "http"}` envelope the CLI sends and handed to the guest, and the guest can
//! only affect the host through `anth_exec`, which reads the frozen grant.
//!
//! What the manifest declares is therefore documentation and the UI's
//! catalogue, not a permission. That is a smaller guarantee than "only declared
//! routes work", and it is the one the artifact can actually support.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

use crate::api::ApiError;
use crate::app::AppState;
use crate::plugins::manifest::{PluginError, PluginErrorKind, validate_id};
use crate::plugins::runtime::{GuestError, GuestErrorKind};
use crate::plugins::{bundle, runtime};

/// Largest request body forwarded to a guest.
///
/// The envelope carries the body as a JSON string, so a guest also pays the
/// escaping cost. An HTTP anchor is for control operations, not for uploads.
pub const MAX_ANCHOR_BODY: usize = 1 << 20;

/// `GET|POST /api/plugins/{id}/{*path}`.
pub async fn anchor(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    method: Method,
    uri: axum::http::Uri,
    body: Bytes,
) -> Result<Response, ApiError> {
    let config = state.config().await;

    if !config.plugins.enabled {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "plugins are disabled in the configuration",
        ));
    }
    // Before any filesystem work: an id reaches a directory join. A malformed
    // id is the caller's mistake, so it is 400; `plugin_error` would call it
    // 422, which reads as "your plugin is broken" when the request never named
    // a plugin.
    validate_id(&id).map_err(|error| ApiError::bad_request(error.detail))?;

    let grant = config
        .plugins
        .grants
        .iter()
        .find(|grant| grant.id == id)
        .cloned()
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                format!("no plugin `{id}` is installed"),
            )
        })?;
    if !grant.enabled {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            format!("plugin `{id}` is disabled"),
        ));
    }

    // Every anchor answers to any method, so a method the manifest does not
    // mention is the guest's call to refuse, not the router's. Saying so here
    // would imply the manifest is the authority.
    if !matches!(
        method,
        Method::GET | Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            format!("`{method}` is not an anchor method"),
        ));
    }

    if body.len() > MAX_ANCHOR_BODY {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("an anchor body is at most {MAX_ANCHOR_BODY} bytes"),
        ));
    }

    let root = bundle::plugin_dir(&config.plugins.root_path(), &id).map_err(plugin_error)?;
    let plugins = config.plugins.clone();
    let fuel = plugins.fuel;
    let sudo = config.sudo;
    let query = query_map(uri.query());
    let body_text = if body.is_empty() {
        None
    } else {
        // The body travels as a string, not as parsed JSON: an anchor is not
        // required to speak JSON, and the guest is the one that knows its shape.
        Some(String::from_utf8_lossy(&body).into_owned())
    };

    // Hashing, parsing and running are all blocking, and `invoke` may sit in a
    // subprocess for as long as the deadline allows.
    let reply = tokio::task::spawn_blocking(move || -> Result<runtime::GuestReply, AnchorError> {
        let installed = bundle::load(&root, &grant).map_err(AnchorError::Load)?;
        let wasm = std::fs::read(installed.wasm_path()).map_err(|error| {
            AnchorError::Plugin(PluginError::new(
                PluginErrorKind::Io,
                format!("could not read the module: {error}"),
            ))
        })?;

        let envelope = serde_json::json!({
            "kind": "http",
            "method": method.as_str(),
            "path": canonical_path(&path),
            "query": query,
            "body": body_text,
        })
        .to_string();

        let runner = crate::command::SystemRunner::new(sudo);
        runtime::invoke(&wasm, &grant.id, &grant, &runner, &envelope, fuel)
            .map_err(AnchorError::Guest)
    })
    .await
    .map_err(|error| ApiError::internal(format!("the anchor worker failed: {error}")))?;

    let reply = reply.map_err(anchor_error)?;
    // The log belongs to the operator, not to the HTTP caller: a plugin writing
    // to its caller's response could put text in a place the admin trusts.
    for line in &reply.log {
        eprintln!("[plugin] {line}");
    }
    if reply.log_dropped > 0 {
        eprintln!(
            "[WARN] the plugin logged {} more line(s) than the host accepts",
            reply.log_dropped
        );
    }

    Ok(guest_response(&reply.body))
}

enum AnchorError {
    /// The bundle on disk failed to load. Mapped separately: a `Denied` here is
    /// the manifest lying about itself, not the caller overstepping.
    Load(PluginError),
    Plugin(PluginError),
    Guest(GuestError),
}

/// Turns the guest's envelope into an HTTP response.
///
/// An envelope that is not JSON, or JSON that names no status, is passed through
/// as `200 text/plain` rather than guessed at: a plugin returning a plain string
/// is valid, and inventing a status for it would be the host answering for the
/// plugin.
fn guest_response(body: &str) -> Response {
    let envelope = serde_json::from_str::<Value>(body).ok();
    let declared = envelope
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|map| {
            let status = u16::try_from(map.get("status")?.as_u64()?).ok()?;
            Some((map.clone(), StatusCode::from_u16(status).ok()?))
        });

    let Some((map, status)) = declared else {
        return (StatusCode::OK, body.to_string()).into_response();
    };
    let mut response = match map.get("body") {
        Some(Value::String(text)) => text.clone().into_response(),
        Some(value) => value.to_string().into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    };
    *response.status_mut() = status;
    response
}

/// Puts the wildcard capture into the one form the docs and the manifest use.
///
/// axum strips the leading slash from a `{*path}` capture, so `/status` arrives
/// as `status`, while `//status` arrives as `/status` and `status/` keeps its
/// trailing slash. A guest that compares `path` against the anchors its manifest
/// declares — which are written `/status` — would never match the ordinary
/// request, and would match the doubled-slash one. Prefixing and trimming makes
/// every spelling of a route arrive as the same string.
fn canonical_path(capture: &str) -> String {
    let trimmed = capture.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn query_map(raw: Option<&str>) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    for (key, value) in form_urlencoded_pairs(raw.unwrap_or_default()) {
        // Repeated keys keep the last value; an anchor that cares about order
        // can read the raw query from its own request.
        map.insert(key, Value::String(value));
    }
    map
}

fn form_urlencoded_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    None => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Maps a failure to load the bundle the caller named.
///
/// Separate from [`plugin_error`] because the meaning differs. `Denied` from
/// `Manifest::parse` during a request means the bundle on disk is lying about
/// itself — a rewritten `plugin.toml` — not that the caller lacks authority.
/// Answering `403` would tell a client with a valid token that it is not allowed
/// to do something it never asked for. The bundle is broken either way, which is
/// `422`, the same answer a hash mismatch gets.
fn load_error(error: PluginError) -> ApiError {
    let status = match error.kind {
        PluginErrorKind::Missing => StatusCode::NOT_FOUND,
        PluginErrorKind::Io => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    ApiError::new(status, error.detail)
}

fn plugin_error(error: PluginError) -> ApiError {
    let status = match error.kind {
        PluginErrorKind::Missing => StatusCode::NOT_FOUND,
        PluginErrorKind::Denied => StatusCode::FORBIDDEN,
        PluginErrorKind::Conflict => StatusCode::CONFLICT,
        PluginErrorKind::Tampered | PluginErrorKind::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
        PluginErrorKind::Io => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}

/// Maps a guest failure onto a status the caller can act on.
///
/// The distinction that matters is `Denied` versus everything else: a guest that
/// asked for something the grant does not carry is the plugin's fault and is
/// `502`, meaning "the plugin is broken", not `403`, which would blame the
/// caller for a decision the caller did not make.
fn anchor_error(error: AnchorError) -> ApiError {
    match error {
        AnchorError::Load(plugin) => load_error(plugin),
        AnchorError::Plugin(plugin) => plugin_error(plugin),
        AnchorError::Guest(guest) => {
            let status = match guest.kind {
                GuestErrorKind::Denied => StatusCode::BAD_GATEWAY,
                GuestErrorKind::OutOfFuel | GuestErrorKind::Deadline => StatusCode::GATEWAY_TIMEOUT,
                GuestErrorKind::Invalid => StatusCode::BAD_GATEWAY,
            };
            ApiError::new(status, guest.detail)
        }
    }
}
