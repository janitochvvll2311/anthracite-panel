//! Native services: launch, readiness and WIT-RPC (§19 §4).
//!
//! A service is a provider transport, not a second API. It answers
//! `interface#function` over a private `POST /__anth/call` on a Unix socket the
//! supervisor hands over; routes, CLI and generated clients still speak WIT and
//! do not know whether the provider is a component or a process.
//!
//! The supervisor owns the process group:
//!
//! - `state/run/` is `0711` and each `run/<app-id>/` is `0700`, owned by its
//!   service uid; the socket is `run/<app-id>/<pid>.sock` at `0600`, so the
//!   directory is the first access control;
//! - the child is started with `setsid`, cleared environment and no stdio, and
//!   stopped with `SIGTERM`, five seconds, then `SIGKILL` on the group;
//! - a crash after a healthy start is retried three times in sixty seconds;
//!   after that the app is degraded and calls answer `unavailable`.

use std::collections::BTreeMap;
use std::future::Future;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anth_compose::audit::{timestamp, AuditLog};
use anth_contract::{
    AppId, AuditAction, AuditEntry, Error, ErrorEnvelope, ErrorKind, ExportTarget, Listener,
    Outcome, Root, ServiceClass,
};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex, Notify};
use tokio::task::JoinHandle;

use crate::supervisor::SupervisorCaller;

/// The service-side call cap. Larger transfers are streams, which are M5.
pub const CALL_CAP: usize = 4 * 1024 * 1024;
/// Enough for the status line and headers, never counted as payload.
const HEADER_CAP: usize = 8 * 1024;
const STOP_GRACE: Duration = Duration::from_secs(5);
const MAX_RESTARTS: u32 = 3;

/// L0 limits. `RLIMIT_NPROC` counts threads, not just processes, and the kernel
/// enforces it per real user id: every service shares the engine user until a
/// later layer gives each its own uid (design §28 §9).
const NOFILE_LIMIT: libc::rlim_t = 4096;
const NPROC_LIMIT_EXEC: libc::rlim_t = 4096;
const NPROC_LIMIT_SERVICE: libc::rlim_t = 1024;

/// Everything boot needs to start one service. The manifest has already been
/// validated and the binary hashed.
#[derive(Debug, Clone)]
pub struct ServiceLaunch {
    pub app: String,
    pub binary: PathBuf,
    pub class: ServiceClass,
    pub argv: Vec<String>,
    pub socket: PathBuf,
    /// The per-app data directory substituted for `{data}`.
    pub data_dir: PathBuf,
    /// The v4 roots the operator approved; empty means the implicit set only.
    pub roots: Vec<Root>,
    /// The v4 listeners the operator approved.
    pub listen: Vec<Listener>,
    pub exports: BTreeMap<String, ExportTarget>,
    pub start_timeout: Duration,
}

/// What the supervisor knows about one service while it lives.
struct Service {
    app: AppId,
    binary: PathBuf,
    class: ServiceClass,
    argv: Vec<String>,
    socket: PathBuf,
    data_dir: PathBuf,
    roots: Vec<Root>,
    listen: Vec<Listener>,
    uid: u32,
    gid: u32,
    exports: BTreeMap<String, ExportTarget>,
    start_timeout: Duration,
    audit: AuditLog,
    running: AtomicBool,
    /// The process group id, which is the child's pid because it called
    /// `setsid`. Zero while no child is alive.
    pid: AtomicI32,
    stop: AtomicBool,
    wake: Notify,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Service {
    fn record(&self, action: AuditAction, result: Outcome, detail: Option<Value>) {
        let entry = AuditEntry {
            at: timestamp(),
            actor: "system".to_string(),
            action,
            app: Some(self.app.to_string()),
            result,
            detail,
        };
        // The audit is a record, not a precondition: a full disk must not turn a
        // started service into a failed boot.
        let _ = self.audit.append(&entry);
    }
}

/// Owns every service process and its socket.
pub struct ServiceSupervisor {
    run_dir: PathBuf,
    audit: AuditLog,
    call_timeout: Duration,
    services: Mutex<BTreeMap<AppId, Arc<Service>>>,
}

impl ServiceSupervisor {
    pub fn new(state: &Path, audit: AuditLog, call_timeout: Duration) -> Self {
        Self {
            run_dir: state.join("run"),
            audit,
            call_timeout,
            services: Mutex::new(BTreeMap::new()),
        }
    }

    /// The common constructor: the audit log lives in the same state tree.
    pub fn for_state(state: &Path, call_timeout: Duration) -> Self {
        Self::new(state, AuditLog::new(state), call_timeout)
    }

    /// The socket a service will listen on. The manifest's `{socket}` is
    /// replaced with exactly this path. The engine pid is part of the name so a
    /// fast restart cannot have the old engine unlink the new engine's sockets
    /// while its services are shutting down.
    pub fn socket_path(state: &Path, app: &AppId) -> PathBuf {
        Self::run_dir(state, app).join(format!("{}.sock", std::process::id()))
    }

    /// The per-service socket directory. It belongs to the service uid so the
    /// child can bind its socket; the parent `run/` stays engine-owned.
    pub fn run_dir(state: &Path, app: &AppId) -> PathBuf {
        state.join("run").join(app.as_str())
    }

    /// Launches the service and waits for `GET /health`. A slow service is an
    /// error for this boot, never a hang.
    pub async fn start(&self, launch: ServiceLaunch) -> Result<(), Error> {
        self.ensure_run_dir()?;
        let app = AppId::parse(&launch.app)?;
        self.stop_app(&app).await;
        let uid = if crate::sandbox::is_root() {
            match crate::sandbox::allocate_uid(app.as_str()) {
                Ok(uid) => uid,
                Err(error) => {
                    let reason = format!("uid: allocating an id for {app}: {error}");
                    self.denied(&app, launch.class, &reason);
                    return Err(Error::denied(reason));
                }
            }
        } else {
            // Without root the uid layer is unavailable; Landlock still applies
            // and the service keeps the engine user. The release runs the
            // engine as root or with the capability (D19).
            // SAFETY: `geteuid` has no preconditions.
            unsafe { libc::geteuid() }
        };
        let gid = crate::sandbox::engine_gid();
        let socket_dir = launch
            .socket
            .parent()
            .ok_or_else(|| Error::internal("the socket has no parent directory"))?
            .to_path_buf();
        self.prepare(&app, &launch, &socket_dir, uid, gid)?;
        let binary = match crate::sandbox::stage_binary(&launch.binary, &socket_dir, uid, gid) {
            Ok(binary) => binary,
            Err(error) => {
                let reason = format!(
                    "uid: staging {} for uid {uid}: {error}",
                    launch.binary.display()
                );
                self.denied(&app, launch.class, &reason);
                return Err(Error::denied(reason));
            }
        };
        match std::fs::symlink_metadata(&launch.socket) {
            Ok(metadata) if metadata.file_type().is_socket() => {
                std::fs::remove_file(&launch.socket).map_err(|error| {
                    Error::internal(format!(
                        "removing the stale socket {}: {error}",
                        launch.socket.display()
                    ))
                })?;
            }
            Ok(_) => {
                return Err(Error::invalid(format!(
                    "{} exists and is not a socket",
                    launch.socket.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::internal(format!(
                    "inspecting {}: {error}",
                    launch.socket.display()
                )));
            }
        }

        let service = Arc::new(Service {
            app: app.clone(),
            binary,
            class: launch.class,
            argv: launch.argv,
            socket: launch.socket,
            data_dir: launch.data_dir,
            roots: launch.roots,
            listen: launch.listen,
            uid,
            gid,
            exports: launch.exports,
            start_timeout: launch.start_timeout,
            audit: self.audit.clone(),
            running: AtomicBool::new(false),
            pid: AtomicI32::new(0),
            stop: AtomicBool::new(false),
            wake: Notify::new(),
            task: Mutex::new(None),
        });
        let (ready, answered) = oneshot::channel();
        let driver = Arc::clone(&service);
        let handle = tokio::spawn(async move { drive(driver, ready).await });
        *service.task.lock().await = Some(handle);

        match answered.await {
            Ok(Ok(())) => {
                self.services.lock().await.insert(app, service);
                Ok(())
            }
            Ok(Err(error)) => {
                service.running.store(false, Ordering::Release);
                Err(error)
            }
            Err(_) => Err(Error::internal(
                "the service task ended before reporting readiness",
            )),
        }
    }

    /// The half that implements `interface` for `app`, when the app runs a
    /// service. `None` sends the call to the wasm supervisor.
    pub async fn target(&self, app: &str, interface: &str) -> Option<ExportTarget> {
        let id = AppId::parse(app).ok()?;
        self.services
            .lock()
            .await
            .get(&id)
            .and_then(|service| service.exports.get(interface).copied())
    }

    /// The running service that exports `interface`.
    async fn service(&self, app: &str, interface: &str) -> Result<Arc<Service>, Error> {
        let id = AppId::parse(app)?;
        let service = self
            .services
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::unavailable(format!("`{app}` does not run a service")))?;
        if !service.running.load(Ordering::Acquire) {
            return Err(Error::unavailable(format!(
                "the service `{app}` is not running"
            )));
        }
        if !service.exports.contains_key(interface) {
            return Err(Error::not_found(format!(
                "the service `{app}` does not export `{interface}`"
            )));
        }
        Ok(service)
    }

    /// Calls one function and returns the unwrapped `ok` value.
    pub async fn call(
        &self,
        app: &str,
        interface: &str,
        function: &str,
        input: Value,
    ) -> Result<Value, Error> {
        let service = self.service(app, interface).await?;
        let body = serde_json::to_vec(&serde_json::json!({
            "interface": interface,
            "function": function,
            "input": input,
        }))
        .map_err(|error| Error::internal(format!("encoding the service call: {error}")))?;
        if body.len() > CALL_CAP {
            return Err(Error::too_large(format!(
                "the call is {} bytes; the limit is {CALL_CAP}",
                body.len()
            )));
        }
        let answer = tokio::time::timeout(
            self.call_timeout,
            http(&service.socket, "POST", "/__anth/call", &body),
        )
        .await
        .map_err(|_| {
            Error::exhausted(format!(
                "the service `{app}` did not answer within {}ms",
                self.call_timeout.as_millis()
            ))
        })??;
        if answer.0 == 200 {
            let value: Value = serde_json::from_slice(&answer.1).map_err(|error| {
                Error::internal(format!(
                    "the service `{app}` answered invalid JSON: {error}"
                ))
            })?;
            value.get("ok").cloned().ok_or_else(|| {
                Error::internal(format!(
                    "the service `{app}` answered without an `ok` value"
                ))
            })
        } else {
            Err(service_error(app, answer.0, &answer.1))
        }
    }

    /// Starts a streamed function and returns the body as the engine sees it
    /// (§20). The connection is held open for as long as the caller reads.
    pub async fn stream(
        &self,
        app: &str,
        interface: &str,
        function: &str,
        input: Value,
    ) -> Result<anth_api::ChunkStream, Error> {
        let service = self.service(app, interface).await?;
        let body = serde_json::to_vec(&serde_json::json!({
            "interface": interface,
            "function": function,
            "input": input,
        }))
        .map_err(|error| Error::internal(format!("encoding the service call: {error}")))?;
        if body.len() > CALL_CAP {
            return Err(Error::too_large(format!(
                "the call is {} bytes; the limit is {CALL_CAP}",
                body.len()
            )));
        }
        let opened = tokio::time::timeout(self.call_timeout, open_stream(&service.socket, &body))
            .await
            .map_err(|_| {
                Error::exhausted(format!(
                    "the service `{app}` did not start a stream within {}ms",
                    self.call_timeout.as_millis()
                ))
            })??;
        match opened {
            Opened::Body(reader) => Ok(Box::pin(futures_util::stream::try_unfold(
                reader,
                |mut reader| async move {
                    match next_chunk(&mut reader).await? {
                        Some(chunk) => Ok(Some((chunk, reader))),
                        None => Ok(None),
                    }
                },
            ))),
            Opened::Error(status, payload) => Err(service_error(app, status, &payload)),
        }
    }

    /// Removes sockets left by a previous engine process. A live service is
    /// always restarted by this boot, so a socket with no owner is stale.
    ///
    /// Sockets live one level down, in `run/<app-id>/`, because the per-app
    /// directory belongs to its service uid; a socket at the top of `run/`
    /// comes from a tree older than that layout.
    pub fn clear_stale(&self) {
        let Ok(entries) = std::fs::read_dir(&self.run_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let Ok(files) = std::fs::read_dir(&path) else {
                    continue;
                };
                for file in files.flatten() {
                    if file.file_name().to_string_lossy().ends_with(".sock") {
                        let _ = std::fs::remove_file(file.path());
                    }
                }
            } else if entry.file_name().to_string_lossy().ends_with(".sock") {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Stops one service. Missing is not an error.
    pub async fn stop(&self, app: &str) {
        if let Ok(id) = AppId::parse(app) {
            self.stop_app(&id).await;
        }
    }

    /// Stops every service, for an orderly shutdown.
    pub async fn stop_all(&self) {
        let ids: Vec<AppId> = self.services.lock().await.keys().cloned().collect();
        for id in ids {
            self.stop_app(&id).await;
        }
    }

    async fn stop_app(&self, app: &AppId) {
        let service = self.services.lock().await.remove(app);
        let Some(service) = service else {
            return;
        };
        service.stop.store(true, Ordering::Release);
        service.wake.notify_waiters();
        let pid = service.pid.load(Ordering::Acquire);
        kill_group(pid, libc::SIGTERM);
        let mut task = service.task.lock().await.take();
        if let Some(task) = &mut task {
            if tokio::time::timeout(STOP_GRACE, &mut *task).await.is_err() {
                let pid = service.pid.load(Ordering::Acquire);
                kill_group(pid, libc::SIGKILL);
                let _ = tokio::time::timeout(Duration::from_secs(1), &mut *task).await;
            }
        }
        let _ = std::fs::remove_file(&service.socket);
        service.running.store(false, Ordering::Release);
    }

    /// The L1 preconditions and the directories the service owns. A failure is
    /// a `service.deny` with the layer, not a crash: the launch is not retried.
    fn prepare(
        &self,
        app: &AppId,
        launch: &ServiceLaunch,
        socket_dir: &Path,
        uid: u32,
        gid: u32,
    ) -> Result<(), Error> {
        let denied = |reason: String| {
            self.denied(app, launch.class, &reason);
            Error::denied(reason)
        };
        if crate::sandbox::abi().is_none() {
            return Err(denied(
                "landlock: the kernel has no Landlock (5.13+); refusing to run unconfined"
                    .to_string(),
            ));
        }
        if launch
            .listen
            .iter()
            .any(|listener| listener.is_privileged())
            && !crate::sandbox::has_net_bind_service()
        {
            return Err(denied(
                "capability: a declared listener is below 1024 and the engine has no \
                 CAP_NET_BIND_SERVICE"
                    .to_string(),
            ));
        }
        let state = launch
            .data_dir
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| Error::internal("the data directory has no state root"))?;
        if crate::sandbox::is_root() {
            // The service uid must be able to traverse the engine's root and
            // `run/` to reach the directory it owns; neither is listable.
            crate::sandbox::make_traversable(state).map_err(|error| {
                denied(format!(
                    "uid: making {} traversable: {error}",
                    state.display()
                ))
            })?;
            crate::sandbox::make_traversable(&self.run_dir).map_err(|error| {
                denied(format!(
                    "uid: making {} traversable: {error}",
                    self.run_dir.display()
                ))
            })?;
            // `data/` is engine-owned; each app directory below it is 0700 and
            // owned by its service, so the parent only needs traversal.
            let data_root = state.join("data");
            std::fs::create_dir_all(&data_root).map_err(|error| {
                denied(format!("uid: creating {}: {error}", data_root.display()))
            })?;
            crate::sandbox::make_traversable(&data_root).map_err(|error| {
                denied(format!(
                    "uid: making {} traversable: {error}",
                    data_root.display()
                ))
            })?;
        }
        for path in [socket_dir, launch.data_dir.as_path()] {
            crate::sandbox::prepare_owned(path, uid, gid).map_err(|error| {
                denied(format!(
                    "uid: preparing {} for uid {uid}: {error}",
                    path.display()
                ))
            })?;
        }
        crate::sandbox::prepare_shared(&state.join("shared"), gid).map_err(|error| {
            denied(format!(
                "uid: preparing {}: {error}",
                state.join("shared").display()
            ))
        })?;
        // The broker's listener belongs to the engine (0c); the service only
        // needs the path in its argv, not write access to the directory.
        std::fs::create_dir_all(state.join("broker")).map_err(|error| {
            denied(format!(
                "uid: preparing {}: {error}",
                state.join("broker").display()
            ))
        })?;
        Ok(())
    }

    fn denied(&self, app: &AppId, class: ServiceClass, reason: &str) {
        let layer = deny_layer(reason);
        let entry = AuditEntry {
            at: timestamp(),
            actor: "system".to_string(),
            action: AuditAction::ServiceDeny,
            app: Some(app.to_string()),
            result: Outcome::Denied,
            detail: Some(serde_json::json!({
                "reason": reason,
                "class": class.as_str(),
                "layer": layer,
            })),
        };
        let _ = self.audit.append(&entry);
    }

    fn ensure_run_dir(&self) -> Result<(), Error> {
        match std::fs::symlink_metadata(&self.run_dir) {
            Ok(metadata) if metadata.is_dir() => {
                std::fs::set_permissions(&self.run_dir, std::fs::Permissions::from_mode(0o711))
                    .map_err(|error| {
                        Error::internal(format!(
                            "setting {} to 0711: {error}",
                            self.run_dir.display()
                        ))
                    })?;
                Ok(())
            }
            Ok(_) => Err(Error::invalid(format!(
                "{} is not a directory",
                self.run_dir.display()
            ))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&self.run_dir).map_err(|error| {
                    Error::internal(format!("creating {}: {error}", self.run_dir.display()))
                })?;
                std::fs::set_permissions(&self.run_dir, std::fs::Permissions::from_mode(0o711))
                    .map_err(|error| {
                        Error::internal(format!(
                            "setting {} to 0711: {error}",
                            self.run_dir.display()
                        ))
                    })
            }
            Err(error) => Err(Error::internal(format!(
                "inspecting {}: {error}",
                self.run_dir.display()
            ))),
        }
    }
}

/// One service's process loop: spawn, health, wait, restart with backoff.
async fn drive(service: Arc<Service>, ready: oneshot::Sender<Result<(), Error>>) {
    let mut ready = Some(ready);
    let mut restarts = 0u32;
    let mut window: Option<tokio::time::Instant> = None;
    loop {
        if service.stop.load(Ordering::Acquire) {
            service.running.store(false, Ordering::Release);
            let _ = std::fs::remove_file(&service.socket);
            return;
        }
        let mut child = match spawn(&service) {
            Ok(child) => child,
            Err(error) => {
                service.running.store(false, Ordering::Release);
                // A launch the supervisor refuses is a denial, not a crash: it is
                // recorded once with its class and never retried behind the
                // operator's back, because nothing changed since the refusal.
                service.record(
                    AuditAction::ServiceDeny,
                    Outcome::Denied,
                    Some(serde_json::json!({
                        "reason": error.message(),
                        "class": service.class.as_str(),
                        "layer": deny_layer(error.message()),
                    })),
                );
                let _ = std::fs::remove_file(&service.socket);
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Err(error));
                }
                return;
            }
        };
        let pid = child.id().map_or(0, |pid| pid as i32);
        service.pid.store(pid, Ordering::Release);

        let health = wait_healthy(&service.socket, service.start_timeout).await;
        match health {
            Ok(()) => {
                if let Ok(metadata) = std::fs::symlink_metadata(&service.socket) {
                    if metadata.file_type().is_socket() {
                        let _ = std::fs::set_permissions(
                            &service.socket,
                            std::fs::Permissions::from_mode(0o600),
                        );
                    }
                }
                service.running.store(true, Ordering::Release);
                let missing: Vec<String> = service
                    .roots
                    .iter()
                    .filter(|root| std::fs::metadata(&root.path).is_err())
                    .map(|root| root.path.clone())
                    .collect();
                let mut detail = serde_json::json!({"class": service.class.as_str()});
                if !missing.is_empty() {
                    // The rule was skipped so the service could start; the
                    // operator still gets a trail for the mistyped/missing path.
                    detail["missing-roots"] = serde_json::json!(missing);
                }
                service.record(AuditAction::ServiceStart, Outcome::Ok, Some(detail));
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Ok(()));
                }
                let status = child.wait().await;
                service.pid.store(0, Ordering::Release);
                service.running.store(false, Ordering::Release);
                // The socket file outlives its process on Unix; remove it before
                // the backoff so a probe cannot reach a dead listener.
                let _ = std::fs::remove_file(&service.socket);
                if service.stop.load(Ordering::Acquire) {
                    service.record(AuditAction::ServiceStop, Outcome::Ok, None);
                    let _ = std::fs::remove_file(&service.socket);
                    return;
                }
                service.record(
                    AuditAction::ServiceCrash,
                    Outcome::Failed,
                    Some(serde_json::json!({"status": format!("{status:?}")})),
                );
                if over_budget(&mut restarts, &mut window) {
                    service.record(
                        AuditAction::ServiceDeny,
                        Outcome::Denied,
                        Some(serde_json::json!({"reason": "restart budget exhausted"})),
                    );
                    let _ = std::fs::remove_file(&service.socket);
                    return;
                }
                let backoff = Duration::from_secs(1u64 << (restarts - 1));
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = service.wake.notified() => {}
                }
            }
            Err(error) => {
                kill_group(pid, libc::SIGKILL);
                let _ = child.wait().await;
                service.pid.store(0, Ordering::Release);
                service.running.store(false, Ordering::Release);
                let _ = std::fs::remove_file(&service.socket);
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Err(error));
                    return;
                }
                service.record(
                    AuditAction::ServiceCrash,
                    Outcome::Failed,
                    Some(serde_json::json!({"reason": error.message()})),
                );
                if over_budget(&mut restarts, &mut window) {
                    service.record(
                        AuditAction::ServiceDeny,
                        Outcome::Denied,
                        Some(serde_json::json!({"reason": "restart budget exhausted"})),
                    );
                    return;
                }
                let backoff = Duration::from_secs(1u64 << (restarts - 1));
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = service.wake.notified() => {}
                }
            }
        }
    }
}

/// The Landlock view: the implicit directories the contract guarantees plus
/// every root the operator approved. Roots that cover the engine state are
/// allowed only as far as the per-service uid already narrows them.
fn landlock_rules(service: &Service) -> Result<Vec<crate::sandbox::Rule>, Error> {
    let abi = crate::sandbox::abi()
        .ok_or_else(|| Error::denied("landlock: the kernel has no Landlock (5.13+)"))?;
    let state = service
        .data_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::internal("the data directory has no state root"))?;
    let socket_dir = service
        .socket
        .parent()
        .ok_or_else(|| Error::internal("the socket has no parent directory"))?;
    let mut rules = crate::sandbox::implicit_rules(
        abi,
        &service.data_dir,
        socket_dir,
        &state.join("broker"),
        &state.join("shared"),
        &service.binary,
    );
    for root in &service.roots {
        let path = PathBuf::from(&root.path);
        // A declared runtime socket that is not installed must not stop the
        // service: the path stays denied and the app reports the missing
        // runtime. A directory that the operator named and removed also stays
        // denied; only the rule is skipped.
        if std::fs::metadata(&path).is_err() {
            continue;
        }
        rules.push(crate::sandbox::Rule {
            path,
            access: if root.read_only {
                crate::sandbox::READ
            } else {
                crate::sandbox::read_write(abi)
            },
        });
    }
    Ok(rules)
}

fn spawn(service: &Service) -> Result<Child, Error> {
    let rules = landlock_rules(service)?;
    let ruleset =
        crate::sandbox::build(&rules).map_err(|error| Error::denied(error.to_string()))?;
    let mut command = Command::new(&service.binary);
    command
        .args(&service.argv)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let class = service.class;
    let uid = service.uid;
    let gid = service.gid;
    let keep_bind = service
        .listen
        .iter()
        .any(|listener| listener.is_privileged());
    let root = crate::sandbox::is_root();
    // SAFETY: every call is async-signal-safe, the closure allocates nothing,
    // and it runs between fork and exec, which is the only place it can run.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(prefixed("setsid", std::io::Error::last_os_error()));
            }
            // The prctl knobs are Linux-only; elsewhere the child keeps the
            // process-group and limits below and the gap is documented.
            #[cfg(target_os = "linux")]
            {
                // No setuid/setgid/file capabilities through a later exec.
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == -1 {
                    return Err(prefixed("prctl", std::io::Error::last_os_error()));
                }
                // Die with the engine; do not leave a service behind.
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) == -1 {
                    return Err(prefixed("prctl", std::io::Error::last_os_error()));
                }
                // No core dump, and no ptrace of this service by an unrelated
                // process. It also makes `/proc/<pid>` root-owned, so a service
                // must not rely on reading another process's proc files.
                if libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) == -1 {
                    return Err(prefixed("prctl", std::io::Error::last_os_error()));
                }
            }
            libc::umask(0o077);
            set_limit(libc::RLIMIT_CORE, 0).map_err(|error| prefixed("limits", error))?;
            set_limit(libc::RLIMIT_NOFILE, NOFILE_LIMIT)
                .map_err(|error| prefixed("limits", error))?;
            let nproc = match class {
                ServiceClass::Exec | ServiceClass::Host => NPROC_LIMIT_EXEC,
                ServiceClass::Socket | ServiceClass::Device | ServiceClass::Admin => {
                    NPROC_LIMIT_SERVICE
                }
            };
            set_limit(libc::RLIMIT_NPROC, nproc).map_err(|error| prefixed("limits", error))?;
            // L1: the capability bounding set first (needs the engine's
            // CAP_SETPCAP), then the uid, then the filesystem view. The
            // ruleset was built before the fork so a root the service cannot
            // open itself (the bundle directory) still got a rule.
            if root {
                crate::sandbox::drop_bounding_set(keep_bind)
                    .map_err(|error| prefixed("capability", error))?;
            }
            crate::sandbox::become_service(uid, gid, keep_bind)
                .map_err(|error| prefixed("uid", error))?;
            let restricted = crate::sandbox::restrict_with(ruleset);
            libc::close(ruleset);
            restricted.map_err(|error| prefixed("landlock", error))?;
            Ok(())
        });
    }
    // A fork in a sibling thread can inherit the staged file while it is still
    // open for writing, and `exec` then answers ETXTBSY. The kernel treats it
    // as transient: retry before declaring the launch denied.
    let spawned = {
        let mut attempt = 0u32;
        loop {
            match command.spawn() {
                Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20 * u64::from(attempt)));
                }
                other => break other,
            }
        }
    };
    // SAFETY: the child has its own copy of the descriptor; the parent must not
    // leak it across restarts.
    unsafe {
        libc::close(ruleset);
    }
    spawned.map_err(|error| {
        let error = prefixed("exec", error);
        // The child cannot report why the kernel refused it, so include the
        // facts an operator needs to tell mode, ownership and policy apart.
        let staged = std::fs::metadata(&service.binary)
            .map(|meta| {
                use std::os::unix::fs::MetadataExt;
                use std::os::unix::fs::PermissionsExt;
                format!(
                    "mode={:o} uid={} gid={}",
                    meta.permissions().mode(),
                    meta.uid(),
                    meta.gid()
                )
            })
            .unwrap_or_else(|error| format!("metadata: {error}"));
        // SAFETY: `geteuid` has no preconditions.
        let euid = unsafe { libc::geteuid() };
        Error::denied(format!(
            "sandbox: starting `{}` ({staged}, euid={euid}): {error}",
            service.binary.display()
        ))
    })
}

/// Adds the layer to an `io::Error` produced between fork and exec, so the
/// spawn-time audit entry can classify it.
fn prefixed(layer: &str, error: std::io::Error) -> std::io::Error {
    std::io::Error::new(error.kind(), format!("{layer}: {error}"))
}

/// The layer a denial came from, for the audit detail. Kept free so the
/// spawn-time refusal in `drive` classifies the same way as `prepare`.
fn deny_layer(reason: &str) -> &'static str {
    if reason.contains("landlock:") {
        "landlock"
    } else if reason.contains("capability:") {
        "capability"
    } else if reason.contains("uid:") {
        "uid"
    } else {
        "sandbox"
    }
}

/// The resource argument of `getrlimit`/`setrlimit`. glibc takes an unsigned
/// `__rlimit_resource_t`; musl and the other Unix targets take a `c_int`.
#[cfg(target_env = "gnu")]
type RlimitResource = libc::__rlimit_resource_t;
#[cfg(not(target_env = "gnu"))]
type RlimitResource = libc::c_int;

fn set_limit(resource: RlimitResource, desired: libc::rlim_t) -> std::io::Result<()> {
    let mut current: libc::rlimit = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrlimit(resource, &mut current) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    current.rlim_cur = if current.rlim_max == libc::RLIM_INFINITY {
        desired
    } else {
        desired.min(current.rlim_max)
    };
    if unsafe { libc::setrlimit(resource, &current) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Three restarts inside sixty seconds. A crash after a quiet window starts a
/// fresh budget instead of denying a service that fails once a day.
fn over_budget(restarts: &mut u32, window: &mut Option<tokio::time::Instant>) -> bool {
    let now = tokio::time::Instant::now();
    match window {
        Some(start) if now.duration_since(*start) <= Duration::from_secs(60) => {
            *restarts += 1;
        }
        _ => {
            *window = Some(now);
            *restarts = 1;
        }
    }
    *restarts > MAX_RESTARTS
}

fn kill_group(pid: i32, signal: i32) {
    if pid > 0 {
        // SAFETY: `kill` has no memory preconditions. The child is unreaped, so
        // its pid (and therefore the group) cannot have been recycled yet.
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}

async fn wait_healthy(socket: &Path, timeout: Duration) -> Result<(), Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(Error::unavailable(format!(
                "the service did not answer /health within {}ms",
                timeout.as_millis()
            )));
        }
        // One attempt must not outlive the whole budget: a service that accepts
        // and never answers is slow, not a reason to hang the boot.
        let attempt = (deadline - now).min(Duration::from_millis(250));
        match tokio::time::timeout(attempt, health(socket)).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(error)) if tokio::time::Instant::now() >= deadline => return Err(error),
            Ok(Err(_)) | Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
}

async fn health(socket: &Path) -> Result<(), Error> {
    let (status, _) = http(socket, "GET", "/health", b"").await?;
    if status == 200 {
        Ok(())
    } else {
        Err(Error::unavailable(format!(
            "the service answered {status} to /health"
        )))
    }
}

async fn http(
    socket: &Path,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<(u16, Vec<u8>), Error> {
    let mut stream = UnixStream::connect(socket).await.map_err(|error| {
        Error::unavailable(format!("connecting to {}: {error}", socket.display()))
    })?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: anthracite\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| Error::unavailable(format!("writing the service request: {error}")))?;
    stream
        .write_all(body)
        .await
        .map_err(|error| Error::unavailable(format!("writing the service request: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| Error::unavailable(format!("writing the service request: {error}")))?;

    let cap = CALL_CAP + HEADER_CAP;
    let mut response = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|error| Error::unavailable(format!("reading the service answer: {error}")))?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..read]);
        if response.len() > cap {
            return Err(Error::too_large(format!(
                "the service answered more than {CALL_CAP} bytes"
            )));
        }
    }

    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| Error::internal("the service answer has no HTTP header block"))?;
    let header = std::str::from_utf8(&response[..split])
        .map_err(|_| Error::internal("the service answer headers are not UTF-8"))?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| Error::internal("the service answer has no status line"))?;
    let mut payload = response[split + 4..].to_vec();
    if let Some(length) = header.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    }) {
        if payload.len() < length {
            return Err(Error::internal(format!(
                "the service promised {length} bytes and sent {}",
                payload.len()
            )));
        }
        payload.truncate(length);
    }
    Ok((status, payload))
}

/// The head of a service stream: either the chunked body or the JSON envelope of
/// an error that happened before the first byte.
enum Opened {
    Body(BufReader<UnixStream>),
    Error(u16, Vec<u8>),
}

/// Opens `POST /__anth/stream` and reads only its head, so a long-lived body does
/// not sit behind the call timeout.
async fn open_stream(socket: &Path, body: &[u8]) -> Result<Opened, Error> {
    let mut reader = BufReader::new(UnixStream::connect(socket).await.map_err(|error| {
        Error::unavailable(format!("connecting to {}: {error}", socket.display()))
    })?);
    let head = format!(
        "POST /__anth/stream HTTP/1.1\r\nHost: anthracite\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    {
        let stream = reader.get_mut();
        stream
            .write_all(head.as_bytes())
            .await
            .map_err(|error| Error::unavailable(format!("writing the stream request: {error}")))?;
        stream
            .write_all(body)
            .await
            .map_err(|error| Error::unavailable(format!("writing the stream request: {error}")))?;
        stream
            .flush()
            .await
            .map_err(|error| Error::unavailable(format!("writing the stream request: {error}")))?;
    }

    let mut status_line = String::new();
    if reader
        .read_line(&mut status_line)
        .await
        .map_err(|error| Error::unavailable(format!("reading the stream answer: {error}")))?
        == 0
    {
        return Err(Error::internal("the service stream has no status line"));
    }
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| Error::internal("the service stream has no status line"))?;

    let mut content_length = None;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|error| Error::unavailable(format!("reading the stream headers: {error}")))?;
        if read == 0 {
            return Err(Error::internal(
                "the service stream ended inside its headers",
            ));
        }
        if line == "\r\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().ok();
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.eq_ignore_ascii_case("chunked");
        }
    }

    if status == 200 {
        if !chunked {
            return Err(Error::internal(format!(
                "the service `{}` stream is not chunked",
                socket.display()
            )));
        }
        return Ok(Opened::Body(reader));
    }
    let length = content_length
        .ok_or_else(|| Error::internal("the service stream error has no `Content-Length`"))?;
    if length > CALL_CAP {
        return Err(Error::too_large(format!(
            "the service stream error is {length} bytes; the limit is {CALL_CAP}"
        )));
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| Error::unavailable(format!("reading the stream error: {error}")))?;
    Ok(Opened::Error(status, payload))
}

/// Reads one chunk of the service's chunked encoding. `None` is the clean
/// terminator; a connection that ends before it is an error, not EOF.
async fn next_chunk(reader: &mut BufReader<UnixStream>) -> Result<Option<Vec<u8>>, Error> {
    let mut line = String::new();
    let read = reader
        .read_line(&mut line)
        .await
        .map_err(|error| Error::unavailable(format!("reading a stream chunk: {error}")))?;
    if read == 0 {
        return Err(Error::unavailable(
            "the service stream ended before its terminator",
        ));
    }
    let size = usize::from_str_radix(line.trim(), 16)
        .map_err(|_| Error::internal("the service stream has an invalid chunk size"))?;
    if size == 0 {
        // Trailer fields, then the blank line that closes the body.
        loop {
            let mut trailer = String::new();
            let read = reader
                .read_line(&mut trailer)
                .await
                .map_err(|error| Error::unavailable(format!("reading stream trailers: {error}")))?;
            if read == 0 || trailer == "\r\n" {
                break;
            }
        }
        return Ok(None);
    }
    if size > CALL_CAP {
        return Err(Error::too_large(format!(
            "the service sent a {size}-byte chunk; the limit is {CALL_CAP}"
        )));
    }
    let mut chunk = vec![0u8; size];
    reader
        .read_exact(&mut chunk)
        .await
        .map_err(|_| Error::unavailable("the service stream ended in the middle of a chunk"))?;
    let mut terminator = [0u8; 2];
    reader
        .read_exact(&mut terminator)
        .await
        .map_err(|_| Error::unavailable("the service stream ended after a chunk"))?;
    if &terminator != b"\r\n" {
        return Err(Error::internal(
            "the service stream has no chunk terminator",
        ));
    }
    Ok(Some(chunk))
}

fn service_error(app: &str, status: u16, payload: &[u8]) -> Error {
    let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(payload) else {
        return Error::internal(format!(
            "the service `{app}` answered {status} without the error envelope"
        ));
    };
    let kind = ErrorKind::from_code(&envelope.error.code).unwrap_or(ErrorKind::Internal);
    let error = Error::new(kind, envelope.error.message);
    match envelope.error.detail {
        Some(detail) => error.with_detail(detail),
        None => error,
    }
}

/// The API's route dispatcher for a daemon that can run both kinds.
pub struct EngineCaller {
    wasm: SupervisorCaller,
    services: Arc<ServiceSupervisor>,
}

impl EngineCaller {
    pub fn new(wasm: SupervisorCaller, services: Arc<ServiceSupervisor>) -> Self {
        Self { wasm, services }
    }
}

impl anth_api::AppCaller for EngineCaller {
    fn call<'a>(
        &'a self,
        app: &'a str,
        interface: &'a str,
        function: &'a str,
        input: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, Error>> + Send + 'a>> {
        Box::pin(async move {
            match self.services.target(app, interface).await {
                Some(ExportTarget::Service) => {
                    self.services.call(app, interface, function, input).await
                }
                _ => self.wasm.call(app, interface, function, input).await,
            }
        })
    }

    fn stream<'a>(
        &'a self,
        app: &'a str,
        interface: &'a str,
        function: &'a str,
        input: Value,
    ) -> anth_api::StreamFuture<'a> {
        Box::pin(async move {
            match self.services.target(app, interface).await {
                Some(ExportTarget::Service) => {
                    self.services.stream(app, interface, function, input).await
                }
                _ => self.wasm.stream(app, interface, function, input).await,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{deny_layer, ServiceSupervisor};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-service-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn clear_stale_removes_the_sockets_below_the_run_directory() {
        let state = scratch("clear-stale");
        let app = state.join("run").join("org.example.notes");
        std::fs::create_dir_all(&app).expect("mkdir");
        std::fs::write(app.join("1234.sock"), b"").expect("write");
        std::fs::write(app.join("keep.txt"), b"").expect("write");
        let supervisor = ServiceSupervisor::for_state(&state, std::time::Duration::from_secs(1));
        supervisor.clear_stale();
        assert!(!app.join("1234.sock").exists(), "the stale socket goes");
        assert!(
            app.join("keep.txt").exists(),
            "the directory is not emptied"
        );
        assert!(app.is_dir(), "the app's socket directory stays");
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn a_prefixed_sandbox_error_keeps_its_layer() {
        assert_eq!(
            deny_layer("sandbox: starting x: landlock: /x: no"),
            "landlock"
        );
        assert_eq!(
            deny_layer("sandbox: starting x: capability: no bit"),
            "capability"
        );
        assert_eq!(deny_layer("sandbox: starting x: uid: EPERM"), "uid");
        assert_eq!(deny_layer("sandbox: starting x: ENOENT"), "sandbox");
    }
}
