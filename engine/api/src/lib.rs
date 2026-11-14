//! The engine's HTTP surface.
//!
//! The router is a library value on purpose: the official panel mounts it in its
//! own process (§6.5), and a headless deployment serves it directly.
//!
//! Two routers: the full one, meant for the Unix socket where a peer credential
//! exists, and the session-authenticated one a TCP listener serves. `/health` is
//! public on both; everything under `/api` needs a peer or a session.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

pub use anth_auth::{Auth, SessionInfo};
use anth_compose::audit::{timestamp, AuditLog};
use anth_compose::bundle::Options as BundleOptions;
use anth_compose::composition::CompositionStore;
use anth_compose::grant::GrantStore;
use anth_compose::keys::Distribution;
use anth_compose::registry::{Installed, Registry};
use anth_compose::risk::{app_risk, approve, Approval};
use anth_compose::{composed_dependents, resolve, DegradedReason, InstalledApp, Resolution};
use anth_contract::manifest::{parse_call, RouteTransport, SlotSection};
use anth_contract::{
    AppId, AppProblem, AppView, AuditAction, AuditEntry, Composition, CompositionView, DegradedApp,
    Error, ErrorKind, FatalView, Grant, GrantState, GrantView, InterfaceId, KeyAlgorithm, KeyView,
    Outcome, Risk, SurfaceApp, SurfaceCli, SurfaceFabAction, SurfaceLink, SurfaceRoute,
    SurfaceSlot, SurfaceView, TrustedKey, KEY_VERSION,
};
use axum::body::Bytes;
use axum::extract::{
    connect_info::Connected, ws::Message, ws::WebSocket, ws::WebSocketUpgrade, ConnectInfo,
    DefaultBodyLimit, Extension, FromRequestParts, Path, Query, RawQuery, Request, State,
};
use axum::http::header::{
    ALLOW, CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_MATCH, IF_NONE_MATCH, LOCATION,
};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get};
use axum::{Json, Router};
use futures_core::Stream;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::UnixListener;

/// The installed apps and their grants, when this router serves them.
#[derive(Debug, Clone)]
pub struct Apps {
    /// The state tree this registry reads, kept so a host can pin the
    /// distribution mode before it starts serving.
    pub state: std::path::PathBuf,
    pub registry: Registry,
    pub grants: GrantStore,
    pub audit: AuditLog,
    /// `apps.max-bundle-bytes` and `apps.require-signature`.
    pub max_bundle_bytes: u64,
    pub require_signature: bool,
    /// `services.allow-third-party`: the startup opt-in for native services.
    pub allow_third_party_service: bool,
    /// The composition file under the same state root.
    pub compositions: CompositionStore,
}

impl Apps {
    fn options(&self) -> BundleOptions {
        BundleOptions {
            max_bundle_bytes: self.max_bundle_bytes,
            require_signature: self.require_signature,
            allow_third_party_service: self.allow_third_party_service,
            known_packages: Vec::new(),
        }
    }
}

/// What the router needs from the engine.
#[derive(Clone)]
pub struct EngineState {
    pub version: String,
    pub protocol: u32,
    /// The uid that owns the daemon; root is always accepted as well.
    pub allowed_uid: u32,
    /// What is composed. The same document the daemon loaded from disk.
    pub composition: Arc<RwLock<Composition>>,
    /// Serializes a composition replacement so the `If-Match` check and the
    /// write cannot interleave with another replacement.
    compose_lock: Arc<tokio::sync::Mutex<()>>,
    /// The installed apps, when the daemon could open the state tree.
    pub apps: Option<Arc<Apps>>,
    /// The operator credentials and the live TCP sessions, once the host has
    /// opened the state tree.
    pub auth: Option<Arc<Auth>>,
    /// `false` while the daemon boots apps; readiness answers `503 starting`.
    ready: Arc<AtomicBool>,
    /// The route dispatcher, when this host runs apps.
    caller: Option<Arc<dyn AppCaller>>,
}

impl EngineState {
    pub fn new(allowed_uid: u32) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: 1,
            allowed_uid,
            composition: Arc::new(RwLock::new(Composition::empty())),
            compose_lock: Arc::new(tokio::sync::Mutex::new(())),
            apps: None,
            auth: None,
            ready: Arc::new(AtomicBool::new(true)),
            caller: None,
        }
    }

    pub fn with_caller(mut self, caller: Arc<dyn AppCaller>) -> Self {
        self.caller = Some(caller);
        self
    }

    /// Opens the operator credentials and the session store. A host that mounts
    /// the router without a state tree omits this; login then answers `503`.
    pub fn with_auth(mut self, auth: Arc<Auth>) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Drops the compiled distribution key on the app registry.
    ///
    /// It can only remove trust, never grant it: a host that calls this cannot
    /// mint a `system` app, it can only stop honouring the key the binary was
    /// built with. The daemon does not call it; tests use it to model a keyless
    /// development build, and a host that wants one says so explicitly.
    pub fn keyless(mut self) -> Self {
        if let Some(apps) = self.apps.take() {
            let mut apps = Arc::try_unwrap(apps)
                .unwrap_or_else(|_| panic!("`keyless` must be called before the state is shared"));
            apps.registry = Registry::with_distribution(&apps.state, Distribution::none());
            self.apps = Some(Arc::new(apps));
        }
        self
    }

    /// The daemon is still starting apps; readiness answers `503` until
    /// [`EngineState::mark_ready`].
    pub fn booting(mut self) -> Self {
        self.ready = Arc::new(AtomicBool::new(false));
        self
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// What the daemon loaded from the composition file.
    pub fn with_composition(mut self, composition: Composition) -> Self {
        self.composition = Arc::new(RwLock::new(composition));
        self
    }

    /// The state tree the registry and the grant store read.
    pub fn with_apps(
        mut self,
        state: &std::path::Path,
        max_bundle_bytes: u64,
        require_signature: bool,
        allow_third_party_service: bool,
    ) -> Self {
        self.apps = Some(Arc::new(Apps {
            state: state.to_path_buf(),
            registry: Registry::new(state),
            grants: GrantStore::new(state),
            audit: AuditLog::new(state),
            max_bundle_bytes,
            require_signature,
            allow_third_party_service,
            compositions: CompositionStore::new(state),
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

/// Who performed the request, for the audit entry.
#[derive(Debug, Clone)]
pub struct Actor(pub String);

/// How a request proved who it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// The kernel reported the service user or root.
    Peer,
    /// The embedding panel authenticated the browser and marked the request.
    Trusted,
    /// A bearer token minted by `POST /api/engine/session`.
    Session,
}

/// What the auth middleware learned about this request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub actor: String,
    pub kind: AuthKind,
    pub must_change_password: bool,
    pub session: Option<SessionInfo>,
    pub token: Option<String>,
}

/// The source a login attempt came from. The TCP listener supplies the peer
/// address; the socket and the in-process panel have none.
#[derive(Debug, Clone)]
pub struct SourceAddress(pub String);

async fn remember_source(mut request: Request, next: Next) -> Response {
    let source = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip().to_string())
        .unwrap_or_else(|| "in-process".to_string());
    request.extensions_mut().insert(SourceAddress(source));
    next.run(request).await
}

impl AuthContext {
    fn peer(actor: String, kind: AuthKind) -> Self {
        Self {
            actor,
            kind,
            must_change_password: false,
            session: None,
            token: None,
        }
    }
}

type CallFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + 'a>>;

/// One chunk of a streamed route response (§20).
pub type Chunk = Vec<u8>;

/// The body of a streamed route: chunks until the producer ends cleanly, or one
/// error that truncates the response after whatever the client already read.
pub type ChunkStream = Pin<Box<dyn futures_core::Stream<Item = Result<Chunk, Error>> + Send>>;

/// Resolving a streamed call yields either the error that precedes the first
/// byte, which becomes the normal JSON envelope, or the chunk stream.
pub type StreamFuture<'a> = Pin<Box<dyn Future<Output = Result<ChunkStream, Error>> + Send + 'a>>;

/// The per-stream budget from §7.7, used when the manifest omits it.
const STREAM_BYTES_TOTAL: u64 = 8 * 1024 * 1024;
const STREAM_BYTES_PER_SEC: u64 = 1024 * 1024;

/// Wraps a stream in the manifest's byte budget (§7.7, §20).
fn limited_stream(chunks: ChunkStream, installed: &InstalledApp) -> ChunkStream {
    Box::pin(LimitedStream {
        inner: chunks,
        budget: StreamBudget {
            total: installed
                .manifest
                .limits
                .bytes_total
                .unwrap_or(STREAM_BYTES_TOTAL),
            per_sec: installed
                .manifest
                .limits
                .bytes_per_sec
                .unwrap_or(STREAM_BYTES_PER_SEC),
        },
        total: 0,
        window_start: Instant::now(),
        window_bytes: 0,
        done: false,
    })
}

/// Frames each stream chunk as one SSE `data:` event (§20). The producer emits
/// one complete JSON value per chunk.
struct SseStream {
    inner: ChunkStream,
    done: bool,
}

impl Stream for SseStream {
    type Item = Result<Chunk, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        match Pin::new(&mut this.inner).poll_next(context) {
            Poll::Ready(Some(Ok(chunk))) => {
                let mut event = Vec::with_capacity(chunk.len() + 16);
                for line in chunk.split(|byte| *byte == b'\n') {
                    event.extend_from_slice(b"data: ");
                    event.extend_from_slice(line);
                    event.push(b'\n');
                }
                event.push(b'\n');
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.done = true;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                this.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The route dispatcher's caller is a trait object; the socket task owns its
/// clone so the engine can drop the request future safely.
type SharedCaller = Arc<dyn AppCaller>;

/// Drives one WebSocket route: downstream chunks become binary messages and, if
/// the route names a `write` call, client binary frames become its `data` input.
async fn socket_route(
    socket: WebSocket,
    mut chunks: ChunkStream,
    caller: SharedCaller,
    app: String,
    write: Option<(String, String)>,
    input: Value,
) {
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
            chunk = chunks.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        if sender.send(Message::Binary(bytes.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(_)) => {
                        // The body failed; a non-normal close tells the client
                        // this was not a clean end of the stream.
                        let _ = sender
                            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                                code: 1011,
                                reason: axum::extract::ws::Utf8Bytes::from_static(
                                    "stream failed",
                                ),
                            })))
                            .await;
                        break;
                    }
                    None => break,
                }
            }
            message = receiver.next() => {
                match message {
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Some((interface, function)) = &write {
                            let mut call_input = input.clone();
                            if let Value::Object(map) = &mut call_input {
                                map.insert(
                                    "data".to_string(),
                                    Value::Array(
                                        bytes
                                            .iter()
                                            .map(|byte| Value::from(*byte))
                                            .collect(),
                                    ),
                                );
                            }
                            // A refused write leaves the output side alive; the
                            // caller sees the refusal on its next control call.
                            let _ = caller
                                .call(&app, interface, function, call_input)
                                .await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
    let _ = sender.close().await;
}

struct StreamBudget {
    total: u64,
    per_sec: u64,
}

/// Enforces the manifest's byte budget while a stream is consumed (§7.7, §20).
/// A violation truncates the body; the client sees a failed producer, not a
/// clean EOF.
struct LimitedStream {
    inner: ChunkStream,
    budget: StreamBudget,
    total: u64,
    window_start: Instant,
    window_bytes: u64,
    done: bool,
}

impl Stream for LimitedStream {
    type Item = Result<Chunk, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        match Pin::new(&mut this.inner).poll_next(context) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.total = this.total.saturating_add(chunk.len() as u64);
                if this.total > this.budget.total {
                    this.done = true;
                    return Poll::Ready(Some(Err(Error::too_large(format!(
                        "the stream exceeded its {} byte budget",
                        this.budget.total
                    )))));
                }
                let now = Instant::now();
                if now.duration_since(this.window_start) >= Duration::from_secs(1) {
                    this.window_start = now;
                    this.window_bytes = 0;
                }
                this.window_bytes = this.window_bytes.saturating_add(chunk.len() as u64);
                if this.window_bytes > this.budget.per_sec {
                    this.done = true;
                    return Poll::Ready(Some(Err(Error::exhausted(format!(
                        "the stream exceeded its {} byte/s budget",
                        this.budget.per_sec
                    )))));
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.done = true;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                this.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The engine's route dispatcher: the API resolves a route to `interface#function`
/// and this calls the running app. The daemon implements it over the supervisor;
/// a host that mounts the router without running apps simply omits it.
pub trait AppCaller: Send + Sync {
    fn call<'a>(
        &'a self,
        app: &'a str,
        interface: &'a str,
        function: &'a str,
        input: Value,
    ) -> CallFuture<'a>;

    /// Starts a route declared `stream = true` (§20). The default keeps callers
    /// that only buffer honest: the route answers `unavailable` instead of
    /// pretending a handle is JSON.
    fn stream<'a>(
        &'a self,
        _app: &'a str,
        _interface: &'a str,
        _function: &'a str,
        _input: Value,
    ) -> StreamFuture<'a> {
        Box::pin(async move { Err(Error::unavailable("this caller does not stream")) })
    }
}

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

/// The `/api` surface without an authentication layer.
///
/// The daemon and the linked panel mount it over a Unix socket (peer
/// credentials) or in-process (`Trusted`); a TCP listener mounts
/// [`session_router`], which authenticates with a bearer session instead.
fn api_routes(state: Arc<EngineState>) -> Router {
    Router::new()
        .route("/health", get(readiness))
        .route("/engine/version", get(version))
        .route(
            "/engine/session",
            get(read_session)
                .post(login_session)
                .delete(logout_session)
                .layer(DefaultBodyLimit::max(SESSION_BODY_LIMIT as usize)),
        )
        .route(
            "/engine/operator",
            get(read_operator)
                .patch(change_operator_password)
                .layer(DefaultBodyLimit::max(SESSION_BODY_LIMIT as usize)),
        )
        .route(
            "/engine/composition",
            get(composition).put(replace_composition),
        )
        .route("/engine/keys", get(list_keys).post(add_key))
        .route("/engine/keys/distribution", get(distribution_key))
        .route("/engine/keys/{fingerprint}", delete(remove_key))
        .route("/engine/audit", get(read_audit))
        .route("/engine/apps", get(list_apps).post(install_app))
        .route("/engine/apps/{id}/wit", get(app_wit))
        .route("/engine/apps/{id}/approval", get(preview_approval))
        .route(
            "/engine/apps/{id}",
            get(get_app)
                .put(replace_app)
                .delete(remove_app)
                .patch(patch_app),
        )
        .route(
            "/engine/apps/{id}/grant",
            get(get_grant).put(approve_app).delete(revoke_grant),
        )
        .route("/apps/{id}/assets/{*module}", get(app_asset))
        .route("/apps/{id}/{*path}", any(app_route))
        .with_state(state)
}

/// The `/api` surface over peer credentials, used by the Unix socket and by an
/// embedding panel that marks its already-authenticated requests `Trusted`.
pub fn api_router(state: Arc<EngineState>) -> Router {
    api_routes(Arc::clone(&state))
        .route_layer(middleware::from_fn(password_change_gate))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            require_peer,
        ))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            require_origin,
        ))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(remember_source))
}

/// The `/api` surface over sessions, for a TCP listener.
pub fn session_router(state: Arc<EngineState>) -> Router {
    api_routes(Arc::clone(&state))
        .route_layer(middleware::from_fn(password_change_gate))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            require_session,
        ))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            require_origin,
        ))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn(remember_source))
}

/// The full surface, for the Unix socket.
pub fn router(state: Arc<EngineState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api", api_router(state))
}

/// What a TCP listener serves: liveness plus the session-authenticated API.
pub fn public_router(state: Arc<EngineState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api", session_router(state))
}

/// Liveness: 200 once the socket is bound, nothing else.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness: 200 once the surface can serve. There is nothing to start yet, so
/// bound means ready; the moment apps load this answers `503 starting` first.
async fn readiness(State(state): State<Arc<EngineState>>) -> Response {
    if state.is_ready() {
        (StatusCode::OK, Json(json!({ "status": "ready" }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "starting" })),
        )
            .into_response()
    }
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
/// Resolution is the same function the boot uses, so the panel reads why an app
/// is not running instead of guessing from logs (§8.5). The `ETag` is the strong
/// validator a replacement must echo in `If-Match`.
async fn composition(State(state): State<Arc<EngineState>>) -> Result<Response, ApiError> {
    let apps = apps(&state)?;
    let composition = current_composition(&state)?;
    let installed = installed_apps(apps)?;
    let surface = match resolve(&composition, &installed) {
        Ok(resolution) => surface_view(&resolution, &installed).map_err(ApiError)?,
        Err(error) => fatal_surface(error),
    };
    let view = CompositionView {
        composition: composition.clone(),
        surface,
    };
    let etag = apps.compositions.etag(&composition)?;
    Ok(([(ETAG, etag)], Json(view)).into_response())
}

/// Replaces the whole document. The body is the composition itself, not the
/// `CompositionView` the `GET` answers.
async fn replace_composition(
    State(state): State<Arc<EngineState>>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    Json(composition): Json<Composition>,
) -> Result<Response, ApiError> {
    let apps = apps(&state)?;
    // The lock is what makes `If-Match` more than advice: two replacements that
    // read the same validator cannot both win.
    let _guard = state.compose_lock.lock().await;
    let current = current_composition(&state)?;
    let etag = apps.compositions.etag(&current)?;
    if !if_matches(&headers, &etag) {
        return Err(ApiError(Error::precondition_failed(
            "If-Match does not match the current composition",
        )));
    }
    composition.validate().map_err(ApiError)?;
    let installed = installed_apps(apps)?;
    let resolution =
        resolve(&composition, &installed).map_err(|error| ApiError(error.into_error()))?;
    apps.compositions.save(&composition)?;
    *state
        .composition
        .write()
        .map_err(|_| ApiError(Error::internal("the composition lock is poisoned")))? =
        composition.clone();
    audit(
        apps,
        &actor.0,
        AuditAction::Compose,
        None,
        Outcome::Ok,
        Some(json!({"apps": composition.profile[0].apps})),
    )?;
    let view = CompositionView {
        composition: composition.clone(),
        surface: surface_view(&resolution, &installed).map_err(ApiError)?,
    };
    let etag = apps.compositions.etag(&composition)?;
    Ok((StatusCode::OK, [(ETAG, etag)], Json(view)).into_response())
}

/// The current document, or an internal error if a panicking writer poisoned the
/// lock. Reading it is short and never awaits.
fn current_composition(state: &EngineState) -> Result<Composition, ApiError> {
    state
        .composition
        .read()
        .map(|guard| guard.clone())
        .map_err(|_| ApiError(Error::internal("the composition lock is poisoned")))
}

/// A comma-list header (`If-Match`, `If-None-Match`, the acknowledgment token)
/// split and trimmed once, including repeated header lines.
fn header_tokens<K: axum::http::header::AsHeaderName>(
    headers: &HeaderMap,
    name: K,
) -> impl Iterator<Item = &str> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
}

/// `If-Match` is required and uses strong comparison: `*` matches any existing
/// document, a comma-separated list matches if any validator does, and repeated
/// header lines are the same list.
fn if_matches(headers: &HeaderMap, etag: &str) -> bool {
    header_tokens(headers, IF_MATCH).any(|candidate| candidate == "*" || candidate == etag)
}

/// The resolution as a client reads it (§8.7).
///
/// A running app always has an installed manifest and an effective risk; if that
/// invariant breaks, the answer is an internal error rather than a surface that
/// invents `read` or an empty version.
fn surface_view(resolution: &Resolution, installed: &[InstalledApp]) -> Result<SurfaceView, Error> {
    let installed_by_id: BTreeMap<AppId, &InstalledApp> = installed
        .iter()
        .filter_map(|app| AppId::parse(&app.manifest.app.id).ok().map(|id| (id, app)))
        .collect();
    let mut apps = Vec::with_capacity(resolution.order.len());
    for id in &resolution.order {
        let found = installed_by_id
            .get(id)
            .copied()
            .ok_or_else(|| Error::internal(format!("`{id}` runs but has no installed manifest")))?;
        let risk = resolution
            .risk
            .get(id)
            .copied()
            .ok_or_else(|| Error::internal(format!("`{id}` runs but has no effective risk")))?;
        let routes = found
            .manifest
            .route
            .iter()
            .map(|route| {
                let (interface, function) = parse_call(&route.call)?;
                Ok(SurfaceRoute {
                    method: route.method.clone(),
                    path: route.path.clone(),
                    interface: InterfaceId::parse(&interface)?,
                    function,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let slots = found.manifest.slot.iter().map(surface_slot).collect();
        let cli = found
            .manifest
            .cli
            .iter()
            .map(|verb| {
                let (interface, function) = parse_call(&verb.call)?;
                Ok(SurfaceCli {
                    verb: verb.verb.clone(),
                    interface: InterfaceId::parse(&interface)?,
                    function,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        apps.push(SurfaceApp {
            id: id.clone(),
            version: found.manifest.app.version.clone(),
            icon: found.manifest.app.icon.clone(),
            icon_sha256: found.manifest.app.icon_sha256.clone(),
            risk,
            routes,
            slots,
            cli,
        });
    }
    let degraded = resolution
        .degraded
        .iter()
        .map(|(id, degraded)| DegradedApp {
            app: id.clone(),
            version: installed_by_id
                .get(id)
                .map(|app| app.manifest.app.version.clone()),
            reason: reason_code(&degraded.reason).to_string(),
            detail: reason_detail(&degraded.reason),
        })
        .collect();
    let links = resolution
        .links
        .iter()
        .map(|link| SurfaceLink {
            consumer: link.consumer.clone(),
            interface: link.interface.clone(),
            provider: match &link.provider {
                anth_compose::Provider::Engine => "engine".to_string(),
                anth_compose::Provider::App(app) => app.to_string(),
            },
        })
        .collect();
    Ok(SurfaceView {
        apps,
        degraded,
        links,
        fatal: None,
    })
}

/// A fatal resolution is part of the view, not an HTTP failure: the operator
/// needs the document back to remove the app that cannot resolve.
fn fatal_surface(error: anth_compose::ResolutionError) -> SurfaceView {
    let envelope = error.into_error().envelope();
    SurfaceView {
        apps: Vec::new(),
        degraded: Vec::new(),
        links: Vec::new(),
        fatal: Some(FatalView {
            code: envelope.error.code,
            message: envelope.error.message,
            detail: envelope.error.detail,
        }),
    }
}

/// The kebab-case reason of §8.5.
fn reason_code(reason: &DegradedReason) -> &'static str {
    reason.code()
}

/// What the code alone does not say: which link is blocked or how far the risk
/// grew.
fn reason_detail(reason: &DegradedReason) -> Option<Value> {
    match reason {
        DegradedReason::Blocked {
            interface,
            provider,
        } => Some(json!({"interface": interface, "provider": provider})),
        DegradedReason::RiskGrown {
            approved,
            effective,
        } => Some(json!({"approved": approved, "effective": effective})),
        DegradedReason::Unapproved
        | DegradedReason::GrantStale
        | DegradedReason::GrantInvalid
        | DegradedReason::Disabled => None,
    }
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

/// The trusted signers. The keyring can extend trust, never privilege (§9.1).
async fn list_keys(State(state): State<Arc<EngineState>>) -> Result<Json<Value>, ApiError> {
    let apps = apps(&state)?;
    let keys: Vec<KeyView> = apps
        .registry
        .keyring()
        .all()?
        .into_iter()
        .map(|(fingerprint, key)| KeyView { fingerprint, key })
        .collect();
    Ok(Json(json!({ "keys": keys })))
}

/// The distribution key compiled into this binary, if any (§9.1).
///
/// It is public by definition: an operator compares the fingerprint against the
/// release notes to know which builds can install `system` bundles.
async fn distribution_key(State(state): State<Arc<EngineState>>) -> Result<Json<Value>, ApiError> {
    let apps = apps(&state)?;
    let distribution = apps.registry.distribution();
    Ok(Json(json!({
        "fingerprint": distribution.fingerprint(),
        "public_key": distribution.public_key(),
        "problem": distribution.problem(),
    })))
}

const AUDIT_LIMIT: usize = 100;
const AUDIT_LIMIT_MAX: usize = 1000;

#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<usize>,
    before: Option<usize>,
}

/// The audit log, newest first. `before` is the index from which to page back;
/// the response's `next` is the value for the next request.
async fn read_audit(
    State(state): State<Arc<EngineState>>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Value>, ApiError> {
    let apps = apps(&state)?;
    let (entries, problems) = apps.audit.scan()?;
    let total = entries.len();
    let before = query.before.unwrap_or(total).min(total);
    let limit = query.limit.unwrap_or(AUDIT_LIMIT);
    if !(1..=AUDIT_LIMIT_MAX).contains(&limit) {
        return Err(ApiError(Error::invalid(format!(
            "`limit` must be between 1 and {AUDIT_LIMIT_MAX}"
        ))));
    }
    let start = before.saturating_sub(limit);
    let mut page = entries[start..before].to_vec();
    page.reverse();
    let next = (start > 0).then_some(start);
    Ok(Json(json!({
        "entries": page,
        "total": total,
        "next": next,
        "limit": limit,
        "before": before,
        "problems": problems,
    })))
}

/// The public key is the request; the engine fills the operator and the time.
/// Re-adding the same key is idempotent and answers `200` instead of `201`.
async fn add_key(
    State(state): State<Arc<EngineState>>,
    Extension(actor): Extension<Actor>,
    Json(body): Json<KeyBody>,
) -> Result<Response, ApiError> {
    let apps = apps(&state)?;
    let key = TrustedKey {
        version: KEY_VERSION,
        algorithm: KeyAlgorithm::Ed25519,
        public_key: body.public_key,
        label: body.label,
        added_by: actor.0.clone(),
        added_at: timestamp(),
    };
    let fingerprint = anth_compose::signature::fingerprint(&key.public_key)?;
    if let Some(existing) = apps.registry.keyring().load(&fingerprint)? {
        // A retried add changes nothing: the first document (label, operator and
        // timestamp) is the trusted one.
        return Ok((
            StatusCode::OK,
            [(LOCATION, format!("/api/engine/keys/{fingerprint}"))],
            Json(KeyView {
                fingerprint,
                key: existing,
            }),
        )
            .into_response());
    }
    let fingerprint = apps.registry.keyring().add(&key)?;
    audit(
        apps,
        &actor.0,
        AuditAction::KeyAdd,
        None,
        Outcome::Ok,
        Some(json!({"fingerprint": fingerprint})),
    )?;
    Ok((
        StatusCode::CREATED,
        [(LOCATION, format!("/api/engine/keys/{fingerprint}"))],
        Json(KeyView { fingerprint, key }),
    )
        .into_response())
}

async fn remove_key(
    State(state): State<Arc<EngineState>>,
    Path(fingerprint): Path<String>,
    Extension(actor): Extension<Actor>,
) -> Result<StatusCode, ApiError> {
    let apps = apps(&state)?;
    if !apps.registry.keyring().remove(&fingerprint)? {
        return Err(ApiError(Error::not_found(format!(
            "`{fingerprint}` is not a trusted key"
        ))));
    }
    audit(
        apps,
        &actor.0,
        AuditAction::KeyRemove,
        None,
        Outcome::Ok,
        Some(json!({"fingerprint": fingerprint})),
    )?;
    Ok(StatusCode::NO_CONTENT)
}

/// A manifest route: `/api/apps/{id}/{path}` mapped to `interface#function`
/// (§7.13). Query parameters are the input on `GET`/`HEAD`/`DELETE`; a JSON
/// object body is the input otherwise.
async fn app_route(
    State(state): State<Arc<EngineState>>,
    Path((id, path)): Path<(String, String)>,
    method: Method,
    RawQuery(query): RawQuery,
    request: Request,
) -> Result<Response, ApiError> {
    let apps = apps(&state)?;
    let installed = installed_apps(apps)?;
    let app = AppId::parse(&id)?;
    let found = installed
        .iter()
        .find(|installed| installed.manifest.app.id == id)
        .ok_or_else(|| ApiError(Error::not_found(format!("`{id}` is not installed"))))?;
    let wanted = format!("/{path}");
    let routes: Vec<&anth_contract::manifest::RouteSection> = found
        .manifest
        .route
        .iter()
        .filter(|route| route.path == wanted)
        .collect();
    if routes.is_empty() {
        return Err(ApiError(Error::not_found(format!(
            "`{id}` has no route {wanted}"
        ))));
    }
    let mut methods: Vec<&str> = routes.iter().map(|route| route.method.as_str()).collect();
    methods.sort_unstable();
    methods.dedup();
    methods.push("OPTIONS");
    let allow = methods.join(", ");
    if method == Method::OPTIONS {
        return Ok((StatusCode::NO_CONTENT, [(ALLOW, allow)]).into_response());
    }
    let route = match routes.iter().find(|route| route.method == method.as_str()) {
        Some(route) => route,
        None => {
            let error =
                Error::method_not_allowed(format!("`{id}` does not allow {method} {wanted}"));
            return Ok((
                StatusCode::METHOD_NOT_ALLOWED,
                [(ALLOW, allow)],
                Json(error.envelope()),
            )
                .into_response());
        }
    };
    let composition = current_composition(&state)?;
    let resolution =
        resolve(&composition, &installed).map_err(|error| ApiError(error.into_error()))?;
    if !resolution.runs(&app) {
        return Err(ApiError(Error::unavailable(format!(
            "`{id}` is not running"
        ))));
    }
    let (interface, function) = parse_call(&route.call)?;
    let caller = state
        .caller
        .as_ref()
        .ok_or_else(|| ApiError(Error::unavailable("this router does not run apps")))?;
    if route.transport == RouteTransport::Websocket {
        let input = query_input(query.as_deref());
        let chunks = caller
            .stream(app.as_str(), &interface, &function, input.clone())
            .await?;
        let chunks = limited_stream(chunks, found);
        let write = route
            .write
            .as_ref()
            .map(|write| parse_call(write))
            .transpose()?;
        let (mut parts, _) = request.into_parts();
        let upgrade = WebSocketUpgrade::from_request_parts(&mut parts, &state)
            .await
            .map_err(|_| ApiError(Error::invalid("this route needs a WebSocket upgrade")))?;
        let caller = Arc::clone(caller);
        let app = app.to_string();
        return Ok(upgrade
            .on_upgrade(move |socket| socket_route(socket, chunks, caller, app, write, input)));
    }
    let no_body = matches!(method, Method::GET | Method::HEAD | Method::DELETE);
    let body = read_body(request, 1 << 20).await?;
    if no_body && !body.is_empty() {
        return Err(ApiError(Error::invalid(
            "a GET, HEAD or DELETE route takes no body",
        )));
    }
    let input = if no_body {
        query_input(query.as_deref())
    } else if body.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        let value: Value = serde_json::from_slice(&body)
            .map_err(|error| Error::invalid(format!("the body is not JSON: {error}")))?;
        if !value.is_object() {
            return Err(ApiError(Error::invalid(
                "a route body is a JSON object keyed by parameter",
            )));
        }
        value
    };
    if route.stream {
        if method == Method::HEAD {
            return Err(ApiError(Error::method_not_allowed(
                "a stream route cannot answer HEAD",
            )));
        }
        let chunks = caller
            .stream(app.as_str(), &interface, &function, input)
            .await?;
        let chunks = limited_stream(chunks, found);
        if route.events {
            let chunks: ChunkStream = Box::pin(SseStream {
                inner: chunks,
                done: false,
            });
            return Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "text/event-stream")
                .header(CACHE_CONTROL, "no-store")
                .header("x-accel-buffering", "no")
                .body(axum::body::Body::from_stream(chunks))
                .map_err(|error| {
                    ApiError(Error::internal(format!("building the response: {error}")))
                });
        }
        return Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CACHE_CONTROL, "no-store")
            .body(axum::body::Body::from_stream(chunks))
            .map_err(|error| ApiError(Error::internal(format!("building the response: {error}"))));
    }
    let output = caller
        .call(app.as_str(), &interface, &function, input)
        .await?;
    Ok(Json(output).into_response())
}

/// Query keys repeat for `list<T>` and stay strings; the runtime coerces each
/// one to the WIT parameter type (§7.13).
fn query_input(query: Option<&str>) -> Value {
    let mut collected: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(query) = query {
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            collected
                .entry(key.into_owned())
                .or_default()
                .push(value.into_owned());
        }
    }
    let mut object = serde_json::Map::new();
    for (key, mut values) in collected {
        let forced_list = key.ends_with("[]");
        let key = key.trim_end_matches("[]").to_string();
        let value = if values.len() == 1 && !forced_list {
            Value::String(values.remove(0))
        } else {
            Value::Array(values.into_iter().map(Value::String).collect())
        };
        object.insert(key, value);
    }
    Value::Object(object)
}

/// A slot's UI module, hash-pinned and immutable (§6.4, §7.7). Only a module
/// the running app's manifest declares can be requested; the bytes are checked
/// against the manifest hash before they are served.
async fn app_asset(
    State(state): State<Arc<EngineState>>,
    Path((id, module)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    use sha2::{Digest, Sha256};
    let apps = apps(&state)?;
    let installed = installed_apps(apps)?;
    let composition = current_composition(&state)?;
    let resolution =
        resolve(&composition, &installed).map_err(|error| ApiError(error.into_error()))?;
    let app = AppId::parse(&id)?;
    if !resolution.runs(&app) {
        return Err(ApiError(Error::unavailable(format!(
            "`{id}` is not running"
        ))));
    }
    let found = installed
        .iter()
        .find(|installed| installed.manifest.app.id == id)
        .ok_or_else(|| ApiError(Error::not_found(format!("`{id}` is not installed"))))?;
    let sha256 = found.manifest.asset_sha256(&module).ok_or_else(|| {
        ApiError(Error::not_found(format!(
            "`{id}` declares no asset `{module}`"
        )))
    })?;
    let relative = std::path::Path::new(&module);
    if relative
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ApiError(Error::internal(format!(
            "`{id}` declares an unsafe asset path `{module}`"
        ))));
    }
    let path = apps.registry.app_dir(&app).join(relative);
    let etag = format!("\"{sha256}\"");
    let cache = "immutable";
    // The manifest hash is the validator: a conditional hit does not read or
    // hash the file (§6.4).
    if if_none_match(&headers, sha256) {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [(ETAG, etag), (CACHE_CONTROL, cache.to_string())],
        )
            .into_response());
    }
    let bytes = std::fs::read(&path).map_err(|_| Error::internal("the asset could not be read"))?;
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if hash != sha256 {
        return Err(ApiError(Error::internal(format!(
            "`{module}` does not match its manifest hash"
        ))));
    }
    let content_type = match relative.extension().and_then(|value| value.to_str()) {
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    let body = if method == Method::HEAD {
        Vec::new()
    } else {
        bytes
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(ETAG, etag)
        .header(CACHE_CONTROL, cache)
        .header(CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(body))
        .map_err(|error| ApiError(Error::internal(format!("building the response: {error}"))))
}

fn if_none_match(headers: &HeaderMap, hash: &str) -> bool {
    header_tokens(headers, IF_NONE_MATCH).any(|candidate| {
        candidate == "*" || candidate == hash || candidate.trim_matches('"') == hash
    })
}

#[derive(Debug, Deserialize)]
struct KeyBody {
    public_key: String,
    #[serde(default)]
    label: Option<String>,
}

/// The risk `approve` would freeze for this app over the installed graph.
async fn preview_approval(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let _installed = installed_app(apps, &id)?;
    let installed_apps = installed_apps(apps)?;
    let risk = app_risk(&id, &installed_apps).map_err(|error| ApiError(error.into_error()))?;
    let required = Grant::required_levels(risk).collect::<Vec<_>>();
    Ok(Json(json!({
        "app": id,
        "risk": risk,
        "required": required,
    })))
}

/// The installed app's WIT package, for `anth app fetch-wit`.
async fn app_wit(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let installed = installed_app(apps, &id)?;
    let files = apps.registry.wit_files(&id)?;
    Ok(Json(json!({
        "app": id,
        "package": installed.manifest.app.namespace,
        "files": files,
    })))
}

/// The registry's installed entry, with a corrupt tree reported as an internal
/// error rather than a bad request.
fn installed_app(apps: &Apps, id: &AppId) -> Result<Installed, ApiError> {
    apps.registry
        .get(id)
        .map_err(|error| {
            if error.kind() == ErrorKind::Invalid {
                ApiError(Error::internal(format!(
                    "the installed tree for `{id}` is corrupt: {}",
                    error.message()
                )))
            } else {
                ApiError(error)
            }
        })?
        .ok_or_else(|| ApiError(Error::not_found(format!("`{id}` is not installed"))))
}

/// One app: descriptor, effective alias and its frozen grant.
async fn get_app(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
) -> Result<Json<AppView>, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let installed = installed_app(apps, &id)?;
    Ok(Json(app_view(apps, &installed, true)?))
}

/// Installs a bundle (raw `application/zip`). The id lives inside the bundle.
async fn install_app(
    State(state): State<Arc<EngineState>>,
    Extension(actor): Extension<Actor>,
    request: Request,
) -> Result<Response, ApiError> {
    let apps = apps(&state)?;
    let actor = actor.0;
    let body = read_body(request, apps.max_bundle_bytes).await?;
    let installed = match apps.registry.install(&body, &apps.options()) {
        Ok(installed) => installed,
        Err(error) => {
            return Err(audit_or_error(
                audit_failure(apps, &actor, AuditAction::AppInstall, None, &error),
                error,
            ));
        }
    };
    let id = AppId::parse(&installed.manifest.app.id)?;
    audit(
        apps,
        &actor,
        AuditAction::AppInstall,
        Some(id.as_str()),
        Outcome::Ok,
        Some(json!({
            "version": installed.manifest.app.version,
            "signer": installed.descriptor.signer,
        })),
    )?;
    let view = app_view(apps, &installed, true)?;
    Ok((
        StatusCode::CREATED,
        [(LOCATION, format!("/api/engine/apps/{id}"))],
        Json(view),
    )
        .into_response())
}

/// Replaces the bundle of an installed app. The grant is revoked: re-approval is
/// required.
async fn replace_app(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
    request: Request,
) -> Result<Json<AppView>, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let actor = actor.0;
    let body = read_body(request, apps.max_bundle_bytes).await?;
    // Revoke before the swap and never restore: a failed or partial replace
    // leaves the app installed but unapproved. Resurrection would be worse than
    // a re-approval the operator can give in one command.
    apps.grants.remove(&id)?;
    let installed = match apps.registry.replace(&id, &body, &apps.options()) {
        Ok(installed) => installed,
        Err(error) => {
            return Err(audit_or_error(
                audit_failure(
                    apps,
                    &actor,
                    AuditAction::AppUpgrade,
                    Some(id.as_str()),
                    &error,
                ),
                error,
            ));
        }
    };
    audit(
        apps,
        &actor,
        AuditAction::AppUpgrade,
        Some(id.as_str()),
        Outcome::Ok,
        Some(json!({"version": installed.manifest.app.version, "grant": "revoked"})),
    )?;
    Ok(Json(app_view(apps, &installed, true)?))
}

/// Uninstalls the app and its grant: the authority goes with the content.
async fn remove_app(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let actor = actor.0;
    require_ack(&state, apps, &actor, &id, "app.remove", &headers)?;
    // Revoke authority before touching the content: a corrupt or half-removed
    // tree must never keep a grant. A missing app simply has no grant to remove.
    apps.grants.remove(&id)?;
    match apps.registry.remove(&id) {
        Ok(true) => {}
        Ok(false) => {
            return Err(ApiError(Error::not_found(format!(
                "`{id}` is not installed"
            ))));
        }
        Err(error) => {
            return Err(audit_or_error(
                audit_failure(
                    apps,
                    &actor,
                    AuditAction::AppRemove,
                    Some(id.as_str()),
                    &error,
                ),
                error,
            ));
        }
    }
    audit(
        apps,
        &actor,
        AuditAction::AppRemove,
        Some(id.as_str()),
        Outcome::Ok,
        None,
    )?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct PatchBody {
    #[serde(default)]
    enabled: Option<bool>,
    /// `Some(Some(alias))` sets, `Some(None)` clears, absent leaves it alone.
    #[serde(default, deserialize_with = "present")]
    alias: Option<Option<String>>,
}

fn present<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

/// Enables/disables the app or changes its CLI alias. Authority is untouched.
async fn patch_app(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
    Json(body): Json<PatchBody>,
) -> Result<Json<AppView>, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let actor = actor.0;
    if body.enabled.is_none() && body.alias.is_none() {
        return Err(ApiError(Error::invalid(
            "nothing to change; send `enabled` or `alias`",
        )));
    }
    if body.enabled == Some(false) {
        require_ack(&state, apps, &actor, &id, "app.disable", &headers)?;
    }
    let installed = apps.registry.configure(&id, body.enabled, body.alias)?;
    if let Some(enabled) = body.enabled {
        let action = if enabled {
            AuditAction::AppEnable
        } else {
            AuditAction::AppDisable
        };
        audit(
            apps,
            &actor,
            action,
            Some(id.as_str()),
            Outcome::Ok,
            Some(json!({"enabled": enabled})),
        )?;
    }
    Ok(Json(app_view(apps, &installed, true)?))
}

/// The frozen grant, or `404` when the app has none.
async fn get_grant(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
) -> Result<Json<GrantView>, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    match apps.grants.load(&id)? {
        Some(grant) => Ok(Json(GrantView::from(grant))),
        None => Err(ApiError(Error::not_found(format!("`{id}` has no grant")))),
    }
}

#[derive(Debug, Deserialize)]
struct GrantBody {
    /// The `--accept-*` levels the operator presented.
    #[serde(default)]
    accept: Vec<Risk>,
}

/// Approves the app: computes its risk over the installed graph, freezes the
/// grant and audits it.
async fn approve_app(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
    Json(body): Json<GrantBody>,
) -> Result<Response, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let actor = actor.0;
    let installed = apps
        .registry
        .get(&id)?
        .ok_or_else(|| ApiError(Error::not_found(format!("`{id}` is not installed"))))?;
    let material = apps
        .registry
        .approval_material(&id)?
        .ok_or_else(|| ApiError(Error::not_found(format!("`{id}` is not installed"))))?;
    let existed = apps.grants.contains(&id)?;
    let installed_apps = installed_apps(apps)?;
    let grant = approve(
        &id,
        &installed_apps,
        Approval {
            accepted: body.accept,
            approved_by: actor.clone(),
            approved_at: timestamp(),
            manifest_sha256: material.manifest_sha256,
            wit_sha256: material.wit_sha256,
            signature: material.signature,
        },
    )
    .map_err(|error| {
        let error = error.into_error();
        audit_or_error(
            audit_failure(
                apps,
                &actor,
                AuditAction::AppApprove,
                Some(id.as_str()),
                &error,
            ),
            error,
        )
    })?;
    apps.grants.save(&grant)?;
    audit(
        apps,
        &actor,
        AuditAction::AppApprove,
        Some(id.as_str()),
        Outcome::Ok,
        Some(json!({"risk": grant.risk, "accepted": grant.accepted})),
    )?;
    let view = app_view(apps, &installed, true)?;
    let status = if existed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        [(LOCATION, format!("/api/engine/apps/{id}/grant"))],
        Json(view),
    )
        .into_response())
}

/// Revokes the grant. Apps that import what this app exports need
/// `X-Anth-Acknowledge: dependents`.
async fn revoke_grant(
    State(state): State<Arc<EngineState>>,
    Path(id): Path<String>,
    Extension(actor): Extension<Actor>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let apps = apps(&state)?;
    let id = AppId::parse(&id)?;
    let actor = actor.0;
    if !apps.grants.contains(&id)? {
        return Err(ApiError(Error::not_found(format!("`{id}` has no grant"))));
    }
    require_ack(&state, apps, &actor, &id, "app.revoke", &headers)?;
    if !apps.grants.remove(&id)? {
        return Err(ApiError(Error::not_found(format!("`{id}` has no grant"))));
    }
    audit(
        apps,
        &actor,
        AuditAction::AppRevoke,
        Some(id.as_str()),
        Outcome::Ok,
        None,
    )?;
    Ok(StatusCode::NO_CONTENT)
}

/// The installed set in the shape `approve` expects.
fn installed_apps(apps: &Apps) -> Result<Vec<InstalledApp>, Error> {
    // A corrupt tree or an unreadable grant elsewhere must not block approving
    // this app: the catalogue skips what it cannot read and an invalid grant is
    // treated as "not approved".
    Ok(apps.registry.installed_apps(&apps.grants)?.1)
}

const ACKNOWLEDGE: &str = "x-anth-acknowledge";

fn acknowledged(headers: &HeaderMap, token: &str) -> bool {
    header_tokens(headers, ACKNOWLEDGE).any(|item| item == token)
}

/// Refuses a breaking action when other composed apps depend on this one,
/// unless the operator acknowledged the token. The acknowledgment is audited.
fn require_ack(
    state: &EngineState,
    apps: &Apps,
    actor: &str,
    id: &AppId,
    action: &str,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let composition = current_composition(state)?;
    let installed = installed_apps(apps).map_err(ApiError)?;
    let dependents = composed_dependents(&composition, &installed, id)?;
    if dependents.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = dependents.iter().map(ToString::to_string).collect();
    if !acknowledged(headers, "dependents") {
        return Err(ApiError(
            Error::conflict(format!("`{id}` is required by {}", names.join(", "))).with_detail(
                json!({
                    "dependents": names,
                    "acknowledge": ["dependents"],
                }),
            ),
        ));
    }
    audit(
        apps,
        actor,
        AuditAction::Acknowledge,
        Some(id.as_str()),
        Outcome::Ok,
        Some(json!({"dependents": names, "action": action})),
    )?;
    Ok(())
}

/// The audit actor is inserted by the auth middleware.
fn audit(
    apps: &Apps,
    actor: &str,
    action: AuditAction,
    app: Option<&str>,
    result: Outcome,
    detail: Option<Value>,
) -> Result<(), Error> {
    apps.audit.append(&AuditEntry {
        at: timestamp(),
        actor: actor.to_string(),
        action,
        app: app.map(str::to_string),
        result,
        detail,
    })
}

/// A failed action is still an entry; a denial must never be dropped.
fn audit_failure(
    apps: &Apps,
    actor: &str,
    action: AuditAction,
    app: Option<&str>,
    error: &Error,
) -> Result<(), Error> {
    let result = if error.kind() == ErrorKind::Denied {
        Outcome::Denied
    } else {
        Outcome::Failed
    };
    audit(
        apps,
        actor,
        action,
        app,
        result,
        Some(json!({"error": error.message()})),
    )
}

/// The original failure wins, but an audit that could not be written is not
/// silent: both are reported.
fn audit_or_error(audit: Result<(), Error>, original: Error) -> ApiError {
    match audit {
        Ok(()) => ApiError(original),
        Err(audit_error) => ApiError(Error::internal(format!(
            "{}; the audit entry could not be written: {}",
            original.message(),
            audit_error.message()
        ))),
    }
}

/// The auth middleware records denials: an unauthenticated request is an entry.
pub(crate) fn deny(state: &EngineState, actor: String, reason: &str) {
    if let Some(apps) = state.apps.as_deref() {
        let _ = apps.audit.append(&AuditEntry {
            at: timestamp(),
            actor,
            action: AuditAction::Deny,
            app: None,
            result: Outcome::Denied,
            detail: Some(json!({"reason": reason})),
        });
    }
}

async fn read_body(request: Request, limit: u64) -> Result<Bytes, ApiError> {
    use http_body::Body as _;
    // A body that announces more than the cap is refused before reading it;
    // a chunked body hits `to_bytes`'s limit, which is the only size error.
    if request
        .body()
        .size_hint()
        .upper()
        .is_some_and(|size| size > limit)
    {
        return Err(ApiError(Error::too_large(format!(
            "the body is larger than {limit} bytes"
        ))));
    }
    axum::body::to_bytes(request.into_body(), limit as usize)
        .await
        .map_err(|error| {
            if is_length_limit(&error) {
                ApiError(Error::too_large(format!(
                    "the body is larger than {limit} bytes"
                )))
            } else {
                ApiError(Error::invalid("the request body could not be read"))
            }
        })
}

fn is_length_limit(error: &axum::Error) -> bool {
    use std::error::Error as StdError;
    let mut source: Option<&(dyn StdError + 'static)> = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<http_body_util::LengthLimitError>()
            .is_some()
        {
            return true;
        }
        source = error.source();
    }
    false
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
            let material = apps.registry.approval_material(&id).ok().flatten();
            let current = installed.grant_matches(&grant)
                && material
                    .as_ref()
                    .is_some_and(|material| material.matches(&grant));
            let state = if current {
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

/// The manifest-to-surface bridge for a UI slot. Kept as a function so the
/// grouping fields have a direct test.
fn surface_slot(slot: &SlotSection) -> SurfaceSlot {
    SurfaceSlot {
        id: slot.id.clone(),
        label: slot.label.clone(),
        icon: slot.icon.clone(),
        icon_sha256: slot.icon_sha256.clone(),
        order: slot.order,
        hidden: slot.hidden,
        parent: slot.parent.clone(),
        tab: slot.tab.clone(),
        view: slot.view.clone(),
        actions: slot
            .actions
            .iter()
            .map(|action| SurfaceFabAction {
                label: action.label.clone(),
                icon: action.icon.clone(),
                icon_sha256: action.icon_sha256.clone(),
                tag: action.tag.clone(),
                target: action.target.clone(),
                action: action.action.clone(),
                files: action.files,
            })
            .collect(),
        tag: slot.tag.clone(),
        module: slot.module.clone(),
        sha256: slot.sha256.clone(),
    }
}

mod error;
mod guard;
mod session;

pub use error::ApiError;
pub(crate) use guard::{
    password_change_gate, require_origin, require_peer, require_session, security_headers,
};
#[cfg(test)]
pub(crate) use session::cookie_secure;
pub(crate) use session::{
    change_operator_password, login_session, logout_session, read_operator, read_session,
    SESSION_BODY_LIMIT,
};
pub use session::{session_token, SESSION_COOKIE};

#[cfg(test)]
mod tests;
