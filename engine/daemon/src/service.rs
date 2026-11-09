//! Native services: launch, readiness and WIT-RPC (§19 §4).
//!
//! A service is a provider transport, not a second API. It answers
//! `interface#function` over a private `POST /__anth/call` on a Unix socket the
//! supervisor hands over; routes, CLI and generated clients still speak WIT and
//! do not know whether the provider is a component or a process.
//!
//! The supervisor owns the process group:
//!
//! - `state/run/` is `0700` and every socket lives there, so the directory is
//!   the first access control;
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
    AppId, AuditAction, AuditEntry, Error, ErrorEnvelope, ErrorKind, ExportTarget, Outcome,
};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

/// Everything boot needs to start one service. The manifest has already been
/// validated and the binary hashed.
#[derive(Debug, Clone)]
pub struct ServiceLaunch {
    pub app: String,
    pub binary: PathBuf,
    pub argv: Vec<String>,
    pub socket: PathBuf,
    pub exports: BTreeMap<String, ExportTarget>,
    pub start_timeout: Duration,
}

/// What the supervisor knows about one service while it lives.
struct Service {
    app: AppId,
    binary: PathBuf,
    argv: Vec<String>,
    socket: PathBuf,
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
    /// replaced with exactly this path.
    pub fn socket_path(state: &Path, app: &AppId) -> PathBuf {
        state.join("run").join(format!("{app}.sock"))
    }

    /// Launches the service and waits for `GET /health`. A slow service is an
    /// error for this boot, never a hang.
    pub async fn start(&self, launch: ServiceLaunch) -> Result<(), Error> {
        self.ensure_run_dir()?;
        let app = AppId::parse(&launch.app)?;
        self.stop_app(&app).await;
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
            binary: launch.binary,
            argv: launch.argv,
            socket: launch.socket,
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

    /// Calls one function and returns the unwrapped `ok` value.
    pub async fn call(
        &self,
        app: &str,
        interface: &str,
        function: &str,
        input: Value,
    ) -> Result<Value, Error> {
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

    /// Removes sockets left by a previous engine process. A live service is
    /// always restarted by this boot, so a socket with no owner is stale.
    pub fn clear_stale(&self) {
        let Ok(entries) = std::fs::read_dir(&self.run_dir) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().ends_with(".sock") {
                let _ = std::fs::remove_file(entry.path());
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

    fn ensure_run_dir(&self) -> Result<(), Error> {
        match std::fs::symlink_metadata(&self.run_dir) {
            Ok(metadata) if metadata.is_dir() => {
                std::fs::set_permissions(&self.run_dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(|error| {
                        Error::internal(format!(
                            "setting {} to 0700: {error}",
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
                std::fs::set_permissions(&self.run_dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(|error| {
                        Error::internal(format!(
                            "setting {} to 0700: {error}",
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
                service.record(AuditAction::ServiceStart, Outcome::Ok, None);
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

fn spawn(service: &Service) -> Result<Child, Error> {
    let mut command = Command::new(&service.binary);
    command
        .args(&service.argv)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // SAFETY: `setsid` is async-signal-safe and the closure allocates nothing.
    // It runs between fork and exec, which is the only place it can run.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().map_err(|error| {
        Error::unavailable(format!("starting `{}`: {error}", service.binary.display()))
    })
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
}
