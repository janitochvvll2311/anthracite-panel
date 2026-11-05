//! The engine's HTTP surface.
//!
//! The router is a library value on purpose: the official panel mounts it in its
//! own process (§6.5), and a headless deployment serves it directly.
//!
//! Two routers: the full one, meant for the Unix socket where a peer credential
//! exists, and the public one, which is all a TCP listener serves until sessions
//! arrive. `/health` is public on both; everything under `/api` needs a peer.

use std::sync::Arc;

use anth_compose::grant::GrantStore;
use anth_compose::registry::{Installed, Registry};
use anth_contract::{
    AppId, AppProblem, AppView, Composition, Error, ErrorKind, GrantState, GrantView,
};
use axum::extract::{connect_info::Connected, ConnectInfo, Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::UnixListener;

/// The installed apps and their grants, when this router serves them.
#[derive(Debug, Clone)]
pub struct Apps {
    pub registry: Registry,
    pub grants: GrantStore,
}

/// What the router needs from the engine.
#[derive(Debug, Clone)]
pub struct EngineState {
    pub version: String,
    pub protocol: u32,
    /// The uid that owns the daemon; root is always accepted as well.
    pub allowed_uid: u32,
    /// What is composed. Empty until the operator adds an app.
    pub composition: Arc<Composition>,
    /// The installed apps, when the daemon could open the state tree.
    pub apps: Option<Arc<Apps>>,
}

impl EngineState {
    pub fn new(allowed_uid: u32) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: 1,
            allowed_uid,
            composition: Arc::new(Composition::empty()),
            apps: None,
        }
    }

    /// What the daemon loaded from the composition file.
    pub fn with_composition(mut self, composition: Composition) -> Self {
        self.composition = Arc::new(composition);
        self
    }

    /// The state tree the registry and the grant store read.
    pub fn with_apps(mut self, state: &std::path::Path) -> Self {
        self.apps = Some(Arc::new(Apps {
            registry: Registry::new(state),
            grants: GrantStore::new(state),
        }));
        self
    }
}

/// A marker the embedding host sets once it has authenticated a request.
///
/// A panel that mounts this router receives browser requests that carry no peer
/// credential. It authenticates them and marks the request; the router still owns
/// the decision of what an authenticated request may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trusted;

/// The uid the kernel reports for a socket connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerUid(pub u32);

impl Connected<axum::serve::IncomingStream<'_, UnixListener>> for PeerUid {
    fn connect_info(stream: axum::serve::IncomingStream<'_, UnixListener>) -> Self {
        let uid = stream
            .io()
            .peer_cred()
            .map(|credential| credential.uid())
            .unwrap_or(u32::MAX);
        Self(uid)
    }
}

/// The full surface, for the Unix socket.
pub fn router(state: Arc<EngineState>) -> Router {
    let api = Router::new()
        .route("/health", get(readiness))
        .route("/engine/version", get(version))
        .route("/engine/composition", get(composition))
        .route("/engine/apps", get(list_apps))
        .route("/engine/apps/{id}", get(get_app))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_peer));

    Router::new()
        .route("/health", get(health))
        .nest("/api", api)
        .with_state(state)
}

/// Only what a TCP listener may serve before sessions exist.
pub fn public_router() -> Router {
    Router::new().route("/health", get(health))
}

/// Liveness: 200 once the socket is bound, nothing else.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness: 200 once the surface can serve. There is nothing to start yet, so
/// bound means ready; the moment apps load this answers `503 starting` first.
async fn readiness() -> Json<Value> {
    Json(json!({ "status": "ready" }))
}

/// Readiness and the protocol version.
async fn version(State(state): State<Arc<EngineState>>) -> Json<Value> {
    Json(json!({
        "engine": state.version,
        "protocol": state.protocol,
    }))
}

/// What is composed, plus the surface the composer derived from it.
///
/// With no apps the surface is empty; the projection arrives with the resolver
/// (M1), and this endpoint is what the panel reads either way.
async fn composition(State(state): State<Arc<EngineState>>) -> Json<Value> {
    Json(json!({
        "composition": state.composition.as_ref(),
        "surface": { "apps": [] },
    }))
}

/// The installed catalogue. The grant is only on the detail endpoint; the list
/// stays small. A tree that cannot be read is named instead of hiding the rest.
async fn list_apps(State(state): State<Arc<EngineState>>) -> Result<Json<Value>, ApiError> {
    let apps = apps(&state)?;
    let (installed, problems) = apps.registry.catalogue()?;
    let mut views = Vec::new();
    for app in &installed {
        views.push(app_view(apps, app, false)?);
    }
    let problems: Vec<AppProblem> = problems
        .into_iter()
        .map(|problem| AppProblem {
            app: problem.app,
            message: problem.message,
        })
        .collect();
    Ok(Json(json!({ "apps": views, "problems": problems })))
}

/// One app: descriptor, effective alias and its frozen grant.
async fn get_app(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
) -> Result<Json<AppView>, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let installed = apps
        .registry
        .get(&id)
        .map_err(|error| {
            // A corrupt installed tree is state the operator must repair, not a
            // bad request.
            if error.kind() == ErrorKind::Invalid {
                ApiError(Error::internal(format!(
                    "the installed tree for `{id}` is corrupt: {}",
                    error.message()
                )))
            } else {
                ApiError(error)
            }
        })?
        .ok_or_else(|| ApiError(Error::not_found(format!("`{id}` is not installed"))))?;
    Ok(Json(app_view(apps, &installed, true)?))
}

fn apps(state: &EngineState) -> Result<&Apps, ApiError> {
    state
        .apps
        .as_deref()
        .ok_or_else(|| ApiError(Error::unavailable("this router does not serve apps")))
}

fn app_view(apps: &Apps, installed: &Installed, with_grant: bool) -> Result<AppView, Error> {
    let id = AppId::parse(&installed.manifest.app.id)?;
    let (grant, grant_state, grant_problem) = match apps.grants.load(&id) {
        Ok(Some(grant)) => {
            let state = if installed.grant_matches(&grant) {
                GrantState::Current
            } else {
                GrantState::Stale
            };
            let view = if with_grant {
                Some(GrantView::from(grant))
            } else {
                None
            };
            (view, state, None)
        }
        Ok(None) => (None, GrantState::None, None),
        Err(error) => (None, GrantState::Invalid, Some(error.message().to_string())),
    };
    Ok(AppView {
        id,
        version: installed.manifest.app.version.clone(),
        enabled: installed.descriptor.enabled,
        alias: installed.alias().map(str::to_string),
        signer: installed.descriptor.signer.clone(),
        tier: installed.descriptor.tier,
        installed_at: installed.descriptor.installed_at.clone(),
        grant,
        grant_state,
        grant_problem,
    })
}

/// Every `/api` route needs a peer credential from the service user or root.
async fn require_peer(
    State(state): State<Arc<EngineState>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if request.extensions().get::<Trusted>().is_some() {
        return Ok(next.run(request).await);
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<PeerUid>>()
        .map(|ConnectInfo(uid)| uid.0);
    match peer {
        None => Err(ApiError(Error::unauthenticated(
            "this surface needs a local peer credential",
        ))),
        Some(uid) if uid == 0 || uid == state.allowed_uid => Ok(next.run(request).await),
        Some(_) => Err(ApiError(Error::unauthenticated(
            "the peer is not the service user or root",
        ))),
    }
}

/// An error rendered with the one envelope of §7.11.
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self.0.envelope())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn state_with(uid: u32) -> Arc<EngineState> {
        Arc::new(EngineState::new(uid))
    }

    fn state() -> Arc<EngineState> {
        state_with(1000)
    }

    /// The uid this test process runs as. CI does not run as root, so a fixed
    /// number here would pass locally and fail there.
    fn current_euid() -> u32 {
        // SAFETY: `geteuid` has no preconditions.
        unsafe { libc::geteuid() }
    }

    fn request(uri: &str, peer: Option<u32>) -> Request<Body> {
        let mut request = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        if let Some(uid) = peer {
            request.extensions_mut().insert(ConnectInfo(PeerUid(uid)));
        }
        request
    }

    async fn status(uri: &str, peer: Option<u32>) -> StatusCode {
        router(state())
            .oneshot(request(uri, peer))
            .await
            .expect("response")
            .status()
    }

    fn apps_state() -> (Arc<EngineState>, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
            "anth-api-apps-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let app = dir.join("apps/org.example.notes");
        std::fs::create_dir_all(&app).expect("mkdir");
        for path in [&dir, &dir.join("apps"), &app] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        }
        let manifest = "manifest_version = 2\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n";
        let descriptor = "version = 1\napp = \"org.example.notes\"\nenabled = true\ntier = \"third-party\"\ninstalled_at = \"2026-11-03T20:00:00Z\"\n";
        for (name, text) in [("manifest.toml", manifest), ("descriptor.toml", descriptor)] {
            let path = app.join(name);
            std::fs::write(&path, text).expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        }
        (Arc::new(EngineState::new(1000).with_apps(&dir)), dir)
    }

    fn write_grant(dir: &std::path::Path, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        let grants = dir.join("grants");
        std::fs::create_dir_all(&grants).expect("grants");
        std::fs::set_permissions(&grants, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let path = grants.join("org.example.notes.toml");
        std::fs::write(&path, text).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }

    fn grant_toml(app_version: &str) -> String {
        format!(
            "version = 1\napp = \"org.example.notes\"\napp_version = \"{app_version}\"\nmanifest_sha256 = \"{}\"\nwit_sha256 = \"{}\"\nrisk = \"read\"\napproved_by = \"operator-3f2a\"\napproved_at = \"2026-11-03T20:00:00Z\"\n",
            "a".repeat(64),
            "b".repeat(64)
        )
    }

    async fn json(state: Arc<EngineState>, uri: &str) -> (StatusCode, Value) {
        let response = router(state)
            .oneshot(request(uri, Some(1000)))
            .await
            .expect("response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn the_app_catalogue_lists_installed_apps() {
        let (state, dir) = apps_state();
        let (status, value) = json(state, "/api/engine/apps").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["apps"][0]["id"], "org.example.notes");
        assert_eq!(value["apps"][0]["alias"], "notes");
        assert_eq!(value["apps"][0]["tier"], "third-party");
        assert!(
            value["apps"][0].get("grant").is_none(),
            "the list carries no grant"
        );
        assert_eq!(value["apps"][0]["grant_state"], "none");
        assert!(value["problems"].as_array().expect("problems").is_empty());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_app_detail_carries_its_descriptor() {
        let (state, dir) = apps_state();
        let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["enabled"], true);
        assert!(
            value.get("grant").is_none(),
            "no grant was approved for this app"
        );
        assert_eq!(value["grant_state"], "none");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn the_detail_shows_a_current_grant() {
        let (state, dir) = apps_state();
        write_grant(&dir, &grant_toml("0.1.0"));
        let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["grant_state"], "current");
        assert_eq!(value["grant"]["risk"], "read");
        assert_eq!(value["grant"]["approved_by"], "operator-3f2a");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_stale_grant_is_marked() {
        let (state, dir) = apps_state();
        write_grant(&dir, &grant_toml("9.9.9"));
        let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["grant_state"], "stale");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_invalid_grant_is_marked() {
        let (state, dir) = apps_state();
        write_grant(&dir, "this is not a grant");
        let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["grant_state"], "invalid");
        assert!(value.get("grant").is_none());
        assert!(
            value["grant_problem"]
                .as_str()
                .expect("problem")
                .contains("not valid"),
            "{value}"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn the_catalogue_states_each_grant_without_sending_it() {
        let (state, dir) = apps_state();
        write_grant(&dir, &grant_toml("0.1.0"));
        let (status, value) = json(state, "/api/engine/apps").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["apps"][0]["grant_state"], "current");
        assert!(value["apps"][0].get("grant").is_none());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn the_detail_of_a_corrupt_tree_is_internal() {
        use std::os::unix::fs::PermissionsExt;
        let (state, dir) = apps_state();
        let broken = dir.join("apps/org.example.broken");
        std::fs::create_dir_all(&broken).expect("mkdir");
        std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let manifest = broken.join("manifest.toml");
        std::fs::write(&manifest, b"not a manifest").expect("write");
        std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let (status, value) = json(state, "/api/engine/apps/org.example.broken").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(value["error"]["code"], "internal");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_corrupt_app_does_not_hide_the_catalogue() {
        use std::os::unix::fs::PermissionsExt;
        let (state, dir) = apps_state();
        let broken = dir.join("apps/org.example.broken");
        std::fs::create_dir_all(&broken).expect("mkdir");
        std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let manifest = broken.join("manifest.toml");
        std::fs::write(&manifest, b"not a manifest").expect("write");
        std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let (status, value) = json(state, "/api/engine/apps").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["apps"].as_array().expect("apps").len(), 1);
        assert_eq!(value["problems"][0]["app"], "org.example.broken");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_missing_app_is_not_found() {
        let (state, dir) = apps_state();
        let (status, value) = json(state, "/api/engine/apps/org.example.missing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"]["code"], "not-found");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_invalid_app_id_is_refused() {
        let (state, dir) = apps_state();
        let (status, value) = json(state, "/api/engine/apps/not-an-id").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_router_without_apps_is_unavailable() {
        let (status, value) = json(state(), "/api/engine/apps").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "unavailable");
    }

    #[tokio::test]
    async fn health_is_public() {
        assert_eq!(status("/health", None).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_api_needs_a_peer_credential() {
        assert_eq!(status("/api/health", None).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_service_user_and_root_are_accepted() {
        assert_eq!(status("/api/health", Some(1000)).await, StatusCode::OK);
        assert_eq!(status("/api/health", Some(0)).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_composition_is_served_with_an_empty_surface() {
        let response = router(state())
            .oneshot(request("/api/engine/composition", Some(1000)))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["composition"]["version"], 1);
        assert_eq!(value["composition"]["profile"][0]["id"], "default");
        assert!(value["surface"]["apps"]
            .as_array()
            .expect("apps")
            .is_empty());
    }

    #[tokio::test]
    async fn a_composed_app_is_echoed() {
        use anth_contract::Profile;
        let composition = Composition {
            version: 1,
            profile: vec![Profile {
                id: "default".to_string(),
                apps: vec!["org.example.notes".to_string()],
            }],
        };
        let state = Arc::new(EngineState::new(1000).with_composition(composition));
        let response = router(state)
            .oneshot(request("/api/engine/composition", Some(1000)))
            .await
            .expect("response");
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            value["composition"]["profile"][0]["apps"][0],
            "org.example.notes"
        );
        // The surface stays empty until the resolver projects it (M1); this test
        // pins that the document reaches the endpoint unchanged.
        assert!(value["surface"]["apps"]
            .as_array()
            .expect("apps")
            .is_empty());
    }

    #[tokio::test]
    async fn readiness_is_a_different_word_than_liveness() {
        let response = router(state())
            .oneshot(request("/api/health", Some(1000)))
            .await
            .expect("response");
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["status"], "ready");
    }

    #[tokio::test]
    async fn a_trusted_in_process_request_is_accepted() {
        let mut request = request("/api/health", None);
        request.extensions_mut().insert(Trusted);
        let response = router(state()).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_real_socket_connection_carries_the_peer_uid() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::env::temp_dir().join(format!("anth-api-uds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("engine.sock");

        let listener = UnixListener::bind(&path).expect("bind");
        let app =
            router(state_with(current_euid())).into_make_service_with_connect_info::<PeerUid>();
        let server = tokio::spawn(async move { axum::serve(listener, app).await });

        let mut stream = tokio::net::UnixStream::connect(&path)
            .await
            .expect("connect");
        stream
            .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        let text = String::from_utf8_lossy(&response);
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.contains("ready"), "{text}");

        server.abort();
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn any_other_peer_is_refused() {
        assert_eq!(
            status("/api/health", Some(4242)).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn version_reports_the_protocol() {
        let response = router(state())
            .oneshot(request("/api/engine/version", Some(1000)))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["protocol"], 1);
    }

    #[tokio::test]
    async fn the_public_router_serves_only_health() {
        let public = public_router();
        let ok = public
            .clone()
            .oneshot(request("/health", None))
            .await
            .expect("response");
        assert_eq!(ok.status(), StatusCode::OK);
        let missing = public
            .oneshot(request("/api/health", None))
            .await
            .expect("response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }
}
