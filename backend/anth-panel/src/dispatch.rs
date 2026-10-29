//! The one place that decides what a request does.
//!
//! The socket and HTTP transports only translate. Every command reaches
//! [`Daemon::execute`], so a transport cannot answer a request differently.

use std::sync::Arc;
use std::time::Duration;

use anth_host::backend::{
    Backends, FilesService, FirewallService, NetworkService, TerminalService,
};
use anth_wire::{ErrorKind, MAX_TERMINAL_INPUT, PageRequest, TerminalSize, WireError};

use crate::rpc::{Request, Response, TerminalOpened, TerminalOutput};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::app_host::AppService;
use crate::app_host::exec::{DemoExecRunner, ExecRunner, SystemExecRunner};
use crate::config::{Config, Paths};
use crate::firewall::SystemFirewall;
use crate::lifecycle::{ACTION_GRACE, Lifecycle};
use crate::log::{Level, Logger};
use crate::network::SystemNetwork;
use anth_host::accounts::SystemAccounts;
use anth_host::files::SystemFiles;
use anth_host::process::SystemProcesses;
use anth_host::terminal::{OpenError, SystemTerminal, resolve_shell, supports_line_editing};
use anth_host::volumes::SystemVolumes;

tokio::task_local! {
    /// The demo owner assigned by the HTTP session middleware.
    static DEMO_OWNER: String;
}

/// Runs one HTTP request under a demo owner.
pub fn with_demo_owner<F: std::future::Future>(
    owner: String,
    future: F,
) -> impl std::future::Future<Output = F::Output> {
    DEMO_OWNER.scope(owner, future)
}

/// How the host services are selected for this process.
enum Mode {
    /// One backend set for the whole daemon.
    Real(Backends),
    /// One backend set per demo session, created on demand.
    Demo(Arc<anth_host::demo::DemoSessions>),
}

/// Everything a request may need from the process.
pub struct Daemon {
    config: Config,
    paths: Paths,
    logger: Arc<Logger>,
    lifecycle: Arc<Lifecycle>,
    mode: Mode,
    apps: AppService,
}

impl Daemon {
    /// A daemon that may restart but never reboot.
    pub fn new(config: Config, paths: Paths, logger: Arc<Logger>) -> Self {
        Self::with_lifecycle(config, paths, logger, Arc::new(Lifecycle::without_reboot()))
    }

    pub fn with_lifecycle(
        config: Config,
        paths: Paths,
        logger: Arc<Logger>,
        lifecycle: Arc<Lifecycle>,
    ) -> Self {
        // A demo build cannot select the real backends, even if the process
        // environment still says `ANTH_BACKEND=real`.
        let demo = cfg!(feature = "demo")
            || std::env::var("ANTH_BACKEND")
                .map(|value| value.eq_ignore_ascii_case("demo"))
                .unwrap_or(false);
        let mode = if demo {
            let settings = anth_host::demo::DemoSettings {
                root: paths.state_dir.join("demo/sessions"),
                ttl_seconds: config.demo.session_ttl_seconds,
                max_sessions: config.demo.max_sessions,
                features: config.demo.features.clone(),
                files: anth_host::demo::DemoFileSettings {
                    max_entries: config.files.max_entries,
                    max_preview_bytes: config.files.max_preview_bytes,
                    max_upload_bytes: config.files.max_upload_bytes,
                    max_trash_bytes: config.files.max_trash_bytes,
                    max_archive_bytes: config.files.max_archive_bytes,
                },
                process: anth_host::demo::DemoProcessSettings {
                    control: config.process.control,
                    spawn: config.process.spawn,
                    spawn_allow: config.process.spawn_allow.clone(),
                },
                network_control: config.network.control,
                network_manage: config.network.manage,
                firewall_control: config.firewall.control,
                volumes: config.volumes.settings(),
                accounts: config.accounts.settings(),
                terminal_enabled: config.terminal.enabled,
                terminal_max_sessions: config.terminal.max_sessions,
            };
            Mode::Demo(Arc::new(anth_host::demo::DemoSessions::new(settings)))
        } else {
            let root = {
                let root = std::path::PathBuf::from(&config.files.root);
                if root.is_absolute() {
                    root
                } else {
                    paths.config_dir.join(root)
                }
            };
            let files_root = root.clone();
            let files = SystemFiles::new(
                root,
                config.files.write,
                config.files.max_entries,
                config.files.max_preview_bytes,
                config.files.max_upload_bytes,
            )
            .map(|service| {
                Arc::new(
                    service
                        .with_trash(config.files.trash, config.files.max_trash_bytes)
                        .with_archive_limit(config.files.max_archive_bytes),
                ) as Arc<dyn FilesService>
            });
            let mut protected = vec![
                files_root,
                paths.config.clone(),
                paths.socket.clone(),
                paths.state_dir.clone(),
                paths.secrets.clone(),
                paths.sessions.clone(),
                paths.apps.clone(),
                paths.grants.clone(),
                paths.app_state.clone(),
            ];
            if let Ok(executable) = std::env::current_exe() {
                protected.push(executable);
            }
            Mode::Real(Backends {
                files,
                terminal: Arc::new(SystemTerminal::new(
                    config.terminal.enabled,
                    config.terminal.max_sessions,
                    Duration::from_secs(config.terminal.idle_seconds),
                )),
                process: Arc::new(SystemProcesses::new(
                    config.process.control,
                    config.process.spawn,
                    config.process.spawn_allow.clone(),
                )),
                network: Arc::new(SystemNetwork::new(
                    config.network.control,
                    config.network.manage,
                )),
                firewall: Arc::new(SystemFirewall::new(config.firewall.control)),
                volumes: Arc::new(SystemVolumes::new(config.volumes.settings(), protected)),
                accounts: Arc::new(SystemAccounts::new(config.accounts.settings())),
            })
        };
        let exec: Arc<dyn ExecRunner> = if demo {
            Arc::new(DemoExecRunner::new())
        } else {
            Arc::new(SystemExecRunner::unavailable())
        };
        let apps = AppService::new(
            config.apps.clone(),
            paths.apps.clone(),
            paths.grants.clone(),
            exec,
        )
        .deny_socket(paths.socket.clone());
        Self {
            config,
            paths,
            logger,
            lifecycle,
            mode,
            apps,
        }
    }

    /// Replaces the network service. Tests use it to script `ip` without
    /// touching the host; the transport still goes through `execute`.
    pub fn with_network(mut self, network: SystemNetwork) -> Self {
        if let Mode::Real(backends) = &mut self.mode {
            backends.network = Arc::new(network);
        }
        self
    }

    /// Replaces the network backend with any implementation.
    pub fn with_network_backend(mut self, network: Arc<dyn NetworkService>) -> Self {
        if let Mode::Real(backends) = &mut self.mode {
            backends.network = network;
        }
        self
    }

    /// Replaces the firewall service. Tests use it to script `nft` without
    /// touching the host.
    pub fn with_firewall(mut self, firewall: SystemFirewall) -> Self {
        if let Mode::Real(backends) = &mut self.mode {
            backends.firewall = Arc::new(firewall);
        }
        self
    }

    /// Replaces the firewall backend with any implementation.
    pub fn with_firewall_backend(mut self, firewall: Arc<dyn FirewallService>) -> Self {
        if let Mode::Real(backends) = &mut self.mode {
            backends.firewall = firewall;
        }
        self
    }

    /// Replaces the volumes service. Tests use it to script the probe without
    /// touching the host.
    pub fn with_volumes(mut self, volumes: SystemVolumes) -> Self {
        if let Mode::Real(backends) = &mut self.mode {
            backends.volumes = Arc::new(volumes);
        }
        self
    }

    /// Replaces the accounts service. Tests use it to script `/etc` and the
    /// account tools without touching the host.
    pub fn with_accounts(mut self, accounts: SystemAccounts) -> Self {
        if let Mode::Real(backends) = &mut self.mode {
            backends.accounts = Arc::new(accounts);
        }
        self
    }

    /// Replaces the app service. Tests use it to script an app root
    /// without touching the installation directory.
    pub fn with_apps(mut self, extensions: AppService) -> Self {
        self.apps = extensions;
        self
    }

    pub fn apps(&self) -> &AppService {
        &self.apps
    }

    /// The real terminal, for tests that inspect it directly.
    pub fn terminals(&self) -> Option<&Arc<dyn TerminalService>> {
        match &self.mode {
            Mode::Real(backends) => Some(&backends.terminal),
            Mode::Demo(_) => None,
        }
    }

    pub fn close_terminals(&self, owner: &str) -> usize {
        match &self.mode {
            Mode::Real(backends) => backends.terminal.close_owner(owner),
            Mode::Demo(sessions) => sessions.close_terminals(owner),
        }
    }

    /// Reaps idle terminals and demo sessions forever. Called once by the
    /// serving command.
    pub fn spawn_terminal_reaper(self: &Arc<Self>) {
        let daemon = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                daemon.reap_idle();
            }
        });
    }

    fn reap_idle(&self) {
        match &self.mode {
            Mode::Real(backends) => {
                backends.terminal.reap();
            }
            Mode::Demo(sessions) => {
                sessions.reap();
            }
        }
    }

    fn backends(&self, owner: &str) -> Backends {
        match &self.mode {
            Mode::Real(backends) => backends.clone(),
            Mode::Demo(sessions) => sessions.backends(owner),
        }
    }

    fn demo(&self) -> Option<&Arc<anth_host::demo::DemoSessions>> {
        match &self.mode {
            Mode::Demo(sessions) => Some(sessions),
            Mode::Real(_) => None,
        }
    }

    pub fn demo_mode(&self) -> bool {
        self.demo().is_some()
    }

    pub fn demo_status(&self, owner: &str) -> Option<anth_host::demo::DemoStatus> {
        self.demo().map(|sessions| sessions.status(owner))
    }

    pub fn demo_reset(&self, owner: &str) -> Option<u64> {
        self.demo().and_then(|sessions| sessions.reset(owner))
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn logger(&self) -> &Arc<Logger> {
        &self.logger
    }

    pub fn lifecycle(&self) -> &Arc<Lifecycle> {
        &self.lifecycle
    }

    /// Reads an image for the HTTP preview route.
    ///
    /// Images are a binary resource rather than a wire command: the socket
    /// remains a JSON transport, and the HTTP adapter streams the bytes.
    pub fn image(&self, owner: &str, path: &str) -> Result<(Vec<u8>, &'static str), WireError> {
        let backends = self.backends(owner);
        files_of(&backends)?.image(path)
    }

    /// Reads a file for the HTTP download route.
    pub fn download(&self, owner: &str, path: &str) -> Result<Vec<u8>, WireError> {
        let backends = self.backends(owner);
        files_of(&backends)?.download(path)
    }

    /// Handles one request.
    pub async fn execute(&self, request: Request) -> Response {
        let owner = DEMO_OWNER
            .try_with(Clone::clone)
            .unwrap_or_else(|_| "daemon".to_string());
        self.execute_as(&owner, request).await
    }

    /// Handles one request for a named owner.
    ///
    /// The socket passes its peer identity; the browser bridge passes the
    /// authenticated session id, so one panel session cannot touch another's
    /// terminal handle.
    pub async fn execute_as(&self, owner: &str, request: Request) -> Response {
        let operation = operation_of(&request);
        let started = std::time::Instant::now();
        let response = match self.dispatch(owner, request).await {
            Ok(response) => response,
            Err(error) => Response::Error(error),
        };
        let failed = response.error().is_some();
        let ms = started.elapsed().as_millis().to_string();
        let level = if failed {
            Level::Warn
        } else if operation == "terminal-read" {
            Level::Debug
        } else {
            Level::Info
        };
        self.logger.event(
            level,
            "request",
            if failed { "failed" } else { "answered" },
            &[("op", operation), ("ms", &ms)],
        );
        response
    }

    async fn dispatch(&self, owner: &str, request: Request) -> Result<Response, WireError> {
        let backends = self.backends(owner);
        if let Some(demo) = self.demo()
            && let Some(name) = feature_of(&request)
            && !demo.features().enabled(name)
        {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "demo_feature_disabled",
                format!("the demo has `{name}` turned off"),
            ));
        }
        if matches!(
            &request,
            Request::TerminalOpen { .. }
                | Request::TerminalRead { .. }
                | Request::TerminalWrite { .. }
                | Request::TerminalResize { .. }
                | Request::TerminalClose { .. }
        ) && !backends.terminal.enabled()
        {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "terminal_disabled",
                "the terminal is disabled; set `[terminal] enabled = true`",
            ));
        }
        match request {
            Request::HostInfo => {
                let info = tokio::task::spawn_blocking(anth_host::collect)
                    .await
                    .map_err(|error| {
                        WireError::internal(format!("the host-info task failed: {error}"))
                    })?;
                Ok(Response::HostInfo(info))
            }
            Request::Network => {
                let network = backends.network.clone();
                Ok(Response::Network(
                    blocking(move || network.snapshot()).await?,
                ))
            }
            Request::NetworkSetState { name, up } => {
                let network = backends.network.clone();
                let message = blocking(move || network.set_state(&name, up)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkCreate(args) => {
                let network = backends.network.clone();
                let message = blocking(move || network.create(&args)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkEdit(args) => {
                let network = backends.network.clone();
                let message = blocking(move || network.edit(&args)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkRename { name, new_name } => {
                let network = backends.network.clone();
                let message = blocking(move || network.rename(&name, &new_name)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkDelete { name, peer } => {
                let network = backends.network.clone();
                let message = blocking(move || network.delete(&name, peer.as_deref())).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::FirewallStatus => {
                let firewall = backends.firewall.clone();
                Ok(Response::FirewallSnapshot(
                    blocking(move || firewall.snapshot()).await?,
                ))
            }
            Request::FirewallAdd {
                chain,
                action,
                protocol,
                port,
                comment,
            } => {
                let firewall = backends.firewall.clone();
                let message = blocking(move || {
                    firewall.add(chain, action, protocol, port, comment.as_deref())
                })
                .await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallUpdate {
                handle,
                chain,
                action,
                protocol,
                port,
                comment,
            } => {
                let firewall = backends.firewall.clone();
                let message = blocking(move || {
                    firewall.update(handle, chain, action, protocol, port, comment.as_deref())
                })
                .await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallDelete { handle } => {
                let firewall = backends.firewall.clone();
                let message = blocking(move || firewall.delete(handle)).await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallFlush { chain } => {
                let firewall = backends.firewall.clone();
                let message = blocking(move || firewall.flush(chain)).await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallSetPolicy { chain, policy } => {
                let firewall = backends.firewall.clone();
                let message = blocking(move || firewall.set_policy(chain, policy)).await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::Volumes => {
                let volumes = backends.volumes.clone();
                Ok(Response::Volumes(
                    blocking(move || Ok::<_, WireError>(volumes.snapshot())).await?,
                ))
            }
            Request::VolumesMount(request) => {
                let volumes = backends.volumes.clone();
                let message = blocking(move || volumes.mount(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumesUnmount(request) => {
                let volumes = backends.volumes.clone();
                let message = blocking(move || volumes.unmount(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumePartitionCreate(request) => {
                let volumes = backends.volumes.clone();
                let message = blocking(move || volumes.create_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumePartitionResize(request) => {
                let volumes = backends.volumes.clone();
                let message = blocking(move || volumes.resize_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumePartitionDelete(request) => {
                let volumes = backends.volumes.clone();
                let message = blocking(move || volumes.delete_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumeFormat(request) => {
                let volumes = backends.volumes.clone();
                let message = blocking(move || volumes.format_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::Accounts => {
                let accounts = backends.accounts.clone();
                Ok(Response::Accounts(
                    blocking(move || Ok::<_, WireError>(accounts.snapshot())).await?,
                ))
            }
            Request::AccountCreate(request) => {
                let accounts = backends.accounts.clone();
                let message = blocking(move || accounts.create_user(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountDelete(request) => {
                let accounts = backends.accounts.clone();
                let message = blocking(move || accounts.delete_user(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountSetPassword(request) => {
                let accounts = backends.accounts.clone();
                let message = blocking(move || accounts.set_password(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountGroupCreate(request) => {
                let accounts = backends.accounts.clone();
                let message = blocking(move || accounts.create_group(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountGroupDelete(request) => {
                let accounts = backends.accounts.clone();
                let message = blocking(move || accounts.delete_group(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::Restart => {
                let lifecycle = Arc::clone(&self.lifecycle);
                tokio::spawn(async move {
                    tokio::time::sleep(ACTION_GRACE).await;
                    lifecycle.restart();
                });
                Ok(Response::Restarting {
                    pid: std::process::id(),
                })
            }
            Request::SystemReboot { delay_s } => {
                self.lifecycle.authorize_reboot(delay_s)?;
                let lifecycle = Arc::clone(&self.lifecycle);
                tokio::spawn(async move {
                    tokio::time::sleep(ACTION_GRACE).await;
                    if let Err(error) = lifecycle.perform_reboot(delay_s) {
                        eprintln!("anth-panel: reboot failed: {error}");
                    }
                });
                Ok(Response::Rebooting { delay_s })
            }
            Request::FilesList {
                path,
                limit,
                cursor,
            } => {
                let request = PageRequest { limit, cursor };
                Ok(Response::FilesList(
                    files_of(&backends)?.list(&path, &request)?,
                ))
            }
            Request::FilesRead {
                path,
                offset,
                max_bytes,
            } => Ok(Response::FilesRead(
                files_of(&backends)?.read(&path, offset, max_bytes)?,
            )),
            Request::FilesDelete { path } => Ok(Response::FilesDeleted {
                path: files_of(&backends)?.delete(&path)?,
            }),
            Request::FilesWrite {
                path,
                content,
                create,
            } => Ok(Response::FilesWritten(
                files_of(&backends)?.write(&path, &content, create)?,
            )),
            Request::FilesUpload {
                path,
                content_base64,
                create,
            } => {
                let bytes = STANDARD
                    .decode(content_base64.as_bytes())
                    .map_err(|_| WireError::invalid("the upload content is not valid base64"))?;
                Ok(Response::FilesWritten(
                    files_of(&backends)?.write_bytes(&path, &bytes, create)?,
                ))
            }
            Request::FilesTrash { path } => {
                let (path, id) = files_of(&backends)?.trash(&path)?;
                Ok(Response::FilesTrashed { path, id })
            }
            Request::FilesTrashList => Ok(Response::FilesTrashList(
                files_of(&backends)?.trash_listing()?,
            )),
            Request::FilesTrashRestore { id, overwrite } => {
                let (id, path) = files_of(&backends)?.restore(&id, overwrite)?;
                Ok(Response::FilesRestored { id, path })
            }
            Request::FilesTrashEmpty => Ok(Response::FilesTrashEmptied {
                removed: files_of(&backends)?.empty_trash()?,
            }),
            Request::FilesArchive {
                path,
                destination,
                format,
            } => Ok(Response::FilesArchiveCreated {
                path: files_of(&backends)?.create_archive(&path, destination.as_deref(), format)?,
            }),
            Request::FilesArchiveList { path } => Ok(Response::FilesArchiveList(
                files_of(&backends)?.list_archive(&path)?,
            )),
            Request::FilesArchiveExtract { path, into } => {
                let (path, entries) =
                    files_of(&backends)?.extract_archive(&path, into.as_deref())?;
                Ok(Response::FilesArchiveExtracted { path, entries })
            }
            Request::TerminalOpen { cols, rows } => {
                let program = if self.demo_mode() {
                    let configured = &self.config.terminal.shell;
                    if configured == "auto" || !std::path::Path::new(configured).is_absolute() {
                        "/bin/sh".to_string()
                    } else {
                        configured.clone()
                    }
                } else {
                    resolve_shell(&self.config.terminal.shell).map_err(|detail| {
                        WireError::new(ErrorKind::Forbidden, "terminal_shell", detail)
                    })?
                };
                let size = TerminalSize { cols, rows };
                let line_editing = !self.demo_mode() && supports_line_editing(&program);
                let sessions = Arc::clone(&backends.terminal);
                let owner = owner.to_string();
                let asked = program.clone();
                let id = blocking(move || {
                    sessions
                        .open(&owner, &asked, size)
                        .map_err(OpenError::into_wire)
                })
                .await?;
                Ok(Response::TerminalOpened(TerminalOpened {
                    id,
                    program,
                    cols,
                    rows,
                    line_editing,
                }))
            }
            Request::TerminalRead { id, wait_ms } => {
                let chunk = backends.terminal.read(owner, &id, wait_ms).await?;
                Ok(Response::TerminalOutput(TerminalOutput {
                    id,
                    seq: chunk.seq,
                    bytes_base64: STANDARD.encode(chunk.bytes),
                    eof: chunk.eof,
                    exit_code: chunk.exit_code,
                }))
            }
            Request::TerminalWrite { id, bytes_base64 } => {
                let bytes = STANDARD
                    .decode(bytes_base64.as_bytes())
                    .map_err(|_| WireError::invalid("the terminal input is not valid base64"))?;
                if bytes.len() > MAX_TERMINAL_INPUT {
                    return Err(WireError::too_large(format!(
                        "terminal input is {} bytes, over the {MAX_TERMINAL_INPUT} byte limit",
                        bytes.len()
                    )));
                }
                let sessions = Arc::clone(&backends.terminal);
                let owner = owner.to_string();
                blocking(move || sessions.write(&owner, &id, &bytes)).await?;
                Ok(Response::Accepted)
            }
            Request::TerminalResize { id, cols, rows } => {
                backends
                    .terminal
                    .resize(owner, &id, TerminalSize { cols, rows })?;
                Ok(Response::Accepted)
            }
            Request::TerminalClose { id } => {
                let sessions = Arc::clone(&backends.terminal);
                let owner = owner.to_string();
                blocking(move || sessions.close(&owner, &id)).await?;
                Ok(Response::Accepted)
            }
            Request::Processes { limit, cursor } => {
                let request = PageRequest { limit, cursor };
                let process = backends.process.clone();
                let snapshot = blocking(move || process.snapshot(&request)).await?;
                Ok(Response::Processes(snapshot))
            }
            Request::ProcessSignal {
                pid,
                start_ticks,
                signal,
                group,
            } => {
                let process = backends.process.clone();
                blocking(move || process.signal(pid, start_ticks, signal, group)).await?;
                Ok(Response::ProcessSignalled { pid, signal })
            }
            Request::ProcessSpawn { program, args, cwd } => {
                let process = backends.process.clone();
                let started =
                    blocking(move || process.spawn(&program, &args, cwd.as_deref())).await?;
                Ok(Response::ProcessStarted(started))
            }
        }
    }
}

/// Runs a blocking PTY operation on the blocking pool.
///
/// `portable-pty` spawns processes and reaps them with `waitpid`; doing that on
/// a reactor thread would stall every other request.
async fn blocking<T, F>(task: F) -> Result<T, WireError>
where
    F: FnOnce() -> Result<T, WireError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| WireError::internal(format!("the blocking task failed: {error}")))?
}

fn files_of(backends: &Backends) -> Result<&Arc<dyn FilesService>, WireError> {
    backends
        .files
        .as_ref()
        .map_err(|error| WireError::internal(format!("the file root is unavailable: {error}")))
}

/// Which demo feature one request belongs to, when the demo can turn it off.
fn feature_of(request: &Request) -> Option<&'static str> {
    match request {
        Request::Network
        | Request::NetworkSetState { .. }
        | Request::NetworkCreate(_)
        | Request::NetworkEdit(_)
        | Request::NetworkRename { .. }
        | Request::NetworkDelete { .. } => Some("network"),
        Request::FirewallStatus
        | Request::FirewallAdd { .. }
        | Request::FirewallUpdate { .. }
        | Request::FirewallDelete { .. }
        | Request::FirewallFlush { .. }
        | Request::FirewallSetPolicy { .. } => Some("firewall"),
        Request::Volumes
        | Request::VolumesMount(_)
        | Request::VolumesUnmount(_)
        | Request::VolumePartitionCreate(_)
        | Request::VolumePartitionResize(_)
        | Request::VolumePartitionDelete(_)
        | Request::VolumeFormat(_) => Some("volumes"),
        Request::Accounts
        | Request::AccountCreate(_)
        | Request::AccountDelete(_)
        | Request::AccountSetPassword(_)
        | Request::AccountGroupCreate(_)
        | Request::AccountGroupDelete(_) => Some("accounts"),
        Request::FilesList { .. }
        | Request::FilesRead { .. }
        | Request::FilesDelete { .. }
        | Request::FilesWrite { .. }
        | Request::FilesUpload { .. }
        | Request::FilesTrash { .. }
        | Request::FilesTrashList
        | Request::FilesTrashRestore { .. }
        | Request::FilesTrashEmpty
        | Request::FilesArchive { .. }
        | Request::FilesArchiveList { .. }
        | Request::FilesArchiveExtract { .. } => Some("files"),
        Request::TerminalOpen { .. }
        | Request::TerminalRead { .. }
        | Request::TerminalWrite { .. }
        | Request::TerminalResize { .. }
        | Request::TerminalClose { .. } => Some("terminal"),
        Request::Processes { .. }
        | Request::ProcessSignal { .. }
        | Request::ProcessSpawn { .. } => Some("processes"),
        _ => None,
    }
}

fn operation_of(request: &Request) -> &'static str {
    match request {
        Request::HostInfo => "host-info",
        Request::Network => "network",
        Request::NetworkSetState { .. } => "network-set-state",
        Request::NetworkCreate(_) => "network-create",
        Request::NetworkEdit(_) => "network-edit",
        Request::NetworkRename { .. } => "network-rename",
        Request::NetworkDelete { .. } => "network-delete",
        Request::FirewallStatus => "firewall-status",
        Request::FirewallAdd { .. } => "firewall-add",
        Request::FirewallUpdate { .. } => "firewall-update",
        Request::FirewallDelete { .. } => "firewall-delete",
        Request::FirewallFlush { .. } => "firewall-flush",
        Request::FirewallSetPolicy { .. } => "firewall-set-policy",
        Request::Volumes => "volumes",
        Request::VolumesMount(_) => "volumes-mount",
        Request::VolumesUnmount(_) => "volumes-unmount",
        Request::VolumePartitionCreate(_) => "volume-partition-create",
        Request::VolumePartitionResize(_) => "volume-partition-resize",
        Request::VolumePartitionDelete(_) => "volume-partition-delete",
        Request::VolumeFormat(_) => "volume-format",
        Request::Accounts => "accounts",
        Request::AccountCreate(_) => "account-create",
        Request::AccountDelete(_) => "account-delete",
        Request::AccountSetPassword(_) => "account-set-password",
        Request::AccountGroupCreate(_) => "account-group-create",
        Request::AccountGroupDelete(_) => "account-group-delete",
        Request::Restart => "restart",
        Request::SystemReboot { .. } => "system-reboot",
        Request::FilesList { .. } => "files-list",
        Request::FilesRead { .. } => "files-read",
        Request::FilesDelete { .. } => "files-delete",
        Request::FilesWrite { .. } => "files-write",
        Request::FilesUpload { .. } => "files-upload",
        Request::FilesTrash { .. } => "files-trash",
        Request::FilesTrashList => "files-trash-list",
        Request::FilesTrashRestore { .. } => "files-trash-restore",
        Request::FilesTrashEmpty => "files-trash-empty",
        Request::FilesArchive { .. } => "files-archive",
        Request::FilesArchiveList { .. } => "files-archive-list",
        Request::FilesArchiveExtract { .. } => "files-archive-extract",
        Request::TerminalOpen { .. } => "terminal-open",
        Request::TerminalRead { .. } => "terminal-read",
        Request::TerminalWrite { .. } => "terminal-write",
        Request::TerminalResize { .. } => "terminal-resize",
        Request::TerminalClose { .. } => "terminal-close",
        Request::Processes { .. } => "processes",
        Request::ProcessSignal { .. } => "process-signal",
        Request::ProcessSpawn { .. } => "process-spawn",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::lifecycle::{Recorded, recorded};
    use crate::log::Settings;

    fn daemon(allow_reboot: bool) -> (Arc<Daemon>, Arc<Recorded>) {
        let config = Config::dev_default();
        let paths = config
            .paths("/tmp/anthracite-panel/tests/dispatch/config.toml")
            .expect("paths");
        let (lifecycle, recorder) = recorded(allow_reboot);
        let logger = Arc::new(Logger::buffer(Settings::default()).0);
        (
            Arc::new(Daemon::with_lifecycle(
                config,
                paths,
                logger,
                Arc::new(lifecycle),
            )),
            recorder,
        )
    }

    #[tokio::test]
    async fn restart_is_answered_before_the_action() {
        let (daemon, recorder) = daemon(false);
        let response = daemon.execute(Request::Restart).await;
        assert!(matches!(response, Response::Restarting { .. }));
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(200)).await;
        assert_eq!(recorder.restarts.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reboot_without_the_switch_is_refused_and_not_scheduled() {
        let (daemon, recorder) = daemon(false);
        let response = daemon.execute(Request::SystemReboot { delay_s: 0 }).await;
        assert_eq!(response.error().expect("error").kind, ErrorKind::Forbidden);
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(100)).await;
        assert!(recorder.reboots.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_authorized_reboot_is_scheduled() {
        let (daemon, recorder) = daemon(true);
        let response = daemon.execute(Request::SystemReboot { delay_s: 7 }).await;
        assert!(matches!(response, Response::Rebooting { delay_s: 7 }));
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(200)).await;
        assert_eq!(recorder.reboots.lock().unwrap().as_slice(), &[7]);
    }
}
