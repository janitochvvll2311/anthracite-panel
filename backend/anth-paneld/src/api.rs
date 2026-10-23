//! The HTTP surface, a thin adapter over [`dispatch`].
//!
//! Routes name a command, so a request body carries the command's arguments at
//! the top level and never the socket's `type`/`data` envelope: the envelope is
//! a transport concern, not an API one. Every handler therefore builds a
//! [`Request`] and calls `dispatch::handle`, which keeps one site that decides
//! what a command does no matter which transport reached it.
//!
//! Errors take their status from [`ErrorKind::http_status`], the same mapping
//! the CLI uses for exit codes, so the two cannot drift.
//!
//! A plugin's *own* failure is not an HTTP failure: the route answers the
//! plugin's status and value, because a `404` from an anchor is the plugin
//! saying "I have no such thing", not the panel saying "I have no such route".

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::get;
use serde_json::json;

use anth_wire::{ErrorKind, PluginPipe, Request, Response, WireError};

use crate::dispatch::Daemon;

/// The router, with the daemon it answers from.
///
/// The daemon is `Arc`ed rather than cloned per request: it owns the compiled
/// wasm modules, and recompiling one per request would put that cost on the
/// request path.
#[derive(Clone)]
pub struct AppState(Arc<Daemon>);

impl AppState {
    pub fn new(daemon: Arc<Daemon>) -> Self {
        Self(daemon)
    }
}

pub fn router(daemon: Arc<Daemon>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/host-info", get(host_info))
        .route("/api/plugins", get(plugins))
        .route("/api/plugins/{id}/call", axum::routing::post(plugin_call))
        .route("/p/{*path}", get(anchor))
        // The browser's form: it names its plugin, so two plugins declaring one
        // path are not a conflict here. `/p/` stays for a path-only caller.
        .route("/api/plugins/{id}/{*path}", get(scoped_anchor))
        .with_state(AppState::new(daemon))
}

async fn healthz() -> impl IntoResponse {
    axum::Json(json!({"status": "ok"}))
}

async fn host_info(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let response = state.0.handle(Request::HostInfo).await;
    match response {
        anth_wire::Response::HostInfo(info) => Ok(axum::Json(info)),
        anth_wire::Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "host-info answered with an unexpected response: {other:?}"
        )))),
    }
}

/// Serves an anchor a plugin declared in its manifest.
///
/// The route is resolved through the registry rather than parsed: a path that
/// no plugin declares is a `404`, and two plugins declaring one path is a
/// conflict the operator has to resolve. The query string becomes the anchor's
/// query, so a plugin that reads `?name=Ana` gets it without the panel knowing
/// what a name is.
async fn anchor(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<HttpResponse, ApiError> {
    let path = format!("/{path}");
    let (entry, _loaded) = state
        .0
        .anchor_owner("GET", &path)
        .map_err(ApiError)?
        .ok_or_else(|| {
            ApiError(WireError::new(
                ErrorKind::NotFound,
                format!("no anchor {path}"),
            ))
        })?;
    let id = entry.id.clone();

    let body = serde_json::json!({
        "method": "GET",
        "path": path,
        "query": parse_query(query.as_deref()),
        "body": serde_json::Value::Null,
    });

    forward(state, id, PluginPipe::Http, body.to_string()).await
}

/// Serves an anchor addressed by plugin id and path.
///
/// This is the route [`@anth/plugin`'s `PluginEndpoints`] builds, and it exists
/// because the unscoped form cannot be used by a widget: two plugins may declare
/// the same path, and a widget knows which plugin it belongs to. `{id}` accepts
/// the plugin's alias too — the registry resolves it, and the response is the
/// canonical plugin's. An id that is not installed and a path that plugin does
/// not declare are both a `404`, so the route cannot be used to enumerate other
/// plugins' anchors.
async fn scoped_anchor(
    State(state): State<AppState>,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<HttpResponse, ApiError> {
    let path = format!("/{path}");
    let (entry, _loaded) = state
        .0
        .anchor_for(&id, "GET", &path)
        .map_err(ApiError)?
        .ok_or_else(|| {
            ApiError(WireError::new(
                ErrorKind::NotFound,
                format!("no anchor {path} in plugin `{id}`"),
            ))
        })?;
    let id = entry.id.clone();

    let body = serde_json::json!({
        "method": "GET",
        "path": path,
        "query": parse_query(query.as_deref()),
        "body": serde_json::Value::Null,
    });

    forward(state, id, PluginPipe::Http, body.to_string()).await
}

/// Turns `?a=1&b=two` into the map the HTTP entry expects.
///
/// `anth_plugin::HttpRequest::query` is a `BTreeMap<String, String>`, so passing
/// the raw query string made every anchor answer `400 not an HTTP request`.
/// Percent-decoding is deliberately not done here: the plugin receives what the
/// client wrote, and a decoder in the panel would be a second place for the same
/// bug to live.
fn parse_query(raw: Option<&str>) -> serde_json::Map<String, serde_json::Value> {
    let mut query = serde_json::Map::new();
    for pair in raw.unwrap_or_default().split('&').filter(|p| !p.is_empty()) {
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        query.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
    query
}

/// Sends one call and turns the plugin's status into the response status.
async fn forward(
    state: AppState,
    id: String,
    pipe: PluginPipe,
    payload: String,
) -> Result<HttpResponse, ApiError> {
    match state
        .0
        .handle(Request::PluginCall { id, pipe, payload })
        .await
    {
        Response::PluginReply { status, value, .. } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            Ok((status, axum::Json(value)).into_response())
        }
        Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "the plugin answered unexpectedly: {other:?}"
        )))),
    }
}

/// Every plugin, including the disabled ones.
async fn plugins(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    match state.0.handle(Request::PluginList).await {
        Response::PluginList { plugins } => Ok(axum::Json(json!({ "plugins": plugins }))),
        Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "the plugin list answered unexpectedly: {other:?}"
        )))),
    }
}

/// Runs one plugin pipe.
///
/// The body carries the command's arguments at the top level, like every other
/// route here, and `pipe` defaults to `direct` because that is the one that
/// needs no translation: the caller already speaks the plugin's vocabulary.
/// `{id}` is the plugin's id or the alias it declared; the daemon resolves
/// either before forwarding, so the plugin never learns which was used.
async fn plugin_call(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Option<axum::Json<CallBody>>,
) -> Result<HttpResponse, ApiError> {
    let call = body.map(|axum::Json(body)| body).unwrap_or_default();
    // The plugin's status is the answer's status. A non-2xx here is the plugin's
    // outcome, delivered faithfully rather than reclassified: a caller that
    // asked the plugin directly would see the same code.
    forward(state, id, call.pipe, call.payload).await
}

#[derive(serde::Deserialize, Default)]
struct CallBody {
    #[serde(default)]
    pipe: PluginPipe,
    /// The payload in the shape the chosen pipe carries. A `direct` call sends
    /// `{"action":..,"value":..}` here; a `cli` call sends the command line.
    #[serde(default)]
    payload: String,
}

/// A [`WireError`] rendered as `{"error": "..."}` with its mapped status.
///
/// The body never carries the kind or a code: v0.1.0 answered two shapes and a
/// client had to handle both. Detail belongs in the message, and the status is
/// in the status line.
pub struct ApiError(pub WireError);

impl IntoResponse for ApiError {
    fn into_response(self) -> HttpResponse {
        let status = StatusCode::from_u16(self.0.kind.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, axum::Json(json!({"error": self.0.message}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn test_router() -> Router {
        router(Arc::new(Daemon::new(
            crate::plugins::registry::Registry::empty(),
        )))
    }

    /// A router with one disabled plugin, which is enough to prove a lookup.
    fn aliased_router() -> Router {
        router(Arc::new(Daemon::new(
            crate::plugins::registry::Registry::with_disabled(
                "org.test.aliased",
                Some("short"),
                &[("GET", "/greeting")],
                "the bundle was rewritten",
            ),
        )))
    }

    async fn get_status_and_body(uri: &str) -> (u16, serde_json::Value) {
        ask(test_router(), uri).await
    }

    async fn ask(router: Router, uri: &str) -> (u16, serde_json::Value) {
        let response = router
            .oneshot(
                HttpRequest::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a readable body");
        let body = serde_json::from_slice(&bytes).expect("the body must be JSON");
        (status, body)
    }

    #[tokio::test]
    async fn healthz_answers_ok_without_touching_the_daemon() {
        let (status, body) = get_status_and_body("/healthz").await;
        assert_eq!(status, 200);
        assert_eq!(body["status"], json!("ok"));
    }

    #[tokio::test]
    async fn host_info_answers_the_same_snapshot_the_socket_returns() {
        let (status, body) = get_status_and_body("/api/host-info").await;

        assert_eq!(status, 200);
        // Read from the running container, so assert shape rather than values.
        assert!(
            body["hostname"].as_str().is_some_and(|s| !s.is_empty()),
            "{body}"
        );
        assert!(
            body["logical_cores"].as_u64().is_some_and(|n| n > 0),
            "{body}"
        );
        assert!(body["os"]["name"].is_string(), "{body}");
    }

    #[test]
    fn a_query_string_becomes_the_map_the_http_entry_expects() {
        let query = parse_query(Some("x=1&y=two&flag"));

        assert_eq!(query["x"], json!("1"));
        assert_eq!(query["y"], json!("two"));
        assert_eq!(query["flag"], json!(""));
    }

    #[test]
    fn an_absent_query_is_an_empty_map() {
        assert!(parse_query(None).is_empty());
        assert!(parse_query(Some("")).is_empty());
    }

    /// An anchor no plugin declares must not fall through to the SPA, which
    /// would answer `200 index.html` and look like a working route.
    #[tokio::test]
    async fn an_unknown_anchor_is_a_404() {
        let (status, body) = get_status_and_body("/p/nope").await;

        assert_eq!(status, 404);
        assert!(body["error"].as_str().unwrap().contains("no anchor"));
    }

    /// The browser builds `<prefix>/<id><path>`, so the id-scoped route has to
    /// exist or every widget's first request is a 404.
    #[tokio::test]
    async fn an_unknown_plugin_anchor_is_a_404_that_names_the_plugin() {
        let (status, body) = get_status_and_body("/api/plugins/org.anth.absent/greeting").await;

        assert_eq!(status, 404);
        let error = body["error"].as_str().unwrap();
        assert!(error.contains("no anchor"), "{error}");
        assert!(error.contains("org.anth.absent"), "{error}");
    }

    /// `{id}` accepts the alias, so a widget built with the short handle reaches
    /// the same plugin. The disabled entry refuses on its *canonical* id, which
    /// is what proves the alias resolved rather than being echoed back.
    #[tokio::test]
    async fn a_scoped_anchor_accepts_the_plugin_alias() {
        let (status, body) = ask(aliased_router(), "/api/plugins/short/greeting").await;

        assert_eq!(status, 403);
        let error = body["error"].as_str().unwrap();
        assert!(error.contains("org.test.aliased"), "{error}");
    }

    /// The call route resolves the same way, because both go through the daemon.
    #[tokio::test]
    async fn a_plugin_call_accepts_the_plugin_alias() {
        let response = aliased_router()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/api/plugins/short/call")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"payload":"greet"}"#))
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        assert_eq!(response.status().as_u16(), 403);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a readable body");
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).expect("the body must be JSON");
        assert!(
            body["error"].as_str().unwrap().contains("org.test.aliased"),
            "{}",
            body["error"]
        );
    }

    #[tokio::test]
    async fn an_unknown_route_is_not_the_spa_fallback() {
        let response = test_router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/nope")
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        // An unmatched path under /api must not be answered by the static
        // fallback with 200 and index.html.
        assert_eq!(response.status().as_u16(), 404);
    }

    #[test]
    fn an_error_renders_its_mapped_status_and_a_flat_message() {
        use anth_wire::ErrorKind;

        let error = ApiError(WireError::new(
            ErrorKind::Forbidden,
            "control is off".to_string(),
        ));
        let response = error.into_response();

        assert_eq!(response.status().as_u16(), 403);
    }
}
