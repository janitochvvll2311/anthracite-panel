//! The engine's HTTP surface.
//!
//! The router is a library value on purpose: the official panel mounts it in its
//! own process (§6.5), and a headless deployment serves it directly.
//!
//! Two routers: the full one, meant for the Unix socket where a peer credential
//! exists, and the public one, which is all a TCP listener serves until sessions
//! arrive. `/health` is public on both; everything under `/api` needs a peer.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use anth_compose::audit::{timestamp, AuditLog};
use anth_compose::bundle::Options as BundleOptions;
use anth_compose::composition::CompositionStore;
use anth_compose::grant::GrantStore;
use anth_compose::registry::{Installed, Registry};
use anth_compose::risk::{approve, Approval};
use anth_compose::{composed_dependents, resolve, DegradedReason, InstalledApp, Resolution};
use anth_contract::manifest::parse_call;
use anth_contract::{
    AppId, AppProblem, AppView, AuditAction, AuditEntry, Composition, CompositionView, DegradedApp,
    Error, ErrorKind, FatalView, GrantState, GrantView, InterfaceId, KeyAlgorithm, KeyView,
    Outcome, Risk, SurfaceApp, SurfaceCli, SurfaceLink, SurfaceRoute, SurfaceSlot, SurfaceView,
    TrustedKey, KEY_VERSION,
};
use axum::body::Bytes;
use axum::extract::{
    connect_info::Connected, ConnectInfo, Extension, Path, RawQuery, Request, State,
};
use axum::http::header::{
    ALLOW, CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_MATCH, IF_NONE_MATCH, LOCATION,
};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::UnixListener;

/// The installed apps and their grants, when this router serves them.
#[derive(Debug, Clone)]
pub struct Apps {
    pub registry: Registry,
    pub grants: GrantStore,
    pub audit: AuditLog,
    /// `apps.max-bundle-bytes` and `apps.require-signature`.
    pub max_bundle_bytes: u64,
    pub require_signature: bool,
    /// The composition file under the same state root.
    pub compositions: CompositionStore,
}

impl Apps {
    fn options(&self) -> BundleOptions {
        BundleOptions {
            max_bundle_bytes: self.max_bundle_bytes,
            require_signature: self.require_signature,
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
            ready: Arc::new(AtomicBool::new(true)),
            caller: None,
        }
    }

    pub fn with_caller(mut self, caller: Arc<dyn AppCaller>) -> Self {
        self.caller = Some(caller);
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
    ) -> Self {
        self.apps = Some(Arc::new(Apps {
            registry: Registry::new(state),
            grants: GrantStore::new(state),
            audit: AuditLog::new(state),
            max_bundle_bytes,
            require_signature,
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

type CallFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + 'a>>;

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

/// The `/api` surface, state applied.
///
/// A host that mounts the router in-process (the linked panel) nests this under
/// `/api` and marks its browser requests `Trusted`; the daemon wraps it with
/// liveness in [`router`].
pub fn api_router(state: Arc<EngineState>) -> Router {
    Router::new()
        .route("/health", get(readiness))
        .route("/engine/version", get(version))
        .route(
            "/engine/composition",
            get(composition).put(replace_composition),
        )
        .route("/engine/keys", get(list_keys).post(add_key))
        .route("/engine/keys/{fingerprint}", delete(remove_key))
        .route("/engine/apps", get(list_apps).post(install_app))
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
        .route_layer(middleware::from_fn_with_state(state.clone(), require_peer))
        .with_state(state)
}

/// The full surface, for the Unix socket.
pub fn router(state: Arc<EngineState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api", api_router(state))
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

/// `If-Match` is required and uses strong comparison: `*` matches any existing
/// document, a comma-separated list matches if any validator does, and repeated
/// header lines are the same list.
fn if_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(IF_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| {
            value
                .split(',')
                .map(str::trim)
                .any(|candidate| candidate == "*" || candidate == etag)
        })
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
        let slots = found
            .manifest
            .slot
            .iter()
            .map(|slot| SurfaceSlot {
                id: slot.id.clone(),
                label: slot.label.clone(),
                icon: slot.icon.clone(),
                order: slot.order,
                tag: slot.tag.clone(),
                module: slot.module.clone(),
                sha256: slot.sha256.clone(),
            })
            .collect();
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
    let caller = state
        .caller
        .as_ref()
        .ok_or_else(|| ApiError(Error::unavailable("this router does not run apps")))?;
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
    let slot = found
        .manifest
        .slot
        .iter()
        .find(|slot| slot.module == module)
        .ok_or_else(|| {
            ApiError(Error::not_found(format!(
                "`{id}` declares no asset `{module}`"
            )))
        })?;
    let relative = std::path::Path::new(&slot.module);
    if relative
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ApiError(Error::internal(format!(
            "`{id}` declares an unsafe asset path `{module}`"
        ))));
    }
    let path = apps.registry.app_dir(&app).join(relative);
    let etag = format!("\"{}\"", slot.sha256);
    let cache = "immutable";
    // The manifest hash is the validator: a conditional hit does not read or
    // hash the file (§6.4).
    if if_none_match(&headers, &slot.sha256) {
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
    if hash != slot.sha256 {
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
    headers
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').map(str::trim).any(|candidate| {
                candidate == "*" || candidate == hash || candidate.trim_matches('"') == hash
            })
        })
}

#[derive(Debug, Deserialize)]
struct KeyBody {
    public_key: String,
    #[serde(default)]
    label: Option<String>,
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
    headers
        .get(ACKNOWLEDGE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').map(str::trim).any(|item| item == token))
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
fn deny(state: &EngineState, actor: String, reason: &str) {
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

/// Every `/api` route needs a peer credential from the service user or root.
async fn require_peer(
    State(state): State<Arc<EngineState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if request.extensions().get::<Trusted>().is_some() {
        // A panel authenticates its browser requests and marks them; the
        // operator identity is the host's to add later.
        request.extensions_mut().insert(Actor("system".to_string()));
        return Ok(next.run(request).await);
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<PeerUid>>()
        .map(|ConnectInfo(uid)| uid.0);
    match peer {
        None => {
            deny(&state, "anonymous".to_string(), "no peer credential");
            Err(ApiError(Error::unauthenticated(
                "this surface needs a local peer credential",
            )))
        }
        Some(uid) if uid == 0 || uid == state.allowed_uid => {
            request.extensions_mut().insert(Actor(format!("uid:{uid}")));
            Ok(next.run(request).await)
        }
        Some(uid) => {
            deny(
                &state,
                format!("uid:{uid}"),
                "the peer is not the service user",
            );
            Err(ApiError(Error::unauthenticated(
                "the peer is not the service user or root",
            )))
        }
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
        apps_state_with(&[])
    }

    /// A state tree with `notes` installed and a composition naming `composed`.
    fn apps_state_with(composed: &[&str]) -> (Arc<EngineState>, std::path::PathBuf) {
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
        let wit = app.join("wit");
        let web = app.join("web");
        std::fs::create_dir_all(&wit).expect("mkdir");
        std::fs::create_dir_all(&web).expect("mkdir");
        for path in [&dir, &dir.join("apps"), &app, &wit, &web] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        }
        let manifest = "manifest_version = 2\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n\n[wasm]\npath = \"wasm/test.wasm\"\nsha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\n[[export]]\ninterface = \"example:notes/thing@1.0.0\"\n\n[[route]]\nmethod = \"GET\"\npath = \"/notes\"\ncall = \"example:notes/thing@1.0.0#ping\"\n\n[[slot]]\nid = \"panel.view\"\nlabel = \"Notes\"\norder = 1\ntag = \"notes\"\nmodule = \"web/notes.js\"\nsha256 = \"a87f13cb0f53003ce287621f67b588959fe569bcc8864527feb741b8bf5d6c71\"\n\n[[cli]]\ncall = \"example:notes/thing@1.0.0#ping\"\nverb = \"list\"\n";
        let descriptor = "version = 1\napp = \"org.example.notes\"\nenabled = true\ntier = \"third-party\"\ninstalled_at = \"2026-11-03T20:00:00Z\"\n";
        let wit_source =
            "package example:notes@1.0.0;\ninterface thing { ping: func(); }\nworld app { export thing; }\n";
        let ui_source =
            "export function mount(root) { root.textContent = \"notes\"; return () => {}; }\n";
        for (path, text) in [
            (app.join("manifest.toml"), manifest),
            (app.join("descriptor.toml"), descriptor),
            (wit.join("notes.wit"), wit_source),
            (web.join("notes.js"), ui_source),
        ] {
            std::fs::write(&path, text).expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        }
        (
            Arc::new(
                EngineState::new(1000)
                    .with_composition(Composition {
                        version: 1,
                        profile: vec![anth_contract::Profile {
                            id: "default".to_string(),
                            apps: composed.iter().map(|app| app.to_string()).collect(),
                        }],
                    })
                    .with_apps(&dir, 1 << 20, false),
            ),
            dir,
        )
    }

    /// The public half of the test signer: the verifying key for seed `[7; 32]`.
    fn public_key_hex() -> String {
        "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c".to_string()
    }

    fn write_grant(dir: &std::path::Path, text: &str) {
        write_grant_for(dir, "org.example.notes", text);
    }

    fn write_grant_for(dir: &std::path::Path, app: &str, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        let grants = dir.join("grants");
        std::fs::create_dir_all(&grants).expect("grants");
        std::fs::set_permissions(&grants, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let path = grants.join(format!("{app}.toml"));
        std::fs::write(&path, text).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    }

    fn grant_toml(dir: &std::path::Path) -> String {
        let app = AppId::parse("org.example.notes").expect("id");
        let material = Registry::new(dir)
            .approval_material(&app)
            .expect("material")
            .expect("installed");
        format!(
            "version = 1\napp = \"org.example.notes\"\napp_version = \"0.1.0\"\nmanifest_sha256 = \"{}\"\nwit_sha256 = \"{}\"\nrisk = \"read\"\napproved_by = \"operator-3f2a\"\napproved_at = \"2026-11-03T20:00:00Z\"\nexport = [\"example:notes/thing@1.0.0\"]\n",
            material.manifest_sha256, material.wit_sha256
        )
    }

    /// A second installed app that imports what `notes` exports.
    fn add_dependent(dir: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let app = dir.join("apps/org.example.consumer");
        let wit = app.join("wit");
        std::fs::create_dir_all(&wit).expect("mkdir");
        for path in [&app, &wit] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        }
        let manifest = "manifest_version = 2\n\n[app]\nid = \"org.example.consumer\"\nnamespace = \"example:consumer\"\nname = \"Consumer\"\nversion = \"0.1.0\"\n\n[[import]]\ninterface = \"example:notes/thing@1.0.0\"\n";
        let descriptor = "version = 1\napp = \"org.example.consumer\"\nenabled = true\ntier = \"third-party\"\ninstalled_at = \"2026-11-03T20:00:00Z\"\n";
        let wit_source = "package example:consumer@1.0.0;\ninterface thing { ping: func(); }\nworld app { import example:notes/thing@1.0.0; }\n";
        for (path, text) in [
            (app.join("manifest.toml"), manifest),
            (app.join("descriptor.toml"), descriptor),
            (wit.join("consumer.wit"), wit_source),
        ] {
            std::fs::write(&path, text).expect("write");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        }
    }

    /// A minimal valid bundle: one export, one world, one component.
    fn bundle(id: &str, namespace: &str, version: &str, wasm: &[u8]) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        let hash: String = Sha256::digest(wasm)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let manifest = format!(
            "manifest_version = 2\n\n[app]\nid = \"{id}\"\nnamespace = \"{namespace}\"\nname = \"Test\"\nversion = \"{version}\"\n\n[wasm]\npath = \"wasm/test.wasm\"\nsha256 = \"{hash}\"\n\n[[export]]\ninterface = \"{namespace}/thing@1.0.0\"\n"
        );
        let wit = format!(
            "package {namespace}@1.0.0;\ninterface thing {{ ping: func(); }}\nworld app {{ export thing; }}\n"
        );
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("manifest.toml", manifest.into_bytes()),
            ("wit/test.wit", wit.into_bytes()),
            ("wasm/test.wasm", wasm.to_vec()),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .expect("start");
            writer.write_all(&bytes).expect("write");
        }
        writer.finish().expect("finish").into_inner()
    }

    async fn call(
        state: Arc<EngineState>,
        method: &str,
        uri: &str,
        body: Vec<u8>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        call_with(state, method, uri, body, &[]).await
    }

    async fn call_with(
        state: Arc<EngineState>,
        method: &str,
        uri: &str,
        body: Vec<u8>,
        extra: &[(&str, &str)],
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        let mut request = builder.body(Body::from(body)).expect("request");
        request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
        let response = router(state).oneshot(request).await.expect("response");
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("body");
        let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, headers, value)
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
    async fn a_key_is_added_listed_and_removed() {
        let (state, dir) = apps_state();
        let body = format!(
            r#"{{"public_key":"{}","label":"example signer"}}"#,
            public_key_hex()
        );
        let (status, headers, value) = call(
            state.clone(),
            "POST",
            "/api/engine/keys",
            body.clone().into_bytes(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{value}");
        let fingerprint = value["fingerprint"]
            .as_str()
            .expect("fingerprint")
            .to_string();
        assert_eq!(fingerprint.len(), 64);
        assert_eq!(value["label"], "example signer");
        assert_eq!(value["added_by"], "uid:1000");
        assert_eq!(value["algorithm"], "ed25519");
        assert_eq!(
            headers
                .get("location")
                .and_then(|value| value.to_str().ok()),
            Some(format!("/api/engine/keys/{fingerprint}").as_str())
        );
        let (status, value) = json(state.clone(), "/api/engine/keys").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["keys"][0]["fingerprint"], fingerprint);
        // The same key is idempotent: it keeps the first label and adds no
        // second audit entry.
        let retry = format!(r#"{{"public_key":"{}"}}"#, public_key_hex());
        let (status, _, value) = call(
            state.clone(),
            "POST",
            "/api/engine/keys",
            retry.into_bytes(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["label"], "example signer");
        let (status, _, _) = call(
            state.clone(),
            "DELETE",
            &format!("/api/engine/keys/{fingerprint}"),
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, value) = json(state.clone(), "/api/engine/keys").await;
        assert_eq!(status, StatusCode::OK);
        assert!(value["keys"].as_array().expect("keys").is_empty());
        let (status, _, value) = call(
            state,
            "DELETE",
            &format!("/api/engine/keys/{fingerprint}"),
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"]["code"], "not-found");
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.len(), 2, "a retried add is not a second action");
        assert_eq!(entries[0].action, AuditAction::KeyAdd);
        assert_eq!(entries.last().unwrap().action, AuditAction::KeyRemove);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_key_that_is_not_32_bytes_is_refused() {
        let (state, dir) = apps_state();
        let (status, _, value) = call(
            state.clone(),
            "POST",
            "/api/engine/keys",
            br#"{"public_key":"nope"}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid");
        let (status, value) = json(state, "/api/engine/keys").await;
        assert_eq!(status, StatusCode::OK);
        assert!(value["keys"].as_array().expect("keys").is_empty());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_manifest_route_calls_the_app() {
        struct Echo;
        impl AppCaller for Echo {
            fn call<'a>(
                &'a self,
                app: &'a str,
                interface: &'a str,
                function: &'a str,
                input: Value,
            ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>>
            {
                Box::pin(async move {
                    Ok(json!({
                        "app": app,
                        "interface": interface,
                        "function": function,
                        "input": input,
                    }))
                })
            }
        }

        let (state, dir) = apps_state_with(&["org.example.notes"]);
        write_grant(&dir, &grant_toml(&dir));
        let state = Arc::new(
            Arc::try_unwrap(state)
                .ok()
                .expect("unique state")
                .with_caller(Arc::new(Echo)),
        );
        let (status, headers, value) = call(
            state.clone(),
            "GET",
            "/api/apps/org.example.notes/notes?q=a&q=b&single=1&list[]=only",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["app"], "org.example.notes");
        assert_eq!(value["interface"], "example:notes/thing@1.0.0");
        assert_eq!(value["function"], "ping");
        assert_eq!(value["input"]["q"], json!(["a", "b"]));
        assert_eq!(value["input"]["single"], "1");
        assert_eq!(value["input"]["list"], json!(["only"]));
        let _ = headers;
        let (status, headers, _) = call(
            state.clone(),
            "OPTIONS",
            "/api/apps/org.example.notes/notes",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let allow = headers
            .get("allow")
            .and_then(|value| value.to_str().ok())
            .expect("allow");
        assert!(allow.contains("GET"), "{allow}");
        assert!(allow.contains("OPTIONS"), "{allow}");
        let (status, _, _) = call(
            state.clone(),
            "POST",
            "/api/apps/org.example.notes/notes",
            b"{}".to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_route_for_a_degraded_app_is_unavailable() {
        struct Never;
        impl AppCaller for Never {
            fn call<'a>(
                &'a self,
                _app: &'a str,
                _interface: &'a str,
                _function: &'a str,
                _input: Value,
            ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>>
            {
                Box::pin(async move { Err(Error::internal("should not be called")) })
            }
        }
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        let state = Arc::new(
            Arc::try_unwrap(state)
                .ok()
                .expect("unique state")
                .with_caller(Arc::new(Never)),
        );
        let (status, _, value) = call(
            state.clone(),
            "GET",
            "/api/apps/org.example.notes/notes",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "unavailable");
        assert!(value["error"]["message"]
            .as_str()
            .expect("message")
            .contains("not running"));
        // OPTIONS belongs to the engine and does not need a running instance.
        let (status, headers, _) = call(
            state,
            "OPTIONS",
            "/api/apps/org.example.notes/notes",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(headers
            .get("allow")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|allow| allow.contains("GET")));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_route_that_is_not_declared_is_not_found() {
        struct Never;
        impl AppCaller for Never {
            fn call<'a>(
                &'a self,
                _app: &'a str,
                _interface: &'a str,
                _function: &'a str,
                _input: Value,
            ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>>
            {
                Box::pin(async move { Err(Error::internal("should not be called")) })
            }
        }
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        write_grant(&dir, &grant_toml(&dir));
        let state = Arc::new(
            Arc::try_unwrap(state)
                .ok()
                .expect("unique state")
                .with_caller(Arc::new(Never)),
        );
        let (status, _, value) =
            call(state, "GET", "/api/apps/org.example.notes/nope", Vec::new()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"]["code"], "not-found");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_undeclared_method_is_method_not_allowed() {
        struct Never;
        impl AppCaller for Never {
            fn call<'a>(
                &'a self,
                _app: &'a str,
                _interface: &'a str,
                _function: &'a str,
                _input: Value,
            ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>>
            {
                Box::pin(async move { Err(Error::internal("should not be called")) })
            }
        }
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        write_grant(&dir, &grant_toml(&dir));
        let state = Arc::new(
            Arc::try_unwrap(state)
                .ok()
                .expect("unique state")
                .with_caller(Arc::new(Never)),
        );
        let (status, headers, value) = call(
            state,
            "POST",
            "/api/apps/org.example.notes/notes",
            b"{}".to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(value["error"]["code"], "method-not-allowed");
        let allow = headers
            .get("allow")
            .and_then(|value| value.to_str().ok())
            .expect("allow");
        assert!(allow.contains("GET"), "{allow}");
        assert!(allow.contains("OPTIONS"), "{allow}");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_body_on_a_get_route_is_refused() {
        struct Never;
        impl AppCaller for Never {
            fn call<'a>(
                &'a self,
                _app: &'a str,
                _interface: &'a str,
                _function: &'a str,
                _input: Value,
            ) -> Pin<Box<dyn std::future::Future<Output = Result<Value, Error>> + Send + 'a>>
            {
                Box::pin(async move { Err(Error::internal("should not be called")) })
            }
        }
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        write_grant(&dir, &grant_toml(&dir));
        let state = Arc::new(
            Arc::try_unwrap(state)
                .ok()
                .expect("unique state")
                .with_caller(Arc::new(Never)),
        );
        let (status, _, value) = call(
            state,
            "GET",
            "/api/apps/org.example.notes/notes",
            br#"{}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_slot_module_is_hash_pinned_and_immutable() {
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        write_grant(&dir, &grant_toml(&dir));
        let (status, headers, value) = call(
            state.clone(),
            "GET",
            "/api/apps/org.example.notes/assets/web/notes.js",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value, Value::Null);
        assert!(headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/javascript")));
        assert_eq!(
            headers
                .get("cache-control")
                .and_then(|value| value.to_str().ok()),
            Some("immutable")
        );
        let etag = headers
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .expect("etag")
            .to_string();
        let (status, _, _) = call_with(
            state.clone(),
            "GET",
            "/api/apps/org.example.notes/assets/web/notes.js",
            Vec::new(),
            &[("if-none-match", &etag)],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        let (status, headers, _) = call(
            state.clone(),
            "HEAD",
            "/api/apps/org.example.notes/assets/web/notes.js",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.get("etag").is_some());
        let (status, _, _) = call(
            state,
            "GET",
            "/api/apps/org.example.notes/assets/web/nope.js",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_tampered_asset_is_refused_without_the_host_path() {
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        write_grant(&dir, &grant_toml(&dir));
        std::fs::write(dir.join("apps/org.example.notes/web/notes.js"), b"tampered")
            .expect("write");
        let (status, _, value) = call(
            state,
            "GET",
            "/api/apps/org.example.notes/assets/web/notes.js",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        let message = value["error"]["message"].as_str().expect("message");
        assert!(!message.contains(&dir.display().to_string()), "{message}");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_asset_of_a_degraded_app_is_unavailable() {
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        let (status, _, value) = call(
            state,
            "GET",
            "/api/apps/org.example.notes/assets/web/notes.js",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "unavailable");
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
        write_grant(&dir, &grant_toml(&dir));
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
        let text = grant_toml(&dir).replace("app_version = \"0.1.0\"", "app_version = \"9.9.9\"");
        write_grant(&dir, &text);
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
        write_grant(&dir, &grant_toml(&dir));
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
        let (state, dir) = apps_state();
        let response = router(state)
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
        assert!(value["surface"]["degraded"]
            .as_array()
            .expect("degraded")
            .is_empty());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_composed_unapproved_app_is_degraded() {
        let (state, dir) = apps_state_with(&["org.example.notes"]);
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
        assert!(value["surface"]["apps"]
            .as_array()
            .expect("apps")
            .is_empty());
        assert_eq!(value["surface"]["degraded"][0]["app"], "org.example.notes");
        assert_eq!(value["surface"]["degraded"][0]["reason"], "unapproved");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_approved_composed_app_is_in_the_boot_order() {
        let (state, dir) = apps_state_with(&["org.example.notes"]);
        write_grant(&dir, &grant_toml(&dir));
        let (status, value) = json(state, "/api/engine/composition").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["surface"]["apps"][0]["id"], "org.example.notes");
        assert_eq!(value["surface"]["apps"][0]["version"], "0.1.0");
        assert_eq!(value["surface"]["apps"][0]["risk"], "read");
        assert_eq!(value["surface"]["apps"][0]["routes"][0]["method"], "GET");
        assert_eq!(value["surface"]["apps"][0]["routes"][0]["path"], "/notes");
        assert_eq!(
            value["surface"]["apps"][0]["routes"][0]["interface"],
            "example:notes/thing@1.0.0"
        );
        assert_eq!(value["surface"]["apps"][0]["routes"][0]["function"], "ping");
        assert_eq!(value["surface"]["apps"][0]["slots"][0]["id"], "panel.view");
        assert_eq!(
            value["surface"]["apps"][0]["slots"][0]["module"],
            "web/notes.js"
        );
        assert_eq!(value["surface"]["apps"][0]["slots"][0]["order"], 1);
        assert_eq!(value["surface"]["apps"][0]["cli"][0]["verb"], "list");
        assert_eq!(value["surface"]["apps"][0]["cli"][0]["function"], "ping");
        assert!(value["surface"]["degraded"]
            .as_array()
            .expect("degraded")
            .is_empty());
        std::fs::remove_dir_all(dir).expect("cleanup");
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
    async fn a_booting_engine_is_not_ready() {
        let state = Arc::new(EngineState::new(1000).booting());
        let response = router(state.clone())
            .oneshot(request("/api/health", Some(1000)))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["status"], "starting");
        state.mark_ready();
        let response = router(state)
            .oneshot(request("/api/health", Some(1000)))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
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

    #[tokio::test]
    async fn an_install_is_audited() {
        let (state, dir) = apps_state();
        let bundle = bundle("org.example.other", "example:other", "0.1.0", b"component");
        let (status, headers, value) = call(state, "POST", "/api/engine/apps", bundle).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            headers.get(LOCATION).expect("location").to_str().unwrap(),
            "/api/engine/apps/org.example.other"
        );
        assert_eq!(value["id"], "org.example.other");
        assert_eq!(value["grant_state"], "none");
        assert!(dir.join("apps/org.example.other/manifest.toml").is_file());
        let entries = AuditLog::new(&dir).entries().expect("audit");
        let entry = entries.last().expect("entry");
        assert_eq!(entry.action, AuditAction::AppInstall);
        assert_eq!(entry.actor, "uid:1000");
        assert_eq!(entry.result, Outcome::Ok);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_failed_install_is_audited() {
        let (state, dir) = apps_state();
        let (status, _, value) =
            call(state, "POST", "/api/engine/apps", b"not a zip".to_vec()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid");
        let entries = AuditLog::new(&dir).entries().expect("audit");
        let entry = entries.last().expect("entry");
        assert_eq!(entry.action, AuditAction::AppInstall);
        assert_eq!(entry.result, Outcome::Failed);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn patch_disables_and_renames_the_app() {
        let (state, dir) = apps_state();
        let (status, _, value) = call(
            state.clone(),
            "PATCH",
            "/api/engine/apps/org.example.notes",
            br#"{"enabled":false,"alias":"custom"}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["enabled"], false);
        assert_eq!(value["alias"], "custom");
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.last().unwrap().action, AuditAction::AppDisable);

        let (status, _, value) = call(
            state,
            "PATCH",
            "/api/engine/apps/org.example.notes",
            b"{}".to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn remove_deletes_the_app_and_the_grant() {
        let (state, dir) = apps_state();
        write_grant(&dir, &grant_toml(&dir));
        let app = AppId::parse("org.example.notes").unwrap();
        let (status, _, _) = call(
            state,
            "DELETE",
            "/api/engine/apps/org.example.notes",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!dir.join("apps/org.example.notes").exists());
        assert_eq!(GrantStore::new(&dir).load(&app).expect("load"), None);
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.last().unwrap().action, AuditAction::AppRemove);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn replace_swaps_the_bundle_and_revokes_the_grant() {
        let (state, dir) = apps_state();
        write_grant(&dir, &grant_toml(&dir));
        let app = AppId::parse("org.example.notes").unwrap();
        let bundle = bundle(
            "org.example.notes",
            "example:notes",
            "0.2.0",
            b"new component",
        );
        let (status, _, value) =
            call(state, "PUT", "/api/engine/apps/org.example.notes", bundle).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["version"], "0.2.0");
        assert_eq!(value["grant_state"], "none");
        assert_eq!(GrantStore::new(&dir).load(&app).expect("load"), None);
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.last().unwrap().action, AuditAction::AppUpgrade);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn patch_can_clear_the_alias() {
        let (state, dir) = apps_state();
        let (status, _, value) = call(
            state.clone(),
            "PATCH",
            "/api/engine/apps/org.example.notes",
            br#"{"alias":"custom"}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["alias"], "custom");
        let (status, _, value) = call(
            state,
            "PATCH",
            "/api/engine/apps/org.example.notes",
            br#"{"alias":null}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["alias"], "notes", "the manifest fallback");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_unauthenticated_request_is_audited() {
        let (state, dir) = apps_state();
        let mut request = Request::builder()
            .uri("/api/engine/apps")
            .body(Body::empty())
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(PeerUid(1001)));
        let response = router(state).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let entries = AuditLog::new(&dir).entries().expect("audit");
        let entry = entries.last().expect("entry");
        assert_eq!(entry.action, AuditAction::Deny);
        assert_eq!(entry.actor, "uid:1001");
        assert_eq!(entry.result, Outcome::Denied);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_oversized_install_is_too_large() {
        let (_, dir) = apps_state();
        let state = Arc::new(EngineState::new(1000).with_apps(&dir, 10, false));
        let (status, _, value) = call(state, "POST", "/api/engine/apps", vec![0u8; 20]).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(value["error"]["code"], "too-large");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_failed_replace_does_not_resurrect_the_grant() {
        let (state, dir) = apps_state();
        write_grant(&dir, &grant_toml(&dir));
        let app = AppId::parse("org.example.notes").unwrap();
        let (status, _, _) = call(
            state,
            "PUT",
            "/api/engine/apps/org.example.notes",
            b"not a zip".to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            GrantStore::new(&dir).load(&app).expect("load"),
            None,
            "the grant is not restored"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_chunked_body_over_the_limit_is_too_large() {
        use axum::body::Bytes;
        use futures_util::stream;
        let (_, dir) = apps_state();
        let state = Arc::new(EngineState::new(1000).with_apps(&dir, 10, false));
        let body = axum::body::Body::from_stream(stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(b"0123456789")),
            Ok(Bytes::from_static(b"more")),
        ]));
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/engine/apps")
            .body(body)
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
        let response = router(state).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_corrupt_tree_can_still_be_uninstalled() {
        use std::os::unix::fs::PermissionsExt;
        let (state, dir) = apps_state();
        let broken = dir.join("apps/org.example.broken");
        std::fs::create_dir_all(&broken).expect("mkdir");
        std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let manifest = broken.join("manifest.toml");
        std::fs::write(&manifest, b"not a manifest").expect("write");
        std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let (status, _, _) = call(
            state,
            "DELETE",
            "/api/engine/apps/org.example.broken",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!broken.exists());
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.last().unwrap().action, AuditAction::AppRemove);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn approving_freezes_a_grant() {
        let (state, dir) = apps_state();
        let (status, _, value) = call(
            state,
            "PUT",
            "/api/engine/apps/org.example.notes/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(value["grant_state"], "current");
        assert_eq!(value["grant"]["risk"], "read");
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.last().unwrap().action, AuditAction::AppApprove);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn revoking_removes_the_grant() {
        let (state, dir) = apps_state();
        let (status, _, _) = call(
            state.clone(),
            "PUT",
            "/api/engine/apps/org.example.notes/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _, _) = call(
            state,
            "DELETE",
            "/api/engine/apps/org.example.notes/grant",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.last().unwrap().action, AuditAction::AppRevoke);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn revoking_a_needed_grant_needs_acknowledgment() {
        let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
        add_dependent(&dir);
        let (status, _, _) = call(
            state.clone(),
            "PUT",
            "/api/engine/apps/org.example.notes/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _, value) = call(
            state.clone(),
            "DELETE",
            "/api/engine/apps/org.example.notes/grant",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(value["error"]["code"], "conflict");
        assert_eq!(
            value["error"]["detail"]["dependents"][0],
            "org.example.consumer"
        );
        assert_eq!(value["error"]["detail"]["acknowledge"][0], "dependents");
        let (status, _, _) = call_with(
            state,
            "DELETE",
            "/api/engine/apps/org.example.notes/grant",
            Vec::new(),
            &[("x-anth-acknowledge", "dependents")],
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert!(
            entries
                .iter()
                .any(|entry| entry.action == AuditAction::Acknowledge),
            "{entries:?}"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn disabling_a_needed_app_needs_acknowledgment() {
        let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
        add_dependent(&dir);
        let (status, _, value) = call(
            state.clone(),
            "PATCH",
            "/api/engine/apps/org.example.notes",
            br#"{"enabled":false}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(value["error"]["detail"]["acknowledge"][0], "dependents");
        let (status, _, value) = call_with(
            state,
            "PATCH",
            "/api/engine/apps/org.example.notes",
            br#"{"enabled":false}"#.to_vec(),
            &[("x-anth-acknowledge", "dependents")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["enabled"], false);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_grant_with_another_wit_hash_is_stale() {
        let (state, dir) = apps_state();
        let app = AppId::parse("org.example.notes").unwrap();
        let material = Registry::new(&dir)
            .approval_material(&app)
            .expect("material")
            .expect("installed");
        let text = grant_toml(&dir).replace(&material.wit_sha256, &"0".repeat(64));
        write_grant(&dir, &text);
        let (status, value) = json(state, "/api/engine/apps/org.example.notes").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["grant_state"], "stale");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_blocked_consumer_carries_the_missing_link() {
        let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
        add_dependent(&dir);
        // The consumer is approved; the provider is not, so the consumer is
        // blocked rather than merely unapproved.
        let (status, _, _) = call(
            state.clone(),
            "PUT",
            "/api/engine/apps/org.example.consumer/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, value) = json(state, "/api/engine/composition").await;
        assert_eq!(status, StatusCode::OK);
        assert!(value["surface"]["apps"]
            .as_array()
            .expect("apps")
            .is_empty());
        let degraded = value["surface"]["degraded"].as_array().expect("degraded");
        let consumer = degraded
            .iter()
            .find(|app| app["app"] == "org.example.consumer")
            .expect("consumer");
        assert_eq!(consumer["reason"], "blocked");
        assert_eq!(consumer["detail"]["interface"], "example:notes/thing@1.0.0");
        assert_eq!(consumer["detail"]["provider"], "org.example.notes");
        assert!(value["surface"]["links"]
            .as_array()
            .expect("links")
            .is_empty());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_running_pair_exposes_its_link() {
        let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
        add_dependent(&dir);
        write_grant(&dir, &grant_toml(&dir));
        let (status, _, _) = call(
            state.clone(),
            "PUT",
            "/api/engine/apps/org.example.consumer/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, value) = json(state, "/api/engine/composition").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["surface"]["apps"][0]["id"], "org.example.notes");
        assert_eq!(value["surface"]["apps"][1]["id"], "org.example.consumer");
        assert_eq!(
            value["surface"]["links"][0]["consumer"],
            "org.example.consumer"
        );
        assert_eq!(
            value["surface"]["links"][0]["interface"],
            "example:notes/thing@1.0.0"
        );
        assert_eq!(
            value["surface"]["links"][0]["provider"],
            "org.example.notes"
        );
        assert!(value["surface"]["degraded"]
            .as_array()
            .expect("degraded")
            .is_empty());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn the_composition_is_replaced_with_its_validator() {
        let (state, dir) = apps_state();
        let (status, headers, _) =
            call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        let etag = headers
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .expect("etag")
            .to_string();
        assert!(etag.starts_with('"') && etag.ends_with('"'), "{etag}");
        let body = br#"{"version":1,"profile":[{"id":"default","apps":["org.example.notes"]}]}"#;
        let (status, headers, value) = call_with(
            state.clone(),
            "PUT",
            "/api/engine/composition",
            body.to_vec(),
            &[("if-match", &etag)],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            value["composition"]["profile"][0]["apps"][0],
            "org.example.notes"
        );
        assert_ne!(
            headers.get("etag").and_then(|value| value.to_str().ok()),
            Some(etag.as_str())
        );
        let (status, value) = json(state, "/api/engine/composition").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            value["composition"]["profile"][0]["apps"][0],
            "org.example.notes"
        );
        let entries = AuditLog::new(&dir).entries().expect("audit");
        assert_eq!(entries.last().unwrap().action, AuditAction::Compose);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_stale_or_missing_validator_is_refused() {
        let (state, dir) = apps_state();
        let body = br#"{"version":1,"profile":[{"id":"default","apps":[]}]}"#;
        let (status, _, value) = call(
            state.clone(),
            "PUT",
            "/api/engine/composition",
            body.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(value["error"]["code"], "precondition-failed");
        let (status, _, value) = call_with(
            state.clone(),
            "PUT",
            "/api/engine/composition",
            body.to_vec(),
            &[("if-match", "\"not-the-current-document\"")],
        )
        .await;
        assert_eq!(status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(value["error"]["code"], "precondition-failed");
        assert!(!dir.join("composition.toml").exists());
        // `*` matches any existing document.
        let (status, _, _) = call_with(
            state,
            "PUT",
            "/api/engine/composition",
            body.to_vec(),
            &[("if-match", "*")],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_invalid_composition_is_refused_and_not_written() {
        let (state, dir) = apps_state();
        let body = br#"{"version":1,"profile":[{"id":"default"},{"id":"other","apps":[]}]}"#;
        let (status, _, value) = call_with(
            state,
            "PUT",
            "/api/engine/composition",
            body.to_vec(),
            &[("if-match", "*")],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid");
        assert!(!dir.join("composition.toml").exists());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_composition_naming_an_uninstalled_app_is_refused() {
        let (state, dir) = apps_state();
        let body = br#"{"version":1,"profile":[{"id":"default","apps":["org.example.missing"]}]}"#;
        let (status, _, value) = call_with(
            state,
            "PUT",
            "/api/engine/composition",
            body.to_vec(),
            &[("if-match", "*")],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"]["code"], "not-found");
        assert!(!dir.join("composition.toml").exists());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_fatal_composition_is_still_readable_for_repair() {
        let (state, dir) = apps_state_with(&["org.example.missing"]);
        let (status, headers, value) =
            call(state, "GET", "/api/engine/composition", Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.get("etag").is_some());
        assert_eq!(
            value["composition"]["profile"][0]["apps"][0],
            "org.example.missing"
        );
        assert_eq!(value["surface"]["fatal"]["code"], "not-found");
        assert!(value["surface"]["apps"]
            .as_array()
            .expect("apps")
            .is_empty());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_fatal_composition_is_repaired_with_the_served_validator() {
        let (state, dir) = apps_state_with(&["org.example.missing"]);
        let (status, headers, value) =
            call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["surface"]["fatal"]["code"], "not-found");
        let etag = headers
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .expect("etag")
            .to_string();
        let repaired = br#"{"version":1,"profile":[{"id":"default","apps":[]}]}"#;
        let (status, _, _) = call_with(
            state.clone(),
            "PUT",
            "/api/engine/composition",
            repaired.to_vec(),
            &[("if-match", &etag)],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, value) = json(state, "/api/engine/composition").await;
        assert_eq!(status, StatusCode::OK);
        assert!(value["composition"]["profile"][0]["apps"]
            .as_array()
            .expect("apps")
            .is_empty());
        assert!(value["surface"].get("fatal").is_none());
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_composition_lock_covers_the_validator_check() {
        use std::time::Duration;
        let (state, dir) = apps_state();
        let (status, headers, _) =
            call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        let stale = headers
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .expect("etag")
            .to_string();
        let guard = state.compose_lock.lock().await;
        let body = br#"{"version":1,"profile":[{"id":"default","apps":[]}]}"#;
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            let mut request = Request::builder()
                .method("PUT")
                .uri("/api/engine/composition")
                .header("content-type", "application/json")
                .header("if-match", stale)
                .body(Body::from(body.to_vec()))
                .expect("request");
            request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
            router(task_state)
                .oneshot(request)
                .await
                .expect("response")
                .status()
        });
        // Let the replacement reach the lock, then change the served document
        // while it waits. A check made before the lock would still see the old
        // validator and win; a check inside the lock sees the new one and must
        // refuse.
        tokio::time::sleep(Duration::from_millis(100)).await;
        {
            let mut composition = state.composition.write().expect("composition lock");
            composition.profile[0].apps = vec!["org.example.notes".to_string()];
        }
        drop(guard);
        assert_eq!(task.await.expect("join"), StatusCode::PRECONDITION_FAILED);
        assert_eq!(
            state.composition.read().expect("composition").profile[0].apps,
            vec!["org.example.notes".to_string()]
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_composition_lock_covers_the_write() {
        use std::time::Duration;
        let (state, dir) = apps_state();
        let (status, headers, _) =
            call(state.clone(), "GET", "/api/engine/composition", Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        let etag = headers
            .get("etag")
            .and_then(|value| value.to_str().ok())
            .expect("etag")
            .to_string();
        let guard = state.compose_lock.lock().await;
        let body = br#"{"version":1,"profile":[{"id":"default","apps":["org.example.notes"]}]}"#;
        let task_state = state.clone();
        let task = tokio::spawn(async move {
            let mut request = Request::builder()
                .method("PUT")
                .uri("/api/engine/composition")
                .header("content-type", "application/json")
                .header("if-match", etag)
                .body(Body::from(body.to_vec()))
                .expect("request");
            request.extensions_mut().insert(ConnectInfo(PeerUid(1000)));
            router(task_state)
                .oneshot(request)
                .await
                .expect("response")
                .status()
        });
        // While the lock is held, neither the served document nor the file may
        // move: the write happens after the check, inside the same lock.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(state.composition.read().expect("composition").profile[0]
            .apps
            .is_empty());
        assert!(!dir.join("composition.toml").exists());
        drop(guard);
        assert_eq!(task.await.expect("join"), StatusCode::OK);
        assert_eq!(
            state.composition.read().expect("composition").profile[0].apps,
            vec!["org.example.notes".to_string()]
        );
        let text = std::fs::read_to_string(dir.join("composition.toml")).expect("composition");
        assert!(text.contains("org.example.notes"), "{text}");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn the_if_match_header_is_parsed_with_strong_comparison() {
        use axum::http::header::HeaderValue;
        let etag = "\"abc\"";
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(IF_MATCH, HeaderValue::from_str(value).expect("header"));
            headers
        };
        assert!(if_matches(&with("\"abc\""), etag));
        assert!(if_matches(&with("\"x\", \"abc\""), etag));
        assert!(if_matches(&with(" \"abc\" "), etag));
        assert!(if_matches(&with("*"), etag));
        assert!(!if_matches(&HeaderMap::new(), etag));
        assert!(!if_matches(&with("W/\"abc\""), etag));
        assert!(!if_matches(&with("\"ABC\""), etag));
        assert!(!if_matches(&with("\"nope\""), etag));
        let mut repeated = HeaderMap::new();
        repeated.append(IF_MATCH, HeaderValue::from_static("\"nope\""));
        repeated.append(IF_MATCH, HeaderValue::from_static("\"abc\""));
        assert!(if_matches(&repeated, etag));
    }

    #[tokio::test]
    async fn an_uncomposed_dependent_does_not_block_disabling() {
        let (state, dir) = apps_state();
        add_dependent(&dir);
        let (status, _, value) = call(
            state,
            "PATCH",
            "/api/engine/apps/org.example.notes",
            br#"{"enabled":false}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["enabled"], false);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_unreadable_other_grant_does_not_block_approval() {
        use std::os::unix::fs::PermissionsExt;
        let (state, dir) = apps_state();
        add_dependent(&dir);
        write_grant_for(&dir, "org.example.consumer", "not a grant");
        let broken = dir.join("apps/org.example.broken");
        std::fs::create_dir_all(&broken).expect("mkdir");
        std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        let manifest = broken.join("manifest.toml");
        std::fs::write(&manifest, b"not a manifest").expect("write");
        std::fs::set_permissions(&manifest, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let (status, _, value) = call(
            state,
            "PUT",
            "/api/engine/apps/org.example.notes/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(value["grant_state"], "current");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn revoking_without_a_grant_is_not_found_even_with_dependents() {
        let (state, dir) = apps_state_with(&["org.example.notes", "org.example.consumer"]);
        add_dependent(&dir);
        let (status, _, value) = call(
            state,
            "DELETE",
            "/api/engine/apps/org.example.notes/grant",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"]["code"], "not-found");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn re_approving_a_grant_is_ok() {
        let (state, dir) = apps_state();
        let (status, _, _) = call(
            state.clone(),
            "PUT",
            "/api/engine/apps/org.example.notes/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _, value) = call(
            state,
            "PUT",
            "/api/engine/apps/org.example.notes/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["grant_state"], "current");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn the_grant_has_its_own_get() {
        let (state, dir) = apps_state();
        let (status, _, _) = call(
            state.clone(),
            "PUT",
            "/api/engine/apps/org.example.notes/grant",
            br#"{"accept":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, value) = json(state.clone(), "/api/engine/apps/org.example.notes/grant").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["risk"], "read");
        let (status, _, _) = call(
            state.clone(),
            "DELETE",
            "/api/engine/apps/org.example.notes/grant",
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, value) = json(state, "/api/engine/apps/org.example.notes/grant").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(value["error"]["code"], "not-found");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
